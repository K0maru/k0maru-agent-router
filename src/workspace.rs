//! Workspace Provider SPI and Sandbox Isolation Adapters.
//!
//! Conforms to SPEC.md §7.
//! Decouples workspace sandboxing / git-worktree management from the core routing engine.

use std::fmt;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::RouterError;

/// Workspace isolation and sandbox provisioning service provider interface.
#[async_trait]
pub trait WorkspaceProvider: Send + Sync + fmt::Debug {
    /// Provider name identifier (e.g. "in_place", "cli").
    fn name(&self) -> &'static str;

    /// Returns `true` if this workspace provider is available on the host system.
    fn is_available(&self) -> bool;

    /// Provisions an isolated sandbox workspace for the specified task.
    ///
    /// Returns the absolute path to the sandbox directory.
    async fn provision_sandbox(
        &self,
        task_id: &str,
        repo_root: &Path,
        base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError>;

    /// Tears down the sandbox workspace associated with the specified task.
    async fn teardown_sandbox(&self, task_id: &str, repo_root: &Path) -> Result<(), RouterError>;
}

/// Default zero-overhead pass-through provider returning the original repository root directly.
#[derive(Debug, Clone, Copy, Default)]
pub struct InPlaceWorkspaceProvider;

#[async_trait]
impl WorkspaceProvider for InPlaceWorkspaceProvider {
    fn name(&self) -> &'static str {
        "in_place"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn provision_sandbox(
        &self,
        _task_id: &str,
        repo_root: &Path,
        _base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError> {
        Ok(repo_root.to_path_buf())
    }

    async fn teardown_sandbox(&self, _task_id: &str, _repo_root: &Path) -> Result<(), RouterError> {
        Ok(())
    }
}

/// CLI-driven workspace provider interfacing with external `k0maru-workspace` binary via subprocess.
#[derive(Debug, Clone)]
pub struct CliWorkspaceProvider {
    binary_path: PathBuf,
}

impl Default for CliWorkspaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CliWorkspaceProvider {
    /// Creates a provider searching for `k0maru-workspace` on PATH and `~/.cargo/bin`.
    pub fn new() -> Self {
        Self::with_binary_name("k0maru-workspace")
    }

    /// Creates a provider probing for the specified binary name or path.
    pub fn with_binary_name(name: impl AsRef<Path>) -> Self {
        let name_path = name.as_ref();
        if name_path.is_file() {
            return Self {
                binary_path: name_path.to_path_buf(),
            };
        }

        // Check ~/.cargo/bin/<name>
        if let Some(home) = std::env::var_os("HOME") {
            let cargo_bin = PathBuf::from(home).join(".cargo/bin").join(name_path);
            if cargo_bin.is_file() {
                return Self {
                    binary_path: cargo_bin,
                };
            }
        }

        // Check PATH
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(name_path);
                if candidate.is_file() {
                    return Self {
                        binary_path: candidate,
                    };
                }
            }
        }

        Self {
            binary_path: name_path.to_path_buf(),
        }
    }

    /// Creates a provider with an explicit binary path.
    pub fn with_binary_path(path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: path.into(),
        }
    }

    /// Returns the resolved binary path.
    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }
}

