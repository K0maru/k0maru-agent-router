//! POSIX CLI implementation of `MultiplexerDriver` for Tmux.

use super::{LiveSession, MultiplexerDriver, SessionStatus};
use crate::addressing::normalize_workspace_path;
use crate::error::RouterError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// POSIX CLI driver for `tmux`.
#[derive(Debug, Clone, Default)]
pub struct TmuxDriver {
    /// Optional isolated socket name for testing or separate server instances (`-L <socket>`).
    socket: Option<String>,
}

impl TmuxDriver {
    /// Creates a new `TmuxDriver` targeting the default system tmux server socket.
    pub fn new() -> Self {
        Self { socket: None }
    }

    /// Creates a new `TmuxDriver` targeting an isolated socket name (`tmux -L <socket>`).
    pub fn with_socket(socket: impl Into<String>) -> Self {
        Self {
            socket: Some(socket.into()),
        }
    }

    /// Returns the configured socket name, if any.
    pub fn socket(&self) -> Option<&str> {
        self.socket.as_deref()
    }

    /// Helper to build a `Command` preconfigured with the optional socket flag.
    fn command(&self) -> Command {
        let mut cmd = Command::new("tmux");
        if let Some(ref sock) = self.socket {
            cmd.arg("-L").arg(sock);
        }
        cmd
    }
}

impl MultiplexerDriver for TmuxDriver {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn is_available(&self) -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "tmux",
                reason: "tmux binary not found in PATH".to_string(),
            });
        }

        let output = self
            .command()
            .args([
                "list-panes",
                "-a",
                "-F",
                "#{pane_id}\t#{pane_current_path}\t#{pane_current_command}",
            ])
            .output()
            .map_err(|err| RouterError::DriverExecutionFailed(err.to_string()))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(parse_tmux_output(&stdout))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_lower = stderr.to_lowercase();
            // Graceful offline detection: if no server is running, return empty list
            if stderr_lower.contains("no server running")
                || stderr_lower.contains("error connecting to")
                || stderr_lower.contains("failed to connect to server")
                || stderr_lower.contains("no sessions")
                || stderr_lower.contains("no current target")
            {
                Ok(Vec::new())
            } else {
                Err(RouterError::DriverExecutionFailed(
                    stderr.trim().to_string(),
                ))
            }
        }
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "tmux",
                reason: "tmux binary not found in PATH".to_string(),
            });
        }

        // 1. Send literal prompt string without shell expansion or key name interpretation
        let send_literal = self
            .command()
            .args(["send-keys", "-t", pane_id, "-l", "--", prompt])
            .output()
            .map_err(|err| RouterError::DriverExecutionFailed(err.to_string()))?;

        if !send_literal.status.success() {
            let stderr = String::from_utf8_lossy(&send_literal.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("can't find pane")
                || stderr_lower.contains("can't find session")
                || stderr_lower.contains("session not found")
                || stderr_lower.contains("no server running")
                || stderr_lower.contains("error connecting to")
                || stderr_lower.contains("failed to connect to server")
            {
                return Err(RouterError::SessionNotFound(pane_id.to_string()));
            }
            return Err(RouterError::DriverExecutionFailed(
                stderr.trim().to_string(),
            ));
        }

        // 2. Send Enter key to submit
        let send_enter = self
            .command()
            .args(["send-keys", "-t", pane_id, "Enter"])
            .output()
            .map_err(|err| RouterError::DriverExecutionFailed(err.to_string()))?;

        if !send_enter.status.success() {
            let stderr = String::from_utf8_lossy(&send_enter.stderr);
            let stderr_lower = stderr.to_lowercase();
            if stderr_lower.contains("can't find pane")
                || stderr_lower.contains("can't find session")
                || stderr_lower.contains("session not found")
            {
                return Err(RouterError::SessionNotFound(pane_id.to_string()));
            }
            return Err(RouterError::DriverExecutionFailed(
                stderr.trim().to_string(),
            ));
        }

        Ok(())
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        // Truth Invariant: Pre-flight existence check prevents silent fallback to $HOME
        let canonical_workspace = normalize_workspace_path(workspace)?;

        if !self.is_available() {
            return Err(RouterError::DriverUnavailable {
                driver: "tmux",
                reason: "tmux binary not found in PATH".to_string(),
            });
        }

        let mut cmd = self.command();
        cmd.args([
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{pane_id}\t#{pane_current_path}\t#{pane_current_command}",
            "-c",
        ]);
        cmd.arg(&canonical_workspace);

        if let Some(user_cmd) = command {
            cmd.arg(user_cmd);
        }

        let output = cmd
            .output()
            .map_err(|err| RouterError::DriverExecutionFailed(err.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RouterError::DriverExecutionFailed(
                stderr.trim().to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next().unwrap_or("").trim();
        if let Some(session) = parse_pane_line(first_line) {
            Ok(session)
        } else {
            let parts: Vec<&str> = first_line.split('\t').collect();
            if !parts.is_empty() && parts[0].starts_with('%') {
                let pid = parts[0].trim();
                let cmd = if parts.len() >= 3 && !parts[2].trim().is_empty() {
                    Some(parts[2].trim().to_string())
                } else {
                    command.map(|c| c.to_string())
                };
                let status = SessionStatus::from_command(cmd.as_deref());
                Ok(LiveSession::new(pid, canonical_workspace, status, cmd))
            } else {
                Err(RouterError::DriverExecutionFailed(format!(
                    "Failed to parse newly spawned pane details from tmux output: '{}'",
                    first_line
                )))
            }
        }
    }
}

/// Parses multi-line tmux format output into a list of `LiveSession` instances.
pub fn parse_tmux_output(output: &str) -> Vec<LiveSession> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_pane_line)
        .collect()
}

