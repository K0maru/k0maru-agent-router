//! Cognitive Memory Hook SPI & Graceful Decoupling.
//!
//! Provides optional architectural blueprint preloading via `k0maru-agent-memory`
//! while upholding the Unix Modular Decoupling Invariant (SPEC.md §5).

use std::path::Path;

use crate::error::RouterError;

/// Optional Cognitive Memory Hook Service Provider Interface.
pub trait MemoryHook: Send + Sync + std::fmt::Debug {
    /// Name of the memory provider (e.g. "noop", "k0maru-memory").
    fn name(&self) -> &'static str;

    /// Whether the memory provider is currently available on the host system.
    fn is_available(&self) -> bool;

    /// Attempts to preload architectural context or blueprint for a given workspace.
    ///
    /// Returns `Ok(Some(context))` if found, `Ok(None)` if unavailable or disabled.
    fn preload_context(
        &self,
        workspace: &Path,
        project_name: &str,
    ) -> Result<Option<String>, RouterError>;
}

/// Default zero-overhead memory hook returning `Ok(None)`.
#[derive(Debug, Clone, Default)]
pub struct NoopMemoryHook;

impl MemoryHook for NoopMemoryHook {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn preload_context(
        &self,
        _workspace: &Path,
        _project_name: &str,
    ) -> Result<Option<String>, RouterError> {
        Ok(None)
    }
}

/// Resilient decorator wrapping any `MemoryHook` to guarantee graceful degradation.
///
/// If the underlying hook fails, errors, or times out, `OptionalMemoryHook` intercepts
/// the failure, logs a debug trace, and returns `Ok(None)` to prevent breaking the routing pipeline.
#[derive(Debug, Clone)]
pub struct OptionalMemoryHook<H: MemoryHook> {
    inner: H,
    enabled: bool,
}

impl<H: MemoryHook> OptionalMemoryHook<H> {
    /// Wraps an existing memory hook with graceful degradation protection.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            enabled: true,
        }
    }

    /// Sets whether memory preloading is explicitly enabled or disabled.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Returns a reference to the wrapped inner hook.
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// Returns whether this hook is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl<H: MemoryHook> MemoryHook for OptionalMemoryHook<H> {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn is_available(&self) -> bool {
        self.enabled && self.inner.is_available()
    }

    fn preload_context(
        &self,
        workspace: &Path,
        project_name: &str,
    ) -> Result<Option<String>, RouterError> {
        if !self.enabled || !self.inner.is_available() {
            return Ok(None);
        }

        // Catch any error from inner provider and degrade cleanly to Ok(None)
        match self.inner.preload_context(workspace, project_name) {
            Ok(context) => Ok(context),
            Err(e) => {
                // Non-blocking degradation: router pipeline must never fail due to memory hook
                eprintln!(
                    "[k0maru-router] Memory hook '{}' skipped (provider error): {}",
                    self.name(),
                    e
                );
                Ok(None)
            }
        }
    }
}