#[async_trait]
impl WorkspaceProvider for CliWorkspaceProvider {
    fn name(&self) -> &'static str {
        "cli"
    }

    fn is_available(&self) -> bool {
        self.binary_path.is_file()
    }

    async fn provision_sandbox(
        &self,
        task_id: &str,
        repo_root: &Path,
        base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError> {
        if !self.is_available() {
            return Err(RouterError::WorkspaceError(format!(
                "Workspace CLI binary '{}' not found or not executable",
                self.binary_path.display()
            )));
        }

        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.arg("provision")
            .arg("--task-id")
            .arg(task_id)
            .arg("--repo")
            .arg(repo_root);

        if let Some(branch) = base_branch {
            cmd.arg("--base-branch").arg(branch);
        }

        let output = cmd.output().await.map_err(|e| {
            RouterError::WorkspaceError(format!(
                "Failed to execute workspace CLI '{}': {e}",
                self.binary_path.display()
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RouterError::WorkspaceError(format!(
                "Workspace provision failed with exit code {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let path_str = stdout.trim();
        if path_str.is_empty() {
            return Err(RouterError::WorkspaceError(
                "Workspace provision succeeded but returned empty sandbox path on stdout"
                    .to_string(),
            ));
        }

        Ok(PathBuf::from(path_str))
    }

    async fn teardown_sandbox(&self, task_id: &str, repo_root: &Path) -> Result<(), RouterError> {
        if !self.is_available() {
            return Err(RouterError::WorkspaceError(format!(
                "Workspace CLI binary '{}' not found or not executable",
                self.binary_path.display()
            )));
        }

        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.arg("teardown")
            .arg("--task-id")
            .arg(task_id)
            .arg("--repo")
            .arg(repo_root);

        let output = cmd.output().await.map_err(|e| {
            RouterError::WorkspaceError(format!(
                "Failed to execute workspace CLI '{}': {e}",
                self.binary_path.display()
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RouterError::WorkspaceError(format!(
                "Workspace teardown failed with exit code {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        Ok(())
    }
}

/// Resilient decorator wrapping any `WorkspaceProvider` to guarantee graceful degradation.
///
/// If the underlying provider is unavailable or fails, `OptionalWorkspaceProvider`
/// intercepts the failure, logs a notice, and transparently falls back to `InPlaceWorkspaceProvider`.
#[derive(Debug, Clone)]
pub struct OptionalWorkspaceProvider<P: WorkspaceProvider> {
    inner: P,
    fallback: InPlaceWorkspaceProvider,
    enabled: bool,
}

impl<P: WorkspaceProvider> OptionalWorkspaceProvider<P> {
    /// Wraps an existing workspace provider with graceful fallback protection.
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            fallback: InPlaceWorkspaceProvider,
            enabled: true,
        }
    }

    /// Sets whether sandbox isolation is explicitly enabled or disabled.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Returns whether this provider is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns a reference to the wrapped inner provider.
    pub fn inner(&self) -> &P {
        &self.inner
    }

    /// Returns `true` if the inner provider is enabled and available.
    pub fn is_inner_available(&self) -> bool {
        self.enabled && self.inner.is_available()
    }
}

#[async_trait]
impl<P: WorkspaceProvider> WorkspaceProvider for OptionalWorkspaceProvider<P> {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn is_available(&self) -> bool {
        // Fallback InPlace is always available
        true
    }

    async fn provision_sandbox(
        &self,
        task_id: &str,
        repo_root: &Path,
        base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError> {
        if !self.is_inner_available() {
            return self
                .fallback
                .provision_sandbox(task_id, repo_root, base_branch)
                .await;
        }

        match self
            .inner
            .provision_sandbox(task_id, repo_root, base_branch)
            .await
        {
            Ok(path) => Ok(path),
            Err(e) => {
                eprintln!(
                    "[WorkspaceProvider] Notice: Provider '{}' failed ({e}), falling back to in-place workspace",
                    self.inner.name()
                );
                self.fallback
                    .provision_sandbox(task_id, repo_root, base_branch)
                    .await
            }
        }
    }

    async fn teardown_sandbox(&self, task_id: &str, repo_root: &Path) -> Result<(), RouterError> {
        if !self.is_inner_available() {
            return self.fallback.teardown_sandbox(task_id, repo_root).await;
        }

        match self.inner.teardown_sandbox(task_id, repo_root).await {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!(
                    "[WorkspaceProvider] Notice: Provider '{}' teardown failed ({e}), falling back to in-place teardown",
                    self.inner.name()
                );
                self.fallback.teardown_sandbox(task_id, repo_root).await
            }
        }
    }
}
