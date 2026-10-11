//! Adversarial empirical challenge test suite for Milestone M4 (Challenger 2).
//!
//! Focus:
//! 1. Quarantine invariant: untrusted external tasks are staged to outbox and NEVER trigger driver.
//! 2. Outbox subcommand lifecycle: stage -> list -> inspect -> approve -> reject -> state transitions & duplicate rejections.
//! 3. Top-level convenience aliases: `list-staged`, `inspect-staged`, `approve`, `reject`.
//! 4. Adversarial stress & boundary edge cases: path traversal defense, foreign file tolerance, corrupted drafts.

use assert_cmd::Command;
use k0maru_router::coordinator::{RouteOutcome, RouterCoordinator};
use k0maru_router::driver::{LiveSession, MultiplexerDriver, SessionStatus};
use k0maru_router::envelope::TaskEnvelope;
use k0maru_router::error::RouterError;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("k0maru-router binary must be buildable via cargo_bin")
}

fn sample_external_envelope_json(id: &str, workspace: Option<&str>) -> String {
    let mut val = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T19:00:00Z",
        "caller": {
            "workspace": "/Users/k0maru3/workspace/test-caller",
            "agent": "challenger-agent",
            "pane_id": "%10"
        },
        "target": {
            "identifier": "react-upstream",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Synthetic external bug report",
            "severity": "critical",
            "reproduction": {
                "snippet": "render(<App />)",
                "error": "Invariant Violation: 42"
            },
            "expected": "No crash"
        }
    });

    if let Some(ws) = workspace {
        val["target"]["workspace"] = serde_json::Value::String(ws.to_string());
    }

    serde_json::to_string_pretty(&val).unwrap()
}

fn sample_internal_envelope_json(id: &str, workspace: &str) -> String {
    let val = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T19:00:00Z",
        "caller": {
            "workspace": "/Users/k0maru3/workspace/test-caller",
            "agent": "challenger-agent",
            "pane_id": "%10"
        },
        "target": {
            "identifier": "internal-module",
            "workspace": workspace,
            "is_external": false
        },
        "intent": "feature_request",
        "payload": {
            "title": "Internal module enhancement",
            "severity": "normal",
            "expected": "Smooth dispatch"
        }
    });

    serde_json::to_string_pretty(&val).unwrap()
}

/// Adversarial Spy Driver that panics if any driver method is called when quarantine is expected.
#[derive(Debug, Default, Clone)]
struct SpyDriver {
    list_sessions_calls: Arc<AtomicUsize>,
    send_prompt_calls: Arc<AtomicUsize>,
    spawn_pane_calls: Arc<AtomicUsize>,
}

impl MultiplexerDriver for SpyDriver {
    fn name(&self) -> &'static str {
        "spy-driver"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        self.list_sessions_calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![LiveSession {
            pane_id: "%99".to_string(),
            cwd: PathBuf::from("/nonexistent/fake/cwd"),
            status: SessionStatus::Idle,
            current_command: None,
        }])
    }

    fn send_prompt(&self, _pane_id: &str, _prompt: &str) -> Result<(), RouterError> {
        self.send_prompt_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        _command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        self.spawn_pane_calls.fetch_add(1, Ordering::SeqCst);
        Ok(LiveSession {
            pane_id: "%100".to_string(),
            cwd: workspace.to_path_buf(),
            status: SessionStatus::Working,
            current_command: None,
        })
    }
}

// ============================================================================
// 1. Quarantine Invariant & Driver Non-Interference
// ============================================================================

