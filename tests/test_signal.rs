//! Tests for Task Signal Protocol, Storage Bus, and CLI subcommand.

use std::path::PathBuf;
use std::str::FromStr;
use tempfile::tempdir;

use assert_cmd::Command;
use k0maru_router::signal::{
    resolve_agent_pane, resolve_signals_dir, SignalStatus, SignalStore, TaskSignal,
    DEFAULT_SIGNALS_DIR, DEFAULT_SIGNAL_SCHEMA, ENV_K0MARU_SIGNAL_DIR,
};

#[test]
fn test_signal_status_variants_and_parsing() {
    assert_eq!(
        SignalStatus::from_str("completed").unwrap(),
        SignalStatus::Completed
    );
    assert_eq!(
        SignalStatus::from_str("COMPLETED").unwrap(),
        SignalStatus::Completed
    );
    assert_eq!(
        SignalStatus::from_str("success").unwrap(),
        SignalStatus::Completed
    );
    assert_eq!(
        SignalStatus::from_str("done").unwrap(),
        SignalStatus::Completed
    );

    assert_eq!(
        SignalStatus::from_str("failed").unwrap(),
        SignalStatus::Failed
    );
    assert_eq!(
        SignalStatus::from_str("FAILED").unwrap(),
        SignalStatus::Failed
    );
    assert_eq!(
        SignalStatus::from_str("error").unwrap(),
        SignalStatus::Failed
    );

    assert_eq!(
        SignalStatus::from_str("in_progress").unwrap(),
        SignalStatus::InProgress
    );
    assert_eq!(
        SignalStatus::from_str("inprogress").unwrap(),
        SignalStatus::InProgress
    );
    assert_eq!(
        SignalStatus::from_str("running").unwrap(),
        SignalStatus::InProgress
    );

    assert!(SignalStatus::from_str("unknown").is_err());
    assert!(SignalStatus::from_str("").is_err());

    // Display & as_str
    assert_eq!(SignalStatus::Completed.as_str(), "completed");
    assert_eq!(SignalStatus::Failed.as_str(), "failed");
    assert_eq!(SignalStatus::InProgress.as_str(), "in_progress");
    assert_eq!(format!("{}", SignalStatus::Completed), "completed");
}

#[test]
fn test_resolve_signals_dir_precedence() {
    let tmp = tempdir().unwrap();
    let override_dir = tmp.path().join("custom_override");
    let env_dir = tmp.path().join("env_override");

    // Priority 1: Explicit override wins regardless of env var
    std::env::set_var(ENV_K0MARU_SIGNAL_DIR, &env_dir);
    let resolved_p1 = resolve_signals_dir(Some(&override_dir));
    assert_eq!(resolved_p1, override_dir);

    // Priority 2: Environment variable wins when override is None
    let resolved_p2 = resolve_signals_dir(None);
    assert_eq!(resolved_p2, env_dir);
    assert_eq!(SignalStore::new(&env_dir).base_dir(), env_dir);

    // Clean up env var
    std::env::remove_var(ENV_K0MARU_SIGNAL_DIR);

    // Priority 3: Default fallback
    let resolved_p3 = resolve_signals_dir(None);
    if let Some(home) = dirs::home_dir() {
        assert_eq!(
            resolved_p3,
            home.join(k0maru_router::receipt::DEFAULT_K0MARU_DIR)
                .join(DEFAULT_SIGNALS_DIR)
        );
    } else {
        assert_eq!(
            resolved_p3,
            PathBuf::from(k0maru_router::receipt::DEFAULT_K0MARU_DIR).join(DEFAULT_SIGNALS_DIR)
        );
    }

    let default_store = SignalStore::default();
    assert_eq!(default_store.base_dir(), resolved_p3);
    assert_eq!(SignalStore::default_dir(), resolved_p3);
}

#[test]
fn test_resolve_agent_pane_precedence() {
    // Priority 1: Explicit override wins
    std::env::set_var("HERDR_PANE", "herdr:p0");
    std::env::set_var("TMUX_PANE", "%9");
    assert_eq!(
        resolve_agent_pane(Some("explicit:p1")),
        Some("explicit:p1".to_string())
    );

    // Priority 2: HERDR_PANE wins over TMUX_PANE
    assert_eq!(resolve_agent_pane(None), Some("herdr:p0".to_string()));

    // Priority 3: TMUX_PANE when HERDR_PANE unset
    std::env::remove_var("HERDR_PANE");
    assert_eq!(resolve_agent_pane(None), Some("%9".to_string()));

    // Priority 4: None when both unset
    std::env::remove_var("TMUX_PANE");
    assert_eq!(resolve_agent_pane(None), None);
    assert_eq!(resolve_agent_pane(Some("   ")), None);
}

#[test]
fn test_task_signal_validation() {
    let mut signal = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_01".to_string(),
        status: SignalStatus::Completed,
        summary: Some("done".to_string()),
        agent_pane: Some("master:p0".to_string()),
        workspace: PathBuf::from("/tmp"),
        timestamp: "2026-10-12T18:00:00Z".to_string(),
    };

    assert!(signal.validate().is_ok());

    // Empty task_id
    signal.task_id = "   ".to_string();
    assert!(signal.validate().is_err());

    // Path traversal
    signal.task_id = "../bad".to_string();
    assert!(signal.validate().is_err());
    signal.task_id = "sub/bad".to_string();
    assert!(signal.validate().is_err());

    // Invalid timestamp
    signal.task_id = "task_01".to_string();
    signal.timestamp = "not-a-timestamp".to_string();
    assert!(signal.validate().is_err());
}

