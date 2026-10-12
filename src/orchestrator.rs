//! Master-Worker Orchestrator Inspection Bus.
//!
//! Provides ultra-fast (<50ms, zero-daemon) multi-workspace state inspection,
//! consolidating git branch/commit/dirty status, recent task signals, and execution receipts.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::driver::MultiplexerDriver;
use crate::receipt::{ExecutionReceipt, ReceiptStore, ReceiptVerdict};
use crate::signal::{SignalStatus, SignalStore, TaskSignal};

/// Git repository state for an inspected workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGitStatus {
    /// Active git branch name (or detached HEAD marker).
    pub branch: String,
    /// Short HEAD commit hash (e.g. `dcef5b0`).
    pub commit: String,
    /// Number of dirty files in working tree or index.
    pub dirty_files: usize,
    /// Whether the working tree is 100% clean.
    pub is_clean: bool,
}

/// Task signal summary for an inspected workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSignalStatus {
    /// Task identifier.
    pub task_id: String,
    /// Execution status.
    pub status: SignalStatus,
    /// Optional summary or description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Reporting agent pane identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pane: Option<String>,
    /// Timestamp in RFC-3339 format.
    pub timestamp: String,
}

impl From<TaskSignal> for WorkspaceSignalStatus {
    fn from(signal: TaskSignal) -> Self {
        Self {
            task_id: signal.task_id,
            status: signal.status,
            summary: signal.summary,
            agent_pane: signal.agent_pane,
            timestamp: signal.timestamp,
        }
    }
}

/// Execution receipt summary for an inspected workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceReceiptStatus {
    /// Task identifier.
    pub task_id: String,
    /// Execution verdict.
    pub verdict: ReceiptVerdict,
    /// Whether all checks and the verdict are passing.
    pub is_passing: bool,
    /// Summary of check results.
    pub checks_summary: String,
    /// Receipt generation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl From<ExecutionReceipt> for WorkspaceReceiptStatus {
    fn from(receipt: ExecutionReceipt) -> Self {
        let is_passing = receipt.is_passing();
        let checks_summary = format!(
            "compilation: {} (exit {}), tests: {} ({}/{} failed), linter: {} ({} warnings)",
            receipt.checks.compilation.status,
            receipt.checks.compilation.exit_code,
            receipt.checks.tests.status,
            receipt.checks.tests.failed,
            receipt.checks.tests.total,
            receipt.checks.linter.status,
            receipt.checks.linter.warnings,
        );
        Self {
            task_id: receipt.task_id,
            verdict: receipt.verdict,
            is_passing,
            checks_summary,
            timestamp: receipt.artifacts.receipt_timestamp,
        }
    }
}

/// Comprehensive inspection status for a single workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    /// Workspace directory name.
    pub name: String,
    /// Canonical filesystem path to workspace.
    pub path: PathBuf,
    /// Git repository state if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<WorkspaceGitStatus>,
    /// Latest reported signal if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_signal: Option<WorkspaceSignalStatus>,
    /// Latest verified execution receipt if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_receipt: Option<WorkspaceReceiptStatus>,
}

/// Aggregated multi-workspace status report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorReport {
    /// Total count of workspaces probed.
    pub probed_workspaces: usize,
    /// Individual workspace statuses.
    pub workspaces: Vec<WorkspaceStatus>,
}