#[test]
fn test_quarantine_invariant_driver_is_never_invoked() {
    let spy = SpyDriver::default();
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("staged");

    let coordinator = RouterCoordinator::new(spy.clone()).with_outbox_dir(&outbox_dir);

    // Test with untrusted external envelope
    let json = sample_external_envelope_json("task_quarantine_spy_1", None);
    let envelope = TaskEnvelope::from_json_str(&json).unwrap();

    let outcome = coordinator.route_envelope(&envelope, false).unwrap();

    assert!(outcome.is_staged());
    match outcome {
        RouteOutcome::Staged { issue_path, reason } => {
            assert!(issue_path.exists());
            assert!(reason.contains("react-upstream"));
            assert_eq!(
                issue_path,
                outbox_dir.join("issue_task_quarantine_spy_1.md")
            );
        }
        _ => panic!("Expected Staged outcome"),
    }

    // Crucial quarantine invariant: Spy driver methods MUST NOT have been invoked!
    assert_eq!(
        spy.list_sessions_calls.load(Ordering::SeqCst),
        0,
        "list_sessions must never be called for quarantined tasks"
    );
    assert_eq!(
        spy.send_prompt_calls.load(Ordering::SeqCst),
        0,
        "send_prompt must never be called for quarantined tasks"
    );
    assert_eq!(
        spy.spawn_pane_calls.load(Ordering::SeqCst),
        0,
        "spawn_pane must never be called for quarantined tasks"
    );
}

#[test]
fn test_quarantine_invariant_with_arbitrary_workspaces_never_checks_local_disk() {
    let spy = SpyDriver::default();
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("staged");

    let coordinator = RouterCoordinator::new(spy.clone()).with_outbox_dir(&outbox_dir);

    // External tasks may have arbitrary, completely non-existent or foreign workspace paths
    let json = sample_external_envelope_json(
        "task_quarantine_nonexistent_ws",
        Some("/completely/fake/and/nonexistent/workspace/12345"),
    );
    let envelope = TaskEnvelope::from_json_str(&json).unwrap();

    let outcome = coordinator.route_envelope(&envelope, false).unwrap();
    assert!(outcome.is_staged());

    assert_eq!(spy.list_sessions_calls.load(Ordering::SeqCst), 0);
    assert_eq!(spy.send_prompt_calls.load(Ordering::SeqCst), 0);
    assert_eq!(spy.spawn_pane_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn test_contrast_internal_task_invokes_driver_unlike_external_quarantine() {
    let spy = SpyDriver::default();
    let temp = tempdir().unwrap();
    let ws = temp.path().join("local_internal_ws");
    std::fs::create_dir_all(&ws).unwrap();

    let coordinator = RouterCoordinator::new(spy.clone());

    let json = sample_internal_envelope_json("task_internal_contrast", ws.to_str().unwrap());
    let envelope = TaskEnvelope::from_json_str(&json).unwrap();

    // Internal routing MUST invoke the driver to resolve and dispatch
    let outcome = coordinator.route_envelope(&envelope, false).unwrap();
    assert!(outcome.is_dispatched());

    // Proves the spy oracle detects invocations when they occur
    assert!(
        spy.list_sessions_calls.load(Ordering::SeqCst) > 0,
        "Internal dispatch must query live sessions"
    );
    assert!(
        spy.send_prompt_calls.load(Ordering::SeqCst) > 0
            || spy.spawn_pane_calls.load(Ordering::SeqCst) > 0,
        "Internal dispatch must interact with pane"
    );
}

#[test]
fn test_quarantine_cli_e2e_creates_structured_mre_draft() {
    let temp = tempdir().unwrap();
    let env_file = temp.path().join("envelope.json");
    let outbox_dir = temp.path().join("staged");

    let json = sample_external_envelope_json("task_quarantine_cli_mre", None);
    std::fs::write(&env_file, json).unwrap();

    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[QUARANTINED]"))
        .stdout(predicate::str::contains("task_quarantine_cli_mre"));

    let staged_file = outbox_dir.join("issue_task_quarantine_cli_mre.md");
    assert!(staged_file.is_file());

    let content = std::fs::read_to_string(&staged_file).unwrap();
    assert!(content.contains("# Staged Issue Draft: Synthetic external bug report"));
    assert!(content.contains("- **ID**: task_quarantine_cli_mre"));
    assert!(content.contains("- **Target**: react-upstream"));
    assert!(content.contains("- **Severity**: critical"));
    assert!(content.contains("- **Status**: Pending Human Authorization (HITL)"));
    assert!(content.contains("## Reproduction"));
    assert!(content.contains("Invariant Violation: 42"));
    assert!(content.contains("## Expected"));
    assert!(content.contains("No crash"));
    assert!(content.contains("*Status: Pending Human Authorization (HITL)*"));
}

// ============================================================================
// 2. Outbox Subcommand Lifecycle via CLI & State Transitions
// ============================================================================

#[test]
fn test_outbox_cli_full_lifecycle_and_duplicate_approve_rejection() {
    let temp = tempdir().unwrap();
    let env_file = temp.path().join("envelope.json");
    let outbox_dir = temp.path().join("outbox");
    let issue_id = "task_lifecycle_approve_cycle";

    let json = sample_external_envelope_json(issue_id, None);
    std::fs::write(&env_file, json).unwrap();

    // 1. Stage via route
    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0);

    // 2. List -> shows Pending
    router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(issue_id))
        .stdout(predicate::str::contains("Pending"));

    // 3. Inspect -> shows content
    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("Synthetic external bug report"));

    // 4. Approve -> Success
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(format!(
            "Approved staged issue '{issue_id}'"
        )))
        .stdout(predicate::str::contains("Status updated to: Approved."));

    // 5. Inspect after approve -> shows Approved in header & trailer
    let inspect_out = router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let inspect_str = String::from_utf8(inspect_out).unwrap();
    assert!(inspect_str.contains("- **Status**: Approved by Human"));
    assert!(inspect_str.contains("*Status: Approved by Human*"));

    // 6. List after approve -> shows Approved
    router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(issue_id))
        .stdout(predicate::str::contains("Approved"));

    // 7. Duplicate Approve -> MUST FAIL with exit code 1
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Issue '{issue_id}' is already approved"
        )));

    // 8. Rejecting an already approved issue -> MUST FAIL with exit code 1
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Cannot reject issue '{issue_id}': status is already Approved"
        )));
}

