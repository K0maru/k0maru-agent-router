//! Task Signal Protocol and Storage Bus.
//!
//! Provides the Master-Worker ingest bus signaling mechanism where worker agents
//! emit completion, failure, or in-progress signals upon task execution.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::addressing::is_same_workspace;
use crate::error::RouterError;
use crate::receipt::DEFAULT_K0MARU_DIR;

/// Authoritative JSON Schema URI for task execution signals.
pub const DEFAULT_SIGNAL_SCHEMA: &str = "https://k0maru.dev/schemas/agent-signal-v1.json";

/// Environment variable controlling global signal bus storage location.
pub const ENV_K0MARU_SIGNAL_DIR: &str = "K0MARU_SIGNAL_DIR";

/// Default subfolder name for signal storage.
pub const DEFAULT_SIGNALS_DIR: &str = "signals";

/// Resolves the canonical signals storage directory based on standard precedence:
/// 1. Priority 1: Explicit override path or CLI `--signal-dir <PATH>` if supplied
/// 2. Priority 2: Environment variable `K0MARU_SIGNAL_DIR`
/// 3. Priority 3: Default home directory `dirs::home_dir().join(".k0maru").join("signals")`
///    (fallback to current dir `.k0maru/signals` if home dir cannot be resolved)
pub fn resolve_signals_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }

    if let Ok(env_val) = std::env::var(ENV_K0MARU_SIGNAL_DIR) {
        let trimmed = env_val.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if let Some(home) = dirs::home_dir() {
        home.join(DEFAULT_K0MARU_DIR).join(DEFAULT_SIGNALS_DIR)
    } else {
        PathBuf::from(DEFAULT_K0MARU_DIR).join(DEFAULT_SIGNALS_DIR)
    }
}

/// Resolves the reporting agent's pane or session identifier based on precedence:
/// 1. Explicit override (CLI arg `--agent-pane <PANE>`)
/// 2. Environment variable `HERDR_PANE`
/// 3. Environment variable `TMUX_PANE`
/// 4. None
pub fn resolve_agent_pane(override_pane: Option<&str>) -> Option<String> {
    if let Some(pane) = override_pane {
        let trimmed = pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Ok(herdr_pane) = std::env::var("HERDR_PANE") {
        let trimmed = herdr_pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Ok(tmux_pane) = std::env::var("TMUX_PANE") {
        let trimmed = tmux_pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    None
}

/// Status of the task reported in the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SignalStatus {
    /// Task has completed successfully.
    Completed,
    /// Task execution failed.
    Failed,
    /// Task is actively in progress.
    InProgress,
}

impl SignalStatus {
    /// Returns the canonical string representation of the signal status.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::InProgress => "in_progress",
        }
    }
}

impl fmt::Display for SignalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SignalStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "completed" | "success" | "done" => Ok(Self::Completed),
            "failed" | "error" => Ok(Self::Failed),
            "in_progress" | "inprogress" | "running" => Ok(Self::InProgress),
            other => Err(format!(
                "Invalid signal status: '{other}'. Expected 'completed', 'failed', or 'in_progress'"
            )),
        }
    }
}

impl Serialize for SignalStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SignalStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<SignalStatus>().map_err(serde::de::Error::custom)
    }
}

fn default_signal_schema() -> String {
    DEFAULT_SIGNAL_SCHEMA.to_string()
}

/// Structured task signal emitted by a worker agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSignal {
    /// Authoritative schema URI reference.
    #[serde(rename = "$schema", default = "default_signal_schema")]
    pub schema: String,
    /// Identifier of the task (e.g. `task_router_orchestrator_bus_01`).
    pub task_id: String,
    /// Reported execution status.
    pub status: SignalStatus,
    /// Optional high-level summary or rationale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Reporting agent pane identifier (e.g. `master:p0` or `%1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pane: Option<String>,
    /// Working directory or workspace where the task was performed.
    pub workspace: PathBuf,
    /// Signal generation timestamp in RFC-3339 format.
    pub timestamp: String,
}

