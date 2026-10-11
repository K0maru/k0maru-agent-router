//! Terminal Multiplexer Driver SPI and Domain Types.
//!
//! Provides the pluggable `MultiplexerDriver` abstraction and session state models
//! for interacting with terminal multiplexers (such as `tmux` or `herdr`).

pub mod herdr;
pub mod tmux;

pub use herdr::HerdrDriver;
pub use tmux::{parse_pane_line, parse_tmux_output, TmuxDriver};

use crate::error::RouterError;
use std::path::{Path, PathBuf};

/// Activity status of a multiplexer session or pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    /// Pane is running an interactive shell waiting for input.
    Idle,
    /// Pane is executing a long-running foreground command or process.
    Working,
}

impl SessionStatus {
    /// Classifies an optional process command name into Idle vs Working.
    pub fn from_command(command: Option<&str>) -> Self {
        match command {
            Some(cmd) => classify_command_status(cmd),
            None => SessionStatus::Working,
        }
    }
}

/// Classifies a process command string as Idle (shell prompt) or Working (active tool/process).
///
/// Recognizes standard shells (`bash`, `zsh`, `sh`, `fish`, `ksh`, `csh`, `tcsh`, `dash`, `nu`),
/// including login shells with leading hyphens (`-zsh`, `-bash`) and absolute path prefixes
/// (`/bin/zsh`, `/usr/bin/bash`).
pub fn classify_command_status(cmd: &str) -> SessionStatus {
    let clean = cmd.trim();
    if clean.is_empty() {
        return SessionStatus::Working;
    }

    let basename = clean
        .rsplit('/')
        .next()
        .unwrap_or(clean)
        .trim_start_matches('-');
    let lower = basename.to_lowercase();
    match lower.as_str() {
        "bash" | "zsh" | "sh" | "fish" | "ksh" | "csh" | "tcsh" | "dash" | "nu" => {
            SessionStatus::Idle
        }
        _ => SessionStatus::Working,
    }
}

/// Represents an active terminal multiplexer session or pane.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiveSession {
    /// Unique pane identifier (e.g. "%0", "%1").
    pub pane_id: String,
    /// Canonical working directory of the pane.
    pub cwd: PathBuf,
    /// Activity status (Idle vs Working).
    pub status: SessionStatus,
    /// Foreground command running in the pane, if known.
    pub current_command: Option<String>,
}

impl LiveSession {
    /// Creates a new `LiveSession` instance.
    pub fn new(
        pane_id: impl Into<String>,
        cwd: impl Into<PathBuf>,
        status: SessionStatus,
        current_command: Option<String>,
    ) -> Self {
        Self {
            pane_id: pane_id.into(),
            cwd: cwd.into(),
            status,
            current_command,
        }
    }
}

/// Pluggable SPI trait for terminal multiplexer drivers (Tmux, Herdr, Subprocess).
pub trait MultiplexerDriver: Send + Sync {
    /// Static driver identifier name (e.g. "tmux", "herdr").
    fn name(&self) -> &'static str;

    /// Checks if the driver executable or socket is available on the host system.
    fn is_available(&self) -> bool;

    /// Lists all active sessions/panes. Returns `Ok(vec![])` if the multiplexer server
    /// is not running or has no active sessions.
    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError>;

    /// Dispatches a prompt string into the specified pane.
    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError>;

    /// Spawns a new pane in the specified workspace directory, optionally running a command.
    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError>;
}

/// Unified driver enum selecting between available multiplexer implementations.
#[derive(Debug, Clone)]
pub enum AnyDriver {
    Tmux(TmuxDriver),
    Herdr(HerdrDriver),
}

impl MultiplexerDriver for AnyDriver {
    fn name(&self) -> &'static str {
        match self {
            Self::Tmux(d) => d.name(),
            Self::Herdr(d) => d.name(),
        }
    }

    fn is_available(&self) -> bool {
        match self {
            Self::Tmux(d) => d.is_available(),
            Self::Herdr(d) => d.is_available(),
        }
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        match self {
            Self::Tmux(d) => d.list_sessions(),
            Self::Herdr(d) => d.list_sessions(),
        }
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        match self {
            Self::Tmux(d) => d.send_prompt(pane_id, prompt),
            Self::Herdr(d) => d.send_prompt(pane_id, prompt),
        }
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        match self {
            Self::Tmux(d) => d.spawn_pane(workspace, command),
            Self::Herdr(d) => d.spawn_pane(workspace, command),
        }
    }
}