/// Parses a single line of tmux format output into a `LiveSession`.
/// Supports Pipe (`|`), Tab (`\t`), and Colon (`:`) delimiters.
pub fn parse_pane_line(line: &str) -> Option<LiveSession> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() {
        return None;
    }

    let (pane_id, raw_cwd, cmd_str) = if line.contains('|') {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 2 {
            return None;
        }
        let pid = parts[0].trim();
        let cwd = parts[1].trim();
        if pid.is_empty()
            || cwd.is_empty()
            || !pid.starts_with('%')
            || pid.contains(char::is_whitespace)
        {
            return None;
        }
        let cmd = if parts.len() >= 3 {
            Some(parts[2].trim())
        } else {
            None
        };
        (pid, cwd, cmd)
    } else if line.contains('\t') {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            return None;
        }
        let pid = parts[0].trim();
        let cwd = parts[1].trim();
        if pid.is_empty()
            || cwd.is_empty()
            || !pid.starts_with('%')
            || pid.contains(char::is_whitespace)
        {
            return None;
        }
        let cmd = if parts.len() >= 3 {
            Some(parts[2].trim())
        } else {
            None
        };
        (pid, cwd, cmd)
    } else if line.contains(':') {
        let trimmed = line.trim();
        let first_colon = trimmed.find(':')?;
        let last_colon = trimmed.rfind(':')?;
        let (pid, cwd, cmd) = if first_colon == last_colon {
            let pid = trimmed[..first_colon].trim();
            let cwd = trimmed[first_colon + 1..].trim();
            (pid, cwd, None)
        } else {
            let pid = trimmed[..first_colon].trim();
            let cwd = trimmed[first_colon + 1..last_colon].trim();
            let cmd = Some(trimmed[last_colon + 1..].trim());
            (pid, cwd, cmd)
        };
        if pid.is_empty()
            || cwd.is_empty()
            || !pid.starts_with('%')
            || pid.contains(char::is_whitespace)
        {
            return None;
        }
        (pid, cwd, cmd)
    } else {
        return None;
    };

    let cwd_buf = PathBuf::from(raw_cwd);
    let cwd = std::fs::canonicalize(&cwd_buf).unwrap_or(cwd_buf);
    let current_command = cmd_str.filter(|c| !c.is_empty()).map(|c| c.to_string());
    let status = SessionStatus::from_command(current_command.as_deref());

    Some(LiveSession {
        pane_id: pane_id.to_string(),
        cwd,
        status,
        current_command,
    })
}

