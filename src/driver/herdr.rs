//! Herdr CLI implementation of `MultiplexerDriver`.

use super::{LiveSession, MultiplexerDriver, SessionStatus};
use crate::addressing::normalize_workspace_path;
use crate::error::RouterError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// CLI driver for `herdr` multiplexer.
#[derive(Debug, Clone, Default)]
pub struct HerdrDriver;

#[derive(Deserialize, Debug)]
struct HerdrPaneListResponse {
    result: Option<HerdrPaneListResult>,
}

#[derive(Deserialize, Debug)]
struct HerdrPaneListResult {
    panes: Option<Vec<HerdrPaneItem>>,
}

#[derive(Deserialize, Debug)]
struct HerdrPaneItem {
    pane_id: String,
    cwd: String,
    agent: Option<String>,
    agent_status: Option<String>,
}

#[derive(Deserialize, Debug)]
struct HerdrPaneSplitResponse {
    result: Option<HerdrPaneSplitResult>,
}

#[derive(Deserialize, Debug)]
struct HerdrPaneSplitResult {
    pane: Option<HerdrPaneInfo>,
}

#[derive(Deserialize, Debug)]
struct HerdrPaneInfo {
    pane_id: String,
    cwd: Option<String>,
}

impl HerdrDriver {
    /// Creates a new `HerdrDriver`.
    pub fn new() -> Self {
        Self
    }
}

impl MultiplexerDriver for HerdrDriver {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn is_available(&self) -> bool {
        Command::new("herdr")
            .arg("status")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "herdr",
                reason: "herdr server is not running or binary not found".to_string(),
            });
        }

        let output = Command::new("herdr")
            .args(["pane", "list"])
            .output()
            .map_err(|e| RouterError::DriverExecutionFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RouterError::DriverExecutionFailed(stderr.trim().to_string()));
        }

        let resp: HerdrPaneListResponse = serde_json::from_slice(&output.stdout).map_err(|e| {
            RouterError::DriverExecutionFailed(format!("Failed to parse herdr pane list: {e}"))
        })?;

        let mut sessions = Vec::new();
        if let Some(res) = resp.result {
            if let Some(panes) = res.panes {
                for pane in panes {
                    let status = match pane.agent_status.as_deref() {
                        Some("working") => SessionStatus::Working,
                        _ => SessionStatus::Idle,
                    };
                    let norm_cwd = normalize_workspace_path(&pane.cwd)
                        .unwrap_or_else(|_| PathBuf::from(&pane.cwd));
                    sessions.push(LiveSession::new(
                        pane.pane_id,
                        norm_cwd,
                        status,
                        pane.agent,
                    ));
                }
            }
        }
        Ok(sessions)
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "herdr",
                reason: "herdr server is not running".to_string(),
            });
        }

        // 1. Try agent prompt first (honors bracketed paste mode if agent is attached)
        let agent_out = Command::new("herdr")
            .args(["agent", "prompt", pane_id, prompt])
            .output();

        if let Ok(ref out) = agent_out {
            if out.status.success() {
                return Ok(());
            }
        }

        // 2. Fallback to pane run for standard shells or non-agent panes
        let pane_out = Command::new("herdr")
            .args(["pane", "run", pane_id, prompt])
            .output()
            .map_err(|e| RouterError::DriverExecutionFailed(e.to_string()))?;

        if pane_out.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&pane_out.stderr);
            Err(RouterError::DriverExecutionFailed(stderr.trim().to_string()))
        }
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "herdr",
                reason: "herdr server is not running".to_string(),
            });
        }

        let mut cmd = Command::new("herdr");
        cmd.args([
            "pane",
            "split",
            "--direction",
            "right",
            "--cwd",
            workspace.to_str().unwrap_or("."),
            "--no-focus",
        ]);

        let output = cmd
            .output()
            .map_err(|e| RouterError::DriverExecutionFailed(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RouterError::DriverExecutionFailed(stderr.trim().to_string()));
        }

        let resp: HerdrPaneSplitResponse = serde_json::from_slice(&output.stdout).map_err(|e| {
            RouterError::DriverExecutionFailed(format!(
                "Failed to parse herdr pane split response: {e}"
            ))
        })?;

        let pane = resp
            .result
            .and_then(|r| r.pane)
            .ok_or_else(|| {
                RouterError::DriverExecutionFailed(
                    "Herdr pane split response missing pane details".to_string(),
                )
            })?;

        let cwd_path: PathBuf = pane
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.to_path_buf());
        let norm_cwd = normalize_workspace_path(&cwd_path)
            .unwrap_or_else(|_| cwd_path);

        Ok(LiveSession::new(
            pane.pane_id,
            norm_cwd,
            SessionStatus::Idle,
            command.map(|s| s.to_string()),
        ))
    }
}