#[test]
fn test_outbox_cli_full_lifecycle_and_duplicate_reject_rejection() {
    let temp = tempdir().unwrap();
    let env_file = temp.path().join("envelope.json");
    let outbox_dir = temp.path().join("outbox");
    let issue_id = "task_lifecycle_reject_cycle";

    let json = sample_external_envelope_json(issue_id, None);
    std::fs::write(&env_file, json).unwrap();

    // 1. Stage via route
    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0);

    // 2. Reject -> Success
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(format!(
            "Rejected staged issue '{issue_id}'"
        )))
        .stdout(predicate::str::contains("Status updated to: Rejected."));

    // 3. Inspect after reject -> shows Rejected in header & trailer
    let inspect_out = router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let inspect_str = String::from_utf8(inspect_out).unwrap();
    assert!(inspect_str.contains("- **Status**: Rejected by Human"));
    assert!(inspect_str.contains("*Status: Rejected by Human*"));

    // 4. List after reject -> shows Rejected
    router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(issue_id))
        .stdout(predicate::str::contains("Rejected"));

    // 5. Duplicate Reject -> MUST FAIL with exit code 1
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Issue '{issue_id}' is already rejected"
        )));

    // 6. Approving an already rejected issue -> MUST FAIL with exit code 1
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            issue_id,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Cannot approve issue '{issue_id}': status is Rejected"
        )));
}

// ============================================================================
// 3. Top-Level Convenience Aliases Verification
// ============================================================================

