//! End-to-end integration tests for k0maru-router CLI and Coordinator.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::tempdir;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("k0maru-router binary must be buildable via cargo_bin")
}

fn write_external_envelope(path: &Path, id: &str) {
    let json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/test/caller",
            "agent": "agy",
            "pane_id": "%1"
        },
        "target": {
            "identifier": "Terra-UI",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "TerraButton aria-label bug",
            "severity": "blocking",
            "reproduction": {
                "snippet": "<TerraButton aria-label='close' />",
                "error": "HTML button element does not receive aria-label"
            },
            "expected": "Attribute forwarded to rendered <button> element"
        }
    });
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn write_internal_envelope(path: &Path, id: &str, workspace: &Path) {
    let json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/test/caller",
            "agent": "worker-1",
            "pane_id": "%1"
        },
        "target": {
            "identifier": "internal-core",
            "workspace": workspace.to_string_lossy(),
            "is_external": false
        },
        "intent": "bug_report",
        "payload": {
            "title": "Internal logic defect",
            "severity": "high",
            "reproduction": {
                "snippet": "let res = process();",
                "error": "Panic in core"
            },
            "expected": "Clean return"
        }
    });
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

// ============================================================================
// Suite 1: Status & Probe Commands
// ============================================================================

#[test]
fn test_cli_status_text() {
    router_cmd()
        .arg("status")
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("k0maru-router"))
        .stdout(predicate::str::contains("Multiplexer Driver: tmux"))
        .stdout(predicate::str::contains("HITL Outbox"));
}

#[test]
fn test_cli_status_json() {
    let assert = router_cmd()
        .args(["status", "--json"])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["version"], "0.1.0");
    assert_eq!(val["driver"]["name"], "tmux");
    assert!(val["driver"]["available"].is_boolean());
    assert!(val["outbox"]["total"].is_number());
}

#[test]
fn test_cli_probe_valid_directory() {
    let temp = tempdir().unwrap();
    let ws_path = temp.path().to_str().unwrap();

    router_cmd()
        .args(["probe", "-w", ws_path])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("workspace").or(predicate::str::contains("session")));
}

#[test]
fn test_cli_probe_valid_directory_json() {
    let temp = tempdir().unwrap();
    let ws_path = temp.path().to_str().unwrap();

    let assert = router_cmd()
        .args(["probe", "-w", ws_path, "--json"])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // In resolution enum serialization, ColdSpawn or HotSession
    assert!(val.get("ColdSpawn").is_some() || val.get("HotSession").is_some());
}

#[test]
fn test_cli_probe_nonexistent_directory() {
    router_cmd()
        .args(["probe", "-w", "/definitely/nonexistent/dir_999999"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));
}

#[test]
fn test_cli_probe_nonexistent_directory_json() {
    let assert = router_cmd()
        .args([
            "probe",
            "-w",
            "/definitely/nonexistent/dir_999999",
            "--json",
        ])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(val["status"], "error");
    assert_eq!(val["error"], "WorkspaceNotFound");
}

#[test]
fn test_cli_probe_missing_workspace_arg() {
    router_cmd().arg("probe").assert().failure().code(2);
}

// ============================================================================
// Suite 2: Route Command - Untrusted External Staging & Quarantine
// ============================================================================

#[test]
fn test_cli_route_untrusted_stages_to_outbox() {
    let temp = tempdir().unwrap();
    let env_path = temp.path().join("task_ext.json");
    let outbox_dir = temp.path().join("outbox");
    write_external_envelope(&env_path, "task_ext_stage_1");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[QUARANTINED]"))
        .stdout(predicate::str::contains("task_ext_stage_1"));

    let staged_file = outbox_dir.join("issue_task_ext_stage_1.md");
    assert!(staged_file.exists());
    let content = std::fs::read_to_string(&staged_file).unwrap();
    assert!(content.contains("- **ID**: task_ext_stage_1"));
    assert!(content.contains("- **Target**: Terra-UI"));
    assert!(content.contains("Pending Human Authorization (HITL)"));
}

#[test]
fn test_cli_route_untrusted_json_output() {
    let temp = tempdir().unwrap();
    let env_path = temp.path().join("task_ext.json");
    let outbox_dir = temp.path().join("outbox");
    write_external_envelope(&env_path, "task_ext_json_1");

    let assert = router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(val.get("Staged").is_some());
    let staged = &val["Staged"];
    assert!(staged["reason"].as_str().unwrap().contains("Terra-UI"));
}

#[test]
fn test_cli_route_dry_run_untrusted_zero_filesystem_mutation() {
    let temp = tempdir().unwrap();
    let env_path = temp.path().join("task_ext.json");
    let outbox_dir = temp.path().join("outbox");
    write_external_envelope(&env_path, "task_ext_dry_1");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[DRY-RUN]"))
        .stdout(predicate::str::contains("UntrustedExternal"));

    // Absolute invariant: zero filesystem mutations
    assert!(!outbox_dir.exists());
}

// ============================================================================
// Suite 3: Route Command - Trusted Internal & Dry-Run
// ============================================================================

