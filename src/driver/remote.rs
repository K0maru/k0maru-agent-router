//! Remote SSH Multiplexer Driver for Tailscale and Cross-Host Dispatch.
//!
//! Enables local cockpit agents to query, control, and dispatch tasks to remote
//! Linux or macOS workstations running `tmux` or `herdr` over Tailscale / SSH.

use super::{LiveSession, MultiplexerDriver, SessionStatus};
use crate::driver::herdr::{HerdrPaneListResponse, HerdrPaneSplitResponse};
use crate::driver::tmux::{parse_pane_line, parse_tmux_output};
use crate::error::RouterError;
use crate::pipeline::{PipelineConfig, PipelineRunReport};
use crate::receipt::ExecutionReceipt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Target multiplexer backend on the remote host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RemoteBackend {
    /// Automatically probe whether `herdr` or `tmux` is running on the remote host.
    #[default]
    Auto,
    /// Force `tmux` commands over SSH.
    Tmux,
    /// Force `herdr` commands over SSH.
    Herdr,
}

impl std::str::FromStr for RemoteBackend {
    type Err = RouterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "tmux" => Ok(Self::Tmux),
            "herdr" => Ok(Self::Herdr),
            other => Err(RouterError::InvalidEnvelope(format!(
                "Unknown remote multiplexer backend: '{other}'. Expected 'auto', 'tmux', or 'herdr'"
            ))),
        }
    }
}

/// Remote SSH driver managing terminal multiplexers (`tmux` / `herdr`) across Tailscale or SSH hosts.
#[derive(Debug, Clone)]
pub struct RemoteSshDriver {
    /// Remote host address, Tailscale MagicDNS name, or SSH config alias (e.g. `company-linux`, `user@100.88.22.33`).
    pub node: String,
    /// Target multiplexer flavor on the remote machine.
    pub backend: RemoteBackend,
    /// Path or name of the local SSH executable (defaults to `ssh`).
    pub ssh_bin: String,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u32,
}

