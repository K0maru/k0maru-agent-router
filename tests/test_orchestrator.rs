//! Tests for Master-Worker Orchestrator Inspection Bus, Git probing, and ASCII table.

use std::fs;
use std::process::Command as StdCommand;
use tempfile::tempdir;

use assert_cmd::Command;
use k0maru_router::orchestrator::{
    collect_orchestrator_status, discover_default_workspaces, format_ascii_table, probe_git_status,
};
use k0maru_router::receipt::{
    CompilationCheck, ExecutionChecks, ExecutionReceipt, LinterCheck, ReceiptArtifacts,
    ReceiptStore, ReceiptVerdict, ScopeGuardCheck, TestCheck, DEFAULT_RECEIPT_SCHEMA,
};
use k0maru_router::signal::{SignalStatus, SignalStore, TaskSignal, DEFAULT_SIGNAL_SCHEMA};

#[test]
fn test_probe_git_status_current_workspace() {
    let cwd = std::env::current_dir().unwrap();
    let status = probe_git_status(&cwd).expect("current repo must be a valid git repository");

    assert!(!status.branch.is_empty(), "branch name must not be empty");
    assert!(!status.commit.is_empty(), "commit hash must not be empty");
    assert_ne!(status.commit, "-", "commit hash should be resolved");
}

#[test]
fn test_probe_git_status_non_git_workspace() {
    let tmp = tempdir().unwrap();
    let status = probe_git_status(tmp.path());
    assert!(status.is_none(), "non-git workspace should return None");
}

#[test]
fn test_collect_orchestrator_status_and_ascii_table() {
    let tmp = tempdir().unwrap();
    let ws_git = tmp.path().join("my-agent-worker");
    let ws_plain = tmp.path().join("my-plain-docs");
    fs::create_dir_all(&ws_git).unwrap();
    fs::create_dir_all(&ws_plain).unwrap();

    // Init git repo in ws_git
    StdCommand::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&ws_git)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.name", "Test Agent"])
        .current_dir(&ws_git)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.email", "agent@k0maru.dev"])
        .current_dir(&ws_git)
        .output()
        .unwrap();
    fs::write(ws_git.join("README.md"), "# Test Worker\n").unwrap();
    StdCommand::new("git")
        .args(["add", "README.md"])
        .current_dir(&ws_git)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&ws_git)
        .output()
        .unwrap();

    // Create stores
    let signal_store = SignalStore::new(tmp.path().join("signals"));
    let receipt_store = ReceiptStore::new(tmp.path().join("receipts"));

    // Emit signal for ws_git
    let signal = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_orchestrator_01".to_string(),
        status: SignalStatus::Completed,
        summary: Some("initial build completed".to_string()),
        agent_pane: Some("worker:p1".to_string()),
        workspace: ws_git.clone(),
        timestamp: "2026-10-12T18:00:00Z".to_string(),
    };
    signal_store.emit(&signal).unwrap();

    // Save receipt for ws_git
    let receipt = ExecutionReceipt {
        schema: DEFAULT_RECEIPT_SCHEMA.to_string(),
        task_id: "task_orchestrator_01".to_string(),
        workspace: ws_git.clone(),
        verdict: ReceiptVerdict::Success,
        checks: ExecutionChecks {
            compilation: CompilationCheck {
                status: "passed".to_string(),
                exit_code: 0,
            },
            tests: TestCheck {
                status: "passed".to_string(),
                total: 10,
                failed: 0,
                duration_ms: Some(120),
            },
            linter: LinterCheck {
                status: "passed".to_string(),
                warnings: 0,
            },
            scope_guard: ScopeGuardCheck {
                status: "passed".to_string(),
                modified_files: vec!["README.md".to_string()],
            },
        },
        artifacts: ReceiptArtifacts {
            git_patch_sha256: Some(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            ),
            receipt_timestamp: Some("2026-10-12T18:05:00Z".to_string()),
        },
    };
    receipt_store.save(&receipt).unwrap();

    // Collect orchestrator report
    let workspaces = vec![ws_git.clone(), ws_plain.clone()];
    let report = collect_orchestrator_status(&workspaces, &signal_store, &receipt_store);

    assert_eq!(report.probed_workspaces, 2);
    assert_eq!(report.workspaces.len(), 2);

    let status_git = report
        .workspaces
        .iter()
        .find(|w| w.name == "my-agent-worker")
        .expect("must find my-agent-worker");
    assert!(status_git.git.is_some());
    assert_eq!(status_git.git.as_ref().unwrap().branch, "main");
    assert!(status_git.latest_signal.is_some());
    assert_eq!(
        status_git.latest_signal.as_ref().unwrap().task_id,
        "task_orchestrator_01"
    );
    assert!(status_git.latest_receipt.is_some());
    assert_eq!(
        status_git.latest_receipt.as_ref().unwrap().task_id,
        "task_orchestrator_01"
    );
    assert!(status_git.latest_receipt.as_ref().unwrap().is_passing);

    let status_plain = report
        .workspaces
        .iter()
        .find(|w| w.name == "my-plain-docs")
        .expect("must find my-plain-docs");
    assert!(status_plain.git.is_none());
    assert!(status_plain.latest_signal.is_none());
    assert!(status_plain.latest_receipt.is_none());

    // Format ASCII table
    let table = format_ascii_table(&report);
    assert!(table.contains("K0MARU MASTER-WORKER ORCHESTRATOR STATUS (2 Workspaces Probed)"));
    assert!(table.contains("WORKSPACE"));
    assert!(table.contains("GIT STATUS"));
    assert!(table.contains("LATEST SIGNAL"));
    assert!(table.contains("LATEST RECEIPT"));
    assert!(table.contains("my-agent-worker"));
    assert!(table.contains("my-plain-docs"));
    assert!(table.contains("(not a git repo)"));
    assert!(table.contains("completed (task_orchestrator_01)"));
    assert!(table.contains("SUCCESS:PASS (task_orchestrator_01)"));
}