#[test]
fn test_signal_store_emit_get_list_delete() {
    let tmp = tempdir().unwrap();
    let store = SignalStore::new(tmp.path());

    let signal_a = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_a".to_string(),
        status: SignalStatus::Completed,
        summary: Some("task a completed".to_string()),
        agent_pane: Some("p1".to_string()),
        workspace: tmp.path().to_path_buf(),
        timestamp: "2026-10-12T10:00:00Z".to_string(),
    };

    let signal_b = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_b".to_string(),
        status: SignalStatus::Failed,
        summary: Some("task b failed".to_string()),
        agent_pane: Some("p2".to_string()),
        workspace: tmp.path().to_path_buf(),
        timestamp: "2026-10-12T12:00:00Z".to_string(),
    };

    let emitted_a = store.emit(&signal_a).unwrap();
    assert!(emitted_a.is_file());

    let emitted_b = store.emit(&signal_b).unwrap();
    assert!(emitted_b.is_file());

    // Get
    let fetched_a = store.get("task_a").unwrap().expect("must find task_a");
    assert_eq!(fetched_a.task_id, "task_a");
    assert_eq!(fetched_a.status, SignalStatus::Completed);
    assert_eq!(fetched_a.summary.as_deref(), Some("task a completed"));

    // List: sorted timestamp descending (b then a)
    let list = store.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].task_id, "task_b");
    assert_eq!(list[1].task_id, "task_a");

    // Delete
    assert!(store.delete("task_a").unwrap());
    assert!(!store.delete("task_a").unwrap());
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn test_signal_store_latest_for_workspace() {
    let tmp = tempdir().unwrap();
    let ws_a = tmp.path().join("ws_a");
    let ws_b = tmp.path().join("ws_b");
    let ws_c = tmp.path().join("ws_c");
    std::fs::create_dir_all(&ws_a).unwrap();
    std::fs::create_dir_all(&ws_b).unwrap();
    std::fs::create_dir_all(&ws_c).unwrap();

    let store = SignalStore::new(tmp.path().join("signals"));

    let sig_a1 = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_a1".to_string(),
        status: SignalStatus::InProgress,
        summary: None,
        agent_pane: None,
        workspace: ws_a.clone(),
        timestamp: "2026-10-12T08:00:00Z".to_string(),
    };

    let sig_a2 = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_a2".to_string(),
        status: SignalStatus::Completed,
        summary: Some("latest a".to_string()),
        agent_pane: Some("p0".to_string()),
        workspace: ws_a.clone(),
        timestamp: "2026-10-12T09:00:00Z".to_string(),
    };

    let sig_b1 = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: "task_b1".to_string(),
        status: SignalStatus::Failed,
        summary: None,
        agent_pane: None,
        workspace: ws_b.clone(),
        timestamp: "2026-10-12T08:30:00Z".to_string(),
    };

    store.emit(&sig_a1).unwrap();
    store.emit(&sig_a2).unwrap();
    store.emit(&sig_b1).unwrap();

    let latest_a = store.latest_for_workspace(&ws_a).unwrap().unwrap();
    assert_eq!(latest_a.task_id, "task_a2");
    assert_eq!(latest_a.status, SignalStatus::Completed);

    let latest_b = store.latest_for_workspace(&ws_b).unwrap().unwrap();
    assert_eq!(latest_b.task_id, "task_b1");
    assert_eq!(latest_b.status, SignalStatus::Failed);

    let latest_c = store.latest_for_workspace(&ws_c).unwrap();
    assert!(latest_c.is_none());
}

#[test]
fn test_cli_signal_emit_and_json() {
    let tmp = tempdir().unwrap();
    let signal_dir = tmp.path().join("signals");
    let ws_dir = tmp.path().join("my_workspace");
    std::fs::create_dir_all(&ws_dir).unwrap();

    // 1. Emit signal via CLI with JSON output
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    let assert = cmd
        .args([
            "signal",
            "--task-id",
            "task_cli_test_01",
            "--status",
            "completed",
            "--summary",
            "refactoring done",
            "--agent-pane",
            "worker:p3",
            "--workspace",
            ws_dir.to_str().unwrap(),
            "--signal-dir",
            signal_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success();

    let output = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).expect("must be valid JSON");

    assert_eq!(json["task_id"], "task_cli_test_01");
    assert_eq!(json["status"], "completed");
    assert_eq!(json["summary"], "refactoring done");
    assert_eq!(json["agent_pane"], "worker:p3");

    // Verify file written to disk
    let signal_file = signal_dir.join("task_cli_test_01.json");
    assert!(signal_file.is_file());

    // 2. Emit signal via CLI with human text output
    let mut cmd2 = Command::cargo_bin("k0maru-router").unwrap();
    cmd2.args([
        "signal",
        "--task-id",
        "task_cli_test_02",
        "--status",
        "failed",
        "--signal-dir",
        signal_dir.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains(
        "SIGNAL EMITTED for 'task_cli_test_02'",
    ))
    .stdout(predicates::str::contains("Status:     failed"));
}