impl TaskSignal {
    /// Validates the internal consistency and invariants of the signal.
    pub fn validate(&self) -> Result<(), RouterError> {
        let task_id = self.task_id.trim();
        if task_id.is_empty() {
            return Err(RouterError::InvalidSignal(
                "Signal 'task_id' cannot be empty".to_string(),
            ));
        }

        if task_id.contains("..")
            || task_id.contains('/')
            || task_id.contains('\\')
            || task_id.contains('\0')
        {
            return Err(RouterError::InvalidSignal(format!(
                "Signal 'task_id' contains invalid path traversal tokens: '{task_id}'"
            )));
        }

        // Validate timestamp parseability
        chrono::DateTime::parse_from_rfc3339(&self.timestamp)
            .map_err(RouterError::DateTimeParse)?;

        Ok(())
    }
}

/// Persistent storage repository for task signals.
#[derive(Debug, Clone)]
pub struct SignalStore {
    base_dir: PathBuf,
}

impl Default for SignalStore {
    fn default() -> Self {
        Self {
            base_dir: resolve_signals_dir(None),
        }
    }
}

impl SignalStore {
    /// Creates a new `SignalStore` rooted at `base_dir`.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Default directory path for signals storage resolved via `resolve_signals_dir(None)`.
    pub fn default_dir() -> PathBuf {
        resolve_signals_dir(None)
    }

    /// Returns the underlying base directory.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolves the filesystem path for a given task ID.
    fn resolve_path(&self, task_id: &str) -> Result<PathBuf, RouterError> {
        let trimmed = task_id.trim();
        if trimmed.is_empty() {
            return Err(RouterError::InvalidSignal(
                "Task ID cannot be empty".to_string(),
            ));
        }

        if trimmed.contains("..")
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains('\0')
        {
            return Err(RouterError::InvalidSignal(format!(
                "Task ID contains invalid path traversal tokens: '{task_id}'"
            )));
        }

        let clean_id = trimmed.strip_suffix(".json").unwrap_or(trimmed);
        Ok(self.base_dir.join(format!("{clean_id}.json")))
    }

    /// Validates and emits a task signal under `<base_dir>/<task_id>.json`.
    pub fn emit(&self, signal: &TaskSignal) -> Result<PathBuf, RouterError> {
        signal.validate()?;
        fs::create_dir_all(&self.base_dir)?;

        let target_path = self.resolve_path(&signal.task_id)?;
        let json_data = serde_json::to_string_pretty(signal)?;
        fs::write(&target_path, json_data)?;

        Ok(target_path)
    }

    /// Retrieves a stored task signal by task ID.
    pub fn get(&self, task_id: &str) -> Result<Option<TaskSignal>, RouterError> {
        let target_path = self.resolve_path(task_id)?;
        if !target_path.is_file() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&target_path)?;
        let signal: TaskSignal = serde_json::from_str(&contents)?;
        signal.validate()?;
        Ok(Some(signal))
    }

    /// Lists all stored task signals, sorted by timestamp descending.
    pub fn list(&self) -> Result<Vec<TaskSignal>, RouterError> {
        if !self.base_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut signals = Vec::new();
        let entries = fs::read_dir(&self.base_dir)?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(signal) = serde_json::from_str::<TaskSignal>(&contents) {
                        if signal.validate().is_ok() {
                            signals.push(signal);
                        }
                    }
                }
            }
        }

        // Sort deterministically: timestamp descending, tie-break task_id ascending
        signals.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| a.task_id.cmp(&b.task_id))
        });

        Ok(signals)
    }

    /// Returns the latest task signal matching the specified workspace directory.
    pub fn latest_for_workspace(
        &self,
        workspace: &Path,
    ) -> Result<Option<TaskSignal>, RouterError> {
        let signals = self.list()?;
        for signal in signals {
            if is_same_workspace(&signal.workspace, workspace) {
                return Ok(Some(signal));
            }
        }
        Ok(None)
    }

    /// Deletes a stored task signal if present.
    pub fn delete(&self, task_id: &str) -> Result<bool, RouterError> {
        let target_path = self.resolve_path(task_id)?;
        if target_path.is_file() {
            fs::remove_file(target_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