#[test]
fn test_cli_route_internal_dry_run() {
    let temp = tempdir().unwrap();
    let ws = temp.path().join("my_workspace");
    std::fs::create_dir(&ws).unwrap();

    let env_path = temp.path().join("task_int.json");
    write_internal_envelope(&env_path, "task_int_dry", &ws);

    router_cmd()
        .args(["route", "-f", env_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[DRY-RUN]"))
        .stdout(predicate::str::contains("TrustedInternal"));
}

#[test]
fn test_cli_route_internal_dry_run_json() {
    let temp = tempdir().unwrap();
    let ws = temp.path().join("my_workspace");
    std::fs::create_dir(&ws).unwrap();

    let env_path = temp.path().join("task_int.json");
    write_internal_envelope(&env_path, "task_int_dry_json", &ws);

    let assert = router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(val.get("DryRun").is_some());
    assert_eq!(val["DryRun"]["verdict"], "TrustedInternal");
}

#[test]
fn test_cli_route_internal_missing_workspace_fails_fast() {
    let temp = tempdir().unwrap();
    let missing_ws = temp.path().join("nonexistent_ws_dir");
    let env_path = temp.path().join("task_int.json");
    write_internal_envelope(&env_path, "task_int_missing", &missing_ws);

    router_cmd()
        .args(["route", "-f", env_path.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));
}

// ============================================================================
// Suite 4: Outbox Subcommands & State Lifecycle
// ============================================================================

#[test]
fn test_cli_outbox_list_empty_and_populated() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");

    // Empty listing
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
        .stdout(predicate::str::contains("No staged issues found"));

    // Empty listing JSON
    let assert = router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val.as_array().unwrap().len(), 0);

    // Populate with 1 staged draft
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_listed_1");
    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Listing populated
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
        .stdout(predicate::str::contains("task_listed_1"))
        .stdout(predicate::str::contains("Pending"));

    // Listing populated JSON
    let assert2 = router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout2 = String::from_utf8(assert2.get_output().stdout.clone()).unwrap();
    let val2: serde_json::Value = serde_json::from_str(&stdout2).unwrap();
    assert_eq!(val2.as_array().unwrap().len(), 1);
    assert_eq!(val2[0]["id"], "task_listed_1");
}

#[test]
fn test_cli_outbox_inspect() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_inspect_1");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            "task_inspect_1",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "# Staged Issue Draft: TerraButton aria-label bug",
        ))
        .stdout(predicate::str::contains(
            "HTML button element does not receive aria-label",
        ));
}

#[test]
fn test_cli_outbox_approve_lifecycle() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_app_life");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 1. Approve
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_app_life",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "Approved staged issue 'task_app_life'",
        ));

    // 2. Re-approve must fail
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_app_life",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already approved"));
}

#[test]
fn test_cli_outbox_reject_lifecycle() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_rej_life");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 1. Reject
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "task_rej_life",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "Rejected staged issue 'task_rej_life'",
        ));

    // 2. Re-reject must fail
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "task_rej_life",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already rejected"));

    // 3. Approve rejected must fail
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_rej_life",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("status is Rejected"));
}

// ============================================================================
// Suite 5: Top-Level Convenience Aliases
// ============================================================================

#[test]
fn test_cli_aliases_list_inspect_approve_reject() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_alias_1");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Alias: list-staged
    router_cmd()
        .args(["list-staged", "--outbox-dir", outbox_dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("task_alias_1"));

    // Alias: inspect-staged
    router_cmd()
        .args([
            "inspect-staged",
            "-i",
            "task_alias_1",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Staged Issue Draft:"));

    // Alias: approve
    router_cmd()
        .args([
            "approve",
            "-i",
            "task_alias_1",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Approved staged issue 'task_alias_1'",
        ));

    // Stage second issue for reject alias
    let env_path2 = temp.path().join("task_ext2.json");
    write_external_envelope(&env_path2, "task_alias_2");
    router_cmd()
        .args([
            "route",
            "-f",
            env_path2.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Alias: reject
    router_cmd()
        .args([
            "reject",
            "-i",
            "task_alias_2",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Rejected staged issue 'task_alias_2'",
        ));
}

// ============================================================================
// Suite 6: Error Handling & Exit Codes
// ============================================================================

#[test]
fn test_cli_error_missing_envelope_file() {
    router_cmd()
        .args(["route", "-f", "/path/to/missing_envelope_xyz.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("No such file or directory"));
}

#[test]
fn test_cli_error_corrupt_json() {
    let temp = tempdir().unwrap();
    let bad_json = temp.path().join("corrupt.json");
    std::fs::write(&bad_json, b"{ not valid json").unwrap();

    router_cmd()
        .args(["route", "-f", bad_json.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "JSON serialization/deserialization error",
        ));
}

#[test]
fn test_cli_error_empty_envelope_file() {
    let temp = tempdir().unwrap();
    let empty_file = temp.path().join("empty.json");
    std::fs::write(&empty_file, b"").unwrap();

    router_cmd()
        .args(["route", "-f", empty_file.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Task envelope JSON string cannot be empty",
        ));
}

#[test]
fn test_cli_error_unsupported_driver() {
    let temp = tempdir().unwrap();
    let env_path = temp.path().join("task.json");
    write_external_envelope(&env_path, "task_driver_test");

    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--driver",
            "unsupported_mux",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Driver 'unsupported_mux' is not supported",
        ));
}

#[test]
fn test_cli_outbox_inspect_nonexistent_issue() {
    let temp = tempdir().unwrap();
    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            "task_missing_888",
            "--outbox-dir",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Staged issue not found: 'task_missing_888'",
        ));
}

#[test]
fn test_cli_invalid_arguments_exit_code_2() {
    // Missing required flag
    router_cmd().arg("route").assert().failure().code(2);

    // Unknown subcommand
    router_cmd()
        .arg("unknown-command")
        .assert()
        .failure()
        .code(2);

    // Unknown option
    router_cmd()
        .args(["status", "--bogus-flag"])
        .assert()
        .failure()
        .code(2);
}