impl RemoteSshDriver {
    /// Creates a new `RemoteSshDriver` for the specified remote node.
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            backend: RemoteBackend::Auto,
            ssh_bin: "ssh".to_string(),
            connect_timeout_secs: 5,
        }
    }

    /// Sets the multiplexer backend preference.
    pub fn with_backend(mut self, backend: RemoteBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Sets a custom SSH binary or command (e.g. `tailscale ssh`).
    pub fn with_ssh_bin(mut self, bin: impl Into<String>) -> Self {
        self.ssh_bin = bin.into();
        self
    }

    /// Sets the connection timeout in seconds.
    pub fn with_timeout(mut self, secs: u32) -> Self {
        self.connect_timeout_secs = secs;
        self
    }

    /// Executes a remote command string on the target host via SSH, optionally passing stdin data.
    pub fn run_ssh(
        &self,
        remote_cmd: &str,
        stdin_data: Option<&str>,
    ) -> Result<std::process::Output, RouterError> {
        let mut cmd = Command::new(&self.ssh_bin);
        cmd.arg("-o")
            .arg(format!("ConnectTimeout={}", self.connect_timeout_secs));
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg(&self.node);
        cmd.arg(remote_cmd);

        if let Some(input) = stdin_data {
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            let mut child = cmd.spawn().map_err(|e| {
                RouterError::DriverExecutionFailed(format!(
                    "Failed to spawn SSH process to '{}': {e}",
                    self.node
                ))
            })?;

            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes());
            }

            child.wait_with_output().map_err(|e| {
                RouterError::DriverExecutionFailed(format!(
                    "SSH process to '{}' failed to complete: {e}",
                    self.node
                ))
            })
        } else {
            cmd.output().map_err(|e| {
                RouterError::DriverExecutionFailed(format!(
                    "Failed to execute SSH to '{}': {e}",
                    self.node
                ))
            })
        }
    }

    /// Probes and resolves the concrete multiplexer backend on the remote host.
    pub fn detect_backend(&self) -> Result<RemoteBackend, RouterError> {
        match self.backend {
            RemoteBackend::Tmux => Ok(RemoteBackend::Tmux),
            RemoteBackend::Herdr => Ok(RemoteBackend::Herdr),
            RemoteBackend::Auto => {
                let probe_cmd = "if command -v herdr >/dev/null 2>&1 && herdr status >/dev/null 2>&1; then echo herdr; elif command -v tmux >/dev/null 2>&1; then echo tmux; else echo none; fi";
                let output = self.run_ssh(probe_cmd, None)?;
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    return Err(RouterError::DriverUnavailable {
                        driver: "remote-ssh",
                        reason: format!(
                            "Failed to probe remote node '{}': {}",
                            self.node,
                            err.trim()
                        ),
                    });
                }
                let detected = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_lowercase();
                match detected.as_str() {
                    "herdr" => Ok(RemoteBackend::Herdr),
                    "tmux" => Ok(RemoteBackend::Tmux),
                    _ => Err(RouterError::DriverUnavailable {
                        driver: "remote-ssh",
                        reason: format!(
                            "No supported multiplexer (`herdr` or `tmux`) found active on remote host '{}'",
                            self.node
                        ),
                    }),
                }
            }
        }
    }

    /// Executes an end-to-end task pipeline on the remote host via `k0maru-router pipeline run`.
    pub fn run_remote_pipeline(
        &self,
        config: &PipelineConfig,
    ) -> Result<PipelineRunReport, RouterError> {
        let mut cmd_parts = vec![
            "k0maru-router".to_string(),
            "pipeline".to_string(),
            "run".to_string(),
            "--task-id".to_string(),
            config.task_id.clone(),
            "--workspace".to_string(),
            config.target_workspace.to_string_lossy().to_string(),
            "--json".to_string(),
        ];

        if let Some(ref intent) = config.intent {
            cmd_parts.push("--intent".to_string());
            cmd_parts.push(intent.clone());
        }

        if config.isolate {
            cmd_parts.push("--isolate".to_string());
        }
        if config.verify {
            cmd_parts.push("--verify".to_string());
        }
        if config.auto_merge {
            cmd_parts.push("--merge".to_string());
        }
        if config.emit_signal {
            cmd_parts.push("--signal".to_string());
        }
        if config.crystallize_memory {
            cmd_parts.push("--flush".to_string());
        }
        if let Some(ref r) = config.runner {
            cmd_parts.push("--runner".to_string());
            cmd_parts.push(r.clone());
        }
        if let Some(ref p) = config.patch {
            cmd_parts.push("--patch".to_string());
            cmd_parts.push(p.to_string_lossy().to_string());
        }
        if !config.allowed_files.is_empty() {
            cmd_parts.push("--allowed-files".to_string());
            cmd_parts.push(config.allowed_files.join(","));
        }

        let remote_cmd = cmd_parts.join(" ");
        let out = self.run_ssh(&remote_cmd, None)?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(RouterError::DriverExecutionFailed(format!(
                "Remote pipeline failed on '{}': {}\n{}",
                self.node,
                stderr.trim(),
                stdout.trim()
            )));
        }

        let report: PipelineRunReport = serde_json::from_slice(&out.stdout).map_err(|e| {
            RouterError::DriverExecutionFailed(format!(
                "Failed to deserialize remote pipeline report from '{}': {e}",
                self.node
            ))
        })?;

        Ok(report)
    }

    /// Fetches an execution receipt from the remote host's receipt bus.
    pub fn fetch_remote_receipt(
        &self,
        task_id: &str,
    ) -> Result<Option<ExecutionReceipt>, RouterError> {
        let remote_cmd = format!("k0maru-router receipt status {} --json", task_id);
        let out = self.run_ssh(&remote_cmd, None)?;
        if !out.status.success() {
            return Ok(None);
        }

        let receipt: ExecutionReceipt = serde_json::from_slice(&out.stdout).map_err(|e| {
            RouterError::DriverExecutionFailed(format!(
                "Failed to deserialize remote receipt from '{}': {e}",
                self.node
            ))
        })?;

        Ok(Some(receipt))
    }
}