#[cfg(test)]
mod tests {
    use super::super::classify_command_status;
    use super::*;

    #[test]
    fn test_parse_tmux_output_tsv() {
        let raw = "%0\t/tmp\tzsh\n%1\t/tmp\tcargo\n";
        let sessions = parse_tmux_output(raw);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].pane_id, "%0");
        assert_eq!(sessions[0].status, SessionStatus::Idle);
        assert_eq!(sessions[0].current_command, Some("zsh".into()));
        assert_eq!(sessions[1].pane_id, "%1");
        assert_eq!(sessions[1].status, SessionStatus::Working);
        assert_eq!(sessions[1].current_command, Some("cargo".into()));
    }

    #[test]
    fn test_parse_tmux_output_colon() {
        let raw = "%10:/tmp:bash\n";
        let session = parse_pane_line(raw).unwrap();
        assert_eq!(session.pane_id, "%10");
        assert_eq!(session.status, SessionStatus::Idle);
        assert_eq!(session.current_command, Some("bash".into()));
    }

    #[test]
    fn test_parse_tmux_output_colon_with_path_colons() {
        let raw = "%2:/tmp/repo:v1:python\n";
        let session = parse_pane_line(raw).unwrap();
        assert_eq!(session.pane_id, "%2");
        assert_eq!(session.cwd, PathBuf::from("/tmp/repo:v1"));
        assert_eq!(session.status, SessionStatus::Working);
        assert_eq!(session.current_command, Some("python".into()));
    }

    #[test]
    fn test_parse_tmux_output_single_colon() {
        let raw = "%3:/tmp/my_dir\n";
        let session = parse_pane_line(raw).unwrap();
        assert_eq!(session.pane_id, "%3");
        assert_eq!(session.cwd, PathBuf::from("/tmp/my_dir"));
        assert_eq!(session.status, SessionStatus::Working);
        assert_eq!(session.current_command, None);
    }

    #[test]
    fn test_command_status_matrix() {
        for shell in &[
            "bash", "zsh", "sh", "fish", "ksh", "csh", "tcsh", "dash", "nu",
        ] {
            assert_eq!(classify_command_status(shell), SessionStatus::Idle);
            let login = format!("-{}", shell);
            assert_eq!(classify_command_status(&login), SessionStatus::Idle);
            let path = format!("/usr/bin/{}", shell);
            assert_eq!(classify_command_status(&path), SessionStatus::Idle);
        }

        for tool in &[
            "cargo", "python", "node", "vim", "nvim", "git", "sleep", "agy",
        ] {
            assert_eq!(classify_command_status(tool), SessionStatus::Working);
        }

        assert_eq!(classify_command_status(""), SessionStatus::Working);
        assert_eq!(SessionStatus::from_command(None), SessionStatus::Working);
    }

    #[test]
    fn test_tmux_driver_offline_socket() {
        let driver = TmuxDriver::with_socket("k0maru_nonexistent_offline_socket_12345");
        if driver.is_available() {
            let sessions = driver.list_sessions().unwrap();
            assert!(sessions.is_empty());
        }
    }

    #[test]
    fn test_tmux_driver_spawn_pane_missing_dir_fails() {
        let driver = TmuxDriver::new();
        let missing = PathBuf::from("/nonexistent/workspace/123456789");
        let err = driver.spawn_pane(&missing, None).unwrap_err();
        assert_eq!(err, RouterError::WorkspaceNotFound(missing));
    }
}