#[test]
fn test_discover_default_workspaces() {
    let workspaces = discover_default_workspaces(None);
    assert!(
        !workspaces.is_empty(),
        "default workspace discovery must return at least current dir"
    );
    let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    assert!(
        workspaces.contains(&cwd),
        "discovered workspaces must contain current directory: {cwd:?}"
    );
}

#[test]
fn test_cli_orchestrator_status_and_json() {
    let tmp = tempdir().unwrap();
    let ws = tmp.path().join("cli-test-ws");
    fs::create_dir_all(&ws).unwrap();

    let signal_dir = tmp.path().join("signals");
    let receipt_dir = tmp.path().join("receipts");

    // 1. JSON output via 'orchestrator status --json'
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    let assert = cmd
        .args([
            "orchestrator",
            "status",
            "--workspaces",
            ws.to_str().unwrap(),
            "--signal-dir",
            signal_dir.to_str().unwrap(),
            "--receipt-dir",
            receipt_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    let output = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).expect("must parse JSON");
    assert_eq!(json["probed_workspaces"], 1);
    assert_eq!(json["workspaces"][0]["name"], "cli-test-ws");

    // 2. ASCII table output via 'orchestrator status'
    let mut cmd2 = Command::cargo_bin("k0maru-router").unwrap();
    cmd2.args([
        "orchestrator",
        "status",
        "--workspaces",
        ws.to_str().unwrap(),
        "--signal-dir",
        signal_dir.to_str().unwrap(),
        "--receipt-dir",
        receipt_dir.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "K0MARU MASTER-WORKER ORCHESTRATOR STATUS (1 Workspaces Probed)",
    ))
    .stdout(predicates::str::contains("cli-test-ws"));

    // 3. Shorthand 'orchestrator --json' (default action)
    let mut cmd3 = Command::cargo_bin("k0maru-router").unwrap();
    cmd3.args([
        "orchestrator",
        "--workspaces",
        ws.to_str().unwrap(),
        "--signal-dir",
        signal_dir.to_str().unwrap(),
        "--receipt-dir",
        receipt_dir.to_str().unwrap(),
        "--json",
    ])
    .assert()
    .success();
}