impl MultiplexerDriver for RemoteSshDriver {
    fn name(&self) -> &'static str {
        match self.backend {
            RemoteBackend::Herdr => "remote-herdr",
            RemoteBackend::Tmux => "remote-tmux",
            RemoteBackend::Auto => "remote-ssh",
        }
    }

    fn is_available(&self) -> bool {
        self.run_ssh("true", None)
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        let backend = self.detect_backend()?;
        match backend {
            RemoteBackend::Herdr => {
                let out = self.run_ssh("herdr pane list", None)?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(RouterError::DriverExecutionFailed(format!(
                        "Remote herdr pane list failed on '{}': {}",
                        self.node,
                        err.trim()
                    )));
                }

                let resp: HerdrPaneListResponse =
                    serde_json::from_slice(&out.stdout).map_err(|e| {
                        RouterError::DriverExecutionFailed(format!(
                            "Failed to parse remote herdr pane list on '{}': {e}",
                            self.node
                        ))
                    })?;

                let mut sessions = Vec::new();
                if let Some(res) = resp.result {
                    if let Some(panes) = res.panes {
                        for pane in panes {
                            let status = match pane.agent_status.as_deref() {
                                Some("working") => SessionStatus::Working,
                                _ => SessionStatus::Idle,
                            };
                            sessions.push(LiveSession::new(
                                pane.pane_id,
                                PathBuf::from(pane.cwd),
                                status,
                                pane.agent,
                            ));
                        }
                    }
                }
                Ok(sessions)
            }
            RemoteBackend::Tmux | RemoteBackend::Auto => {
                let tmux_cmd = "tmux list-panes -a -F '#{pane_id}|#{pane_current_path}|#{pane_current_command}' 2>/dev/null || true";
                let out = self.run_ssh(tmux_cmd, None)?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(RouterError::DriverExecutionFailed(format!(
                        "Remote tmux list-panes failed on '{}': {}",
                        self.node,
                        err.trim()
                    )));
                }
                let raw_stdout = String::from_utf8_lossy(&out.stdout);
                Ok(parse_tmux_output(&raw_stdout))
            }
        }
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        let backend = self.detect_backend()?;
        match backend {
            RemoteBackend::Herdr => {
                // Try agent prompt via stdin to avoid shell escaping glitches
                let remote_cmd = format!("herdr agent prompt {} \"$(cat)\"", pane_id);
                let out = self.run_ssh(&remote_cmd, Some(prompt))?;
                if out.status.success() {
                    return Ok(());
                }

                // Fallback to pane run
                let remote_fallback = format!("herdr pane run {} \"$(cat)\"", pane_id);
                let fallback_out = self.run_ssh(&remote_fallback, Some(prompt))?;
                if fallback_out.status.success() {
                    Ok(())
                } else {
                    let err = String::from_utf8_lossy(&fallback_out.stderr);
                    Err(RouterError::DriverExecutionFailed(format!(
                        "Remote herdr send_prompt failed on '{}': {}",
                        self.node,
                        err.trim()
                    )))
                }
            }
            RemoteBackend::Tmux | RemoteBackend::Auto => {
                // Pipe prompt into tmux load-buffer and paste into pane
                let remote_cmd = format!(
                    "tmux load-buffer - && tmux paste-buffer -t {} && tmux send-keys -t {} Enter",
                    pane_id, pane_id
                );
                let out = self.run_ssh(&remote_cmd, Some(prompt))?;
                if out.status.success() {
                    Ok(())
                } else {
                    let err = String::from_utf8_lossy(&out.stderr);
                    Err(RouterError::DriverExecutionFailed(format!(
                        "Remote tmux send_prompt failed on '{}': {}",
                        self.node,
                        err.trim()
                    )))
                }
            }
        }
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        let backend = self.detect_backend()?;
        let ws_str = workspace.to_string_lossy();
        match backend {
            RemoteBackend::Herdr => {
                let cmd_str = match command {
                    Some(c) => format!("herdr pane split -c '{}' '{}'", ws_str, c),
                    None => format!("herdr pane split -c '{}'", ws_str),
                };
                let out = self.run_ssh(&cmd_str, None)?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(RouterError::DriverExecutionFailed(format!(
                        "Remote herdr pane split failed on '{}': {}",
                        self.node,
                        err.trim()
                    )));
                }

                let resp: HerdrPaneSplitResponse =
                    serde_json::from_slice(&out.stdout).map_err(|e| {
                        RouterError::DriverExecutionFailed(format!(
                            "Failed to parse remote herdr split response on '{}': {e}",
                            self.node
                        ))
                    })?;

                if let Some(res) = resp.result {
                    if let Some(pane) = res.pane {
                        let final_cwd = pane
                            .cwd
                            .map(PathBuf::from)
                            .unwrap_or_else(|| workspace.to_path_buf());
                        return Ok(LiveSession::new(
                            pane.pane_id,
                            final_cwd,
                            SessionStatus::Idle,
                            None,
                        ));
                    }
                }

                Err(RouterError::DriverExecutionFailed(
                    "Remote herdr split succeeded but returned empty pane details".to_string(),
                ))
            }
            RemoteBackend::Tmux | RemoteBackend::Auto => {
                let cmd_str = match command {
                    Some(cmd) => format!(
                        "tmux split-window -c '{}' -P -F '#{{pane_id}}|#{{pane_current_path}}|#{{pane_current_command}}' '{}'",
                        ws_str, cmd
                    ),
                    None => format!(
                        "tmux split-window -c '{}' -P -F '#{{pane_id}}|#{{pane_current_path}}|#{{pane_current_command}}'",
                        ws_str
                    ),
                };
                let out = self.run_ssh(&cmd_str, None)?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    return Err(RouterError::DriverExecutionFailed(format!(
                        "Remote tmux split-window failed on '{}': {}",
                        self.node,
                        err.trim()
                    )));
                }

                let line = String::from_utf8_lossy(&out.stdout);
                parse_pane_line(line.trim()).ok_or_else(|| {
                    RouterError::DriverExecutionFailed(format!(
                        "Failed to parse remote pane line: '{}'",
                        line.trim()
                    ))
                })
            }
        }
    }
}