/// Probes Git status (branch, commit, dirty files) for a target workspace directory.
/// Executes in ~5-15ms using non-interactive git commands.
pub fn probe_git_status(workspace: &Path) -> Option<WorkspaceGitStatus> {
    if !workspace.join(".git").exists() {
        return None;
    }

    // 1. Run git status --porcelain=v1 -b to extract branch and dirty file count
    let status_output = Command::new("git")
        .args(["status", "--porcelain=v1", "-b"])
        .current_dir(workspace)
        .output()
        .ok()?;

    if !status_output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&status_output.stdout);
    let mut lines = stdout.lines();

    // Parse branch from line 1: e.g. "## dev...origin/dev" or "## dev" or "## HEAD (no branch)"
    let branch_line = lines.next().unwrap_or("").trim();
    let raw_branch = branch_line.strip_prefix("##").unwrap_or(branch_line).trim();
    let branch = if let Some(idx) = raw_branch.find("...") {
        raw_branch[..idx].trim().to_string()
    } else if let Some(idx) = raw_branch.find(' ') {
        if raw_branch.starts_with("HEAD") {
            raw_branch.to_string()
        } else if raw_branch.starts_with("Initial commit on")
            || raw_branch.starts_with("No commits yet on")
        {
            raw_branch
                .split_whitespace()
                .last()
                .unwrap_or(raw_branch)
                .to_string()
        } else {
            raw_branch[..idx].trim().to_string()
        }
    } else {
        raw_branch.to_string()
    };

    // Remaining non-empty lines are modified / untracked / staged files
    let dirty_files = lines.filter(|l| !l.trim().is_empty()).count();
    let is_clean = dirty_files == 0;

    // 2. Run git rev-parse --short HEAD to extract commit hash
    let rev_output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok();

    let commit = match rev_output {
        Some(ref out) if out.status.success() => {
            let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if hash.is_empty() {
                "-".to_string()
            } else {
                hash
            }
        }
        _ => "-".to_string(),
    };

    Some(WorkspaceGitStatus {
        branch: if branch.is_empty() {
            "-".to_string()
        } else {
            branch
        },
        commit,
        dirty_files,
        is_clean,
    })
}

/// Probes a single workspace, resolving git state, latest signal, and latest receipt.
pub fn probe_workspace_status(
    workspace: &Path,
    signal_store: &SignalStore,
    receipt_store: &ReceiptStore,
) -> WorkspaceStatus {
    let canonical_path = fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let name = canonical_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| workspace.to_str().unwrap_or("workspace"))
        .to_string();

    let git = probe_git_status(&canonical_path);

    let latest_signal = signal_store
        .latest_for_workspace(&canonical_path)
        .ok()
        .flatten()
        .map(WorkspaceSignalStatus::from);

    let latest_receipt = receipt_store
        .latest_for_workspace(&canonical_path)
        .ok()
        .flatten()
        .map(WorkspaceReceiptStatus::from);

    WorkspaceStatus {
        name,
        path: canonical_path,
        git,
        latest_signal,
        latest_receipt,
    }
}