#[test]
fn test_top_level_convenience_aliases_positional_and_json() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_file1 = temp.path().join("env1.json");
    let env_file2 = temp.path().join("env2.json");

    let id1 = "alias_approve_task";
    let id2 = "alias_reject_task";

    std::fs::write(&env_file1, sample_external_envelope_json(id1, None)).unwrap();
    std::fs::write(&env_file2, sample_external_envelope_json(id2, None)).unwrap();

    // Stage both
    router_cmd()
        .args([
            "route",
            "-f",
            env_file1.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    router_cmd()
        .args([
            "route",
            "-f",
            env_file2.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Alias: list-staged (text and json)
    router_cmd()
        .args(["list-staged", "--outbox-dir", outbox_dir.to_str().unwrap()])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(id1))
        .stdout(predicate::str::contains(id2));

    let list_json_out = router_cmd()
        .args([
            "list-staged",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let list_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(list_json_out).unwrap()).unwrap();
    assert_eq!(list_val.as_array().unwrap().len(), 2);

    // Alias: inspect-staged positional
    router_cmd()
        .args([
            "inspect-staged",
            id1,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(id1));

    // Alias: approve positional with --json
    let approve_json_out = router_cmd()
        .args([
            "approve",
            id1,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let approve_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(approve_json_out).unwrap()).unwrap();
    assert_eq!(approve_val["id"], id1);
    assert_eq!(approve_val["status"], "Approved");

    // Alias: reject positional with --json
    let reject_json_out = router_cmd()
        .args([
            "reject",
            id2,
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let reject_val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(reject_json_out).unwrap()).unwrap();
    assert_eq!(reject_val["id"], id2);
    assert_eq!(reject_val["status"], "Rejected");
}

#[test]
fn test_top_level_aliases_missing_arguments_exit_code_2() {
    // Missing required argument must fail with clap exit code 2
    router_cmd().arg("list-staged").assert().success(); // list-staged has optional outbox_dir with default

    router_cmd()
        .arg("inspect-staged")
        .assert()
        .failure()
        .code(2);
    router_cmd().arg("approve").assert().failure().code(2);
    router_cmd().arg("reject").assert().failure().code(2);
}

// ============================================================================
// 4. Adversarial Edge Cases & Robustness
// ============================================================================

#[test]
fn test_adversarial_path_traversal_attempts_in_outbox_cli() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");

    // Attempt traversal via -i
    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            "../../../../etc/passwd",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1);

    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "../secret",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1);

    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "/absolute/root",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn test_outbox_listing_tolerates_corrupted_and_foreign_files() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    std::fs::create_dir_all(&outbox_dir).unwrap();

    // 1. Stage a genuine task
    let env_file = temp.path().join("env.json");
    let id = "task_genuine_1";
    std::fs::write(&env_file, sample_external_envelope_json(id, None)).unwrap();
    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 2. Inject adversarial files into outbox directory
    // A. Random non-issue file
    std::fs::write(outbox_dir.join("random_notes.txt"), "some notes").unwrap();
    // B. Subdirectory
    std::fs::create_dir(outbox_dir.join("subfolder")).unwrap();
    // C. Hidden file
    std::fs::write(outbox_dir.join(".DS_Store"), "binary junk").unwrap();
    // D. Empty issue file
    std::fs::write(outbox_dir.join("issue_empty.md"), "").unwrap();
    // E. Corrupted / unparseable issue file
    std::fs::write(
        outbox_dir.join("issue_corrupted.md"),
        "Garbage content with no headers or keys\n",
    )
    .unwrap();

    // Run outbox list
    let list_out = router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0)
        .get_output()
        .stdout
        .clone();

    let val: serde_json::Value =
        serde_json::from_str(&String::from_utf8(list_out).unwrap()).unwrap();
    let arr = val.as_array().unwrap();
    // The genuine issue must be found!
    assert!(
        arr.iter().any(|item| item["id"] == id),
        "Genuine staged issue must be present in listing despite foreign files"
    );
}

#[test]
fn test_tampered_draft_with_conflicting_status_rejected_safely() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    std::fs::create_dir_all(&outbox_dir).unwrap();

    // Write a tampered markdown file that has conflicting status in the header
    let tampered_content = r#"# Staged Issue Draft: Tampered
- **ID**: task_tampered_999
- **Target**: upstream
- **Severity**: high
- **Created At**: 2026-10-11T19:00:00Z
- **Status**: Approved and Pending Human Authorization (Conflicting Tampered Line)

## Reproduction
```json
{}
```

## Expected
N/A

---
*Status: Pending Human Authorization (HITL)*
"#;

    let tampered_file = outbox_dir.join("issue_task_tampered_999.md");
    std::fs::write(&tampered_file, tampered_content).unwrap();

    // Attempting to approve must fail with OutboxError due to status corruption
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_tampered_999",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Corrupted or missing status"));
}

#[test]
fn test_json_error_reporting_on_domain_errors() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");

    let assert = router_cmd()
        .args([
            "approve",
            "nonexistent_task_id",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(val["status"], "error");
    assert_eq!(val["error"], "IssueNotFound");
    assert!(val["message"]
        .as_str()
        .unwrap()
        .contains("nonexistent_task_id"));
}