/// Discovers candidate workspaces according to default heuristics:
/// 1. Sibling directories under parent matching `k0maru-agent-*`
/// 2. User workspace directory `~/workspace/k0maru-agent-*`
/// 3. Active multiplexer sessions from `driver`
/// 4. Current working directory
pub fn discover_default_workspaces(driver: Option<&dyn MultiplexerDriver>) -> Vec<PathBuf> {
    let mut candidates = HashSet::new();

    // 1. Check parent directory of current working directory
    if let Ok(cwd) = std::env::current_dir() {
        candidates.insert(fs::canonicalize(&cwd).unwrap_or(cwd.clone()));
        if let Some(parent) = cwd.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("k0maru-agent-") {
                                candidates.insert(fs::canonicalize(&path).unwrap_or(path));
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Check user's standard ~/workspace directory
    if let Some(home) = dirs::home_dir() {
        let ws_root = home.join("workspace");
        if ws_root.is_dir() {
            if let Ok(entries) = fs::read_dir(&ws_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("k0maru-agent-") {
                                candidates.insert(fs::canonicalize(&path).unwrap_or(path));
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Check active multiplexer sessions
    if let Some(d) = driver {
        if let Ok(sessions) = d.list_sessions() {
            for session in sessions {
                if session.cwd.is_dir() {
                    candidates.insert(fs::canonicalize(&session.cwd).unwrap_or(session.cwd));
                }
            }
        }
    }

    let mut result: Vec<PathBuf> = candidates.into_iter().collect();
    result.sort_by(|a, b| {
        let name_a = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let name_b = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name_a.cmp(name_b).then_with(|| a.cmp(b))
    });
    result
}

/// Collects orchestrator status across multiple workspaces in parallel via threads (<50ms).
pub fn collect_orchestrator_status(
    workspaces: &[PathBuf],
    signal_store: &SignalStore,
    receipt_store: &ReceiptStore,
) -> OrchestratorReport {
    let statuses: Vec<WorkspaceStatus> = std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(workspaces.len());
        for ws in workspaces {
            handles.push(s.spawn(|| probe_workspace_status(ws, signal_store, receipt_store)));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    OrchestratorReport {
        probed_workspaces: statuses.len(),
        workspaces: statuses,
    }
}

/// Formats the orchestrator status report into a clean, compact ASCII table.
pub fn format_ascii_table(report: &OrchestratorReport) -> String {
    if report.workspaces.is_empty() {
        return "No workspaces probed. Use --workspaces <DIR> or check current directory."
            .to_string();
    }

    // Prepare cell texts
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for ws in &report.workspaces {
        let col_ws = ws.name.clone();

        let col_git = match &ws.git {
            Some(git) => {
                let dirty_str = if git.is_clean {
                    "clean".to_string()
                } else {
                    format!("{} dirty", git.dirty_files)
                };
                format!("{}@{} ({})", git.branch, git.commit, dirty_str)
            }
            None => "(not a git repo)".to_string(),
        };

        let col_signal = match &ws.latest_signal {
            Some(sig) => format!("{} ({})", sig.status, sig.task_id),
            None => "-".to_string(),
        };

        let col_receipt = match &ws.latest_receipt {
            Some(rcpt) => {
                let status_label = if rcpt.is_passing { "PASS" } else { "FAIL" };
                format!(
                    "{}:{} ({})",
                    rcpt.verdict.as_str().to_uppercase(),
                    status_label,
                    rcpt.task_id
                )
            }
            None => "-".to_string(),
        };

        rows.push((col_ws, col_git, col_signal, col_receipt));
    }

    // Calculate column widths with minimums
    let w_ws = rows.iter().map(|r| r.0.len()).max().unwrap_or(12).max(22);
    let w_git = rows.iter().map(|r| r.1.len()).max().unwrap_or(10).max(20);
    let w_sig = rows.iter().map(|r| r.2.len()).max().unwrap_or(13).max(24);
    let w_rcpt = rows.iter().map(|r| r.3.len()).max().unwrap_or(14).max(24);

    let total_width = w_ws + w_git + w_sig + w_rcpt + 6; // 3 separators of 2 spaces
    let border_bar = "=".repeat(total_width);
    let div_bar = "-".repeat(total_width);

    let mut out = String::new();
    out.push_str(&border_bar);
    out.push('\n');
    out.push_str(&format!(
        "K0MARU MASTER-WORKER ORCHESTRATOR STATUS ({} Workspaces Probed)\n",
        report.probed_workspaces
    ));
    out.push_str(&border_bar);
    out.push('\n');

    out.push_str(&format!(
        "{:<w_ws$}  {:<w_git$}  {:<w_sig$}  {:<w_rcpt$}\n",
        "WORKSPACE",
        "GIT STATUS",
        "LATEST SIGNAL",
        "LATEST RECEIPT",
        w_ws = w_ws,
        w_git = w_git,
        w_sig = w_sig,
        w_rcpt = w_rcpt
    ));
    out.push_str(&div_bar);
    out.push('\n');

    for (col_ws, col_git, col_signal, col_receipt) in rows {
        out.push_str(&format!(
            "{:<w_ws$}  {:<w_git$}  {:<w_sig$}  {:<w_rcpt$}\n",
            col_ws,
            col_git,
            col_signal,
            col_receipt,
            w_ws = w_ws,
            w_git = w_git,
            w_sig = w_sig,
            w_rcpt = w_rcpt
        ));
    }

    out.push_str(&border_bar);
    out
}
