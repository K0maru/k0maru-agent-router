//! Adversarial Stress Test Suite for Milestone M4: CLI Binary & Coordinator.
//!
//! Empirically challenges:
//! 1. CLI exit codes: missing arguments (exit 2), nonexistent files (exit 1),
//!    invalid JSON (exit 1), invalid envelope schema (exit 1), invalid workspace (exit 1).
//! 2. JSON output formatting with `--json`: parse validity and schema compliance
//!    across all subcommands (status, probe, route, outbox subcommands, aliases, error outputs).
//! 3. Dry-run mode: verify complete immutability of filesystem and multiplexer state.
//! 4. Edge cases: conflicting args, non-directory files as workspace, broken symlinks,
//!    double actions, and state machine validation.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("k0maru-router binary must be buildable via cargo_bin")
}

fn write_untrusted_envelope(path: &Path, id: &str) {
    let json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/test/caller",
            "agent": "challenger",
            "pane_id": "%1"
        },
        "target": {
            "identifier": "External-Lib",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Adversarial external issue",
            "severity": "critical",
            "reproduction": {
                "snippet": "broken_function()",
                "error": "Segmentation fault"
            },
            "expected": "Graceful failure"
        }
    });
    fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn write_trusted_envelope(path: &Path, id: &str, workspace: &Path) {
    let json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": id,
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/test/caller",
            "agent": "challenger",
            "pane_id": "%1"
        },
        "target": {
            "identifier": "Internal-Core",
            "workspace": workspace.to_string_lossy(),
            "is_external": false
        },
        "intent": "feature_request",
        "payload": {
            "title": "Adversarial internal task",
            "severity": "normal",
            "reproduction": null,
            "expected": "Task dispatched"
        }
    });
    fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

// ============================================================================
// PART 1: CLI EXIT CODES STRESS TESTS
// ============================================================================

#[test]
fn test_exit_code_2_on_missing_required_arguments() {
    // Top-level without subcommand
    router_cmd().assert().failure().code(2);

    // Probe missing workspace
    router_cmd().arg("probe").assert().failure().code(2);

    // Route missing file
    router_cmd().arg("route").assert().failure().code(2);

    // Outbox subcommands missing required id
    router_cmd()
        .args(["outbox", "inspect"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["outbox", "approve"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["outbox", "reject"])
        .assert()
        .failure()
        .code(2);

    // Aliases missing required id
    router_cmd()
        .arg("inspect-staged")
        .assert()
        .failure()
        .code(2);
    router_cmd().arg("approve").assert().failure().code(2);
    router_cmd().arg("reject").assert().failure().code(2);

    // Unknown subcommand
    router_cmd()
        .arg("nonexistent-command")
        .assert()
        .failure()
        .code(2);

    // Unknown options / flags
    router_cmd()
        .args(["status", "--unrecognized-option"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["probe", "-w", "/tmp", "--bad-flag"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["route", "-f", "/tmp/file.json", "--bogus"])
        .assert()
        .failure()
        .code(2);

    // Conflicting positional and flag argument
    router_cmd()
        .args(["probe", "-w", "/tmp", "/tmp"])
        .assert()
        .failure()
        .code(2);
    router_cmd()
        .args(["route", "-f", "/tmp/f1.json", "/tmp/f2.json"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn test_exit_code_1_on_nonexistent_files() {
    // Nonexistent file via flag
    router_cmd()
        .args(["route", "-f", "/definitely/nonexistent/envelope_9999.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("No such file or directory"));

    // Nonexistent file via positional
    router_cmd()
        .args(["route", "/definitely/nonexistent/envelope_9999.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("No such file or directory"));

    // Nonexistent file with --json (outputs structured JSON error to stderr)
    let assert = router_cmd()
        .args([
            "route",
            "-f",
            "/definitely/nonexistent/envelope_9999.json",
            "--json",
        ])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let err_val: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr must be valid JSON");
    assert_eq!(err_val["status"], "error");
    assert_eq!(err_val["error"], "Io");
    assert!(err_val["message"]
        .as_str()
        .unwrap()
        .contains("No such file or directory"));
}

#[test]
fn test_exit_code_1_on_invalid_json() {
    let temp = tempdir().unwrap();

    // Case A: Syntax error
    let malformed_file = temp.path().join("malformed.json");
    fs::write(&malformed_file, "{ \"id\": \"test\", syntax error }").unwrap();

    router_cmd()
        .args(["route", "-f", malformed_file.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("JSON"));

    let assert = router_cmd()
        .args(["route", "-f", malformed_file.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let err_val: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr must be valid JSON");
    assert_eq!(err_val["status"], "error");
    assert_eq!(err_val["error"], "Json");

    // Case B: Completely empty file
    let empty_file = temp.path().join("empty.json");
    fs::write(&empty_file, "").unwrap();

    router_cmd()
        .args(["route", "-f", empty_file.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("JSON"));

    // Case C: Valid JSON array instead of object
    let array_file = temp.path().join("array.json");
    fs::write(&array_file, "[\"item1\", \"item2\"]").unwrap();

    router_cmd()
        .args(["route", "-f", array_file.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("JSON"));
}

#[test]
fn test_exit_code_1_on_invalid_envelope_schema() {
    let temp = tempdir().unwrap();

    // 1. Missing required field 'created_at'
    let missing_field = temp.path().join("missing_field.json");
    fs::write(
        &missing_field,
        r#"{"id":"test_1","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","is_external":true},"intent":"bug","payload":{"title":"t","severity":"high","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    router_cmd()
        .args(["route", "-f", missing_field.to_str().unwrap()])
        .assert()
        .failure()
        .code(1);

    // 2. Empty string ID
    let empty_id = temp.path().join("empty_id.json");
    fs::write(
        &empty_id,
        r#"{"id":"","created_at":"2026-10-11T18:30:00Z","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","is_external":true},"intent":"bug","payload":{"title":"t","severity":"high","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    let assert_id = router_cmd()
        .args(["route", "-f", empty_id.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8(assert_id.get_output().stderr.clone()).unwrap();
    let err_val: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_val["status"], "error");
    assert_eq!(err_val["error"], "InvalidEnvelope");

    // 3. Malformed RFC-3339 timestamp
    let bad_timestamp = temp.path().join("bad_timestamp.json");
    fs::write(
        &bad_timestamp,
        r#"{"id":"t2","created_at":"2026-99-99T99:99:99Z","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","is_external":true},"intent":"bug","payload":{"title":"t","severity":"high","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    let assert_ts = router_cmd()
        .args(["route", "-f", bad_timestamp.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr_ts = String::from_utf8(assert_ts.get_output().stderr.clone()).unwrap();
    let err_val_ts: serde_json::Value = serde_json::from_str(&stderr_ts).unwrap();
    assert_eq!(err_val_ts["error"], "InvalidEnvelope");

    // 4. Invalid severity
    let bad_sev = temp.path().join("bad_sev.json");
    fs::write(
        &bad_sev,
        r#"{"id":"t3","created_at":"2026-10-11T18:30:00Z","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","is_external":true},"intent":"bug","payload":{"title":"t","severity":"nonexistent_severity","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    let assert_sev = router_cmd()
        .args(["route", "-f", bad_sev.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr_sev = String::from_utf8(assert_sev.get_output().stderr.clone()).unwrap();
    let err_val_sev: serde_json::Value = serde_json::from_str(&stderr_sev).unwrap();
    assert_eq!(err_val_sev["error"], "InvalidEnvelope");

    // 5. Internal target with missing workspace
    let internal_no_ws = temp.path().join("internal_no_ws.json");
    fs::write(
        &internal_no_ws,
        r#"{"id":"t4","created_at":"2026-10-11T18:30:00Z","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","is_external":false},"intent":"bug","payload":{"title":"t","severity":"normal","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    let assert_no_ws = router_cmd()
        .args(["route", "-f", internal_no_ws.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr_no_ws = String::from_utf8(assert_no_ws.get_output().stderr.clone()).unwrap();
    let err_val_no_ws: serde_json::Value = serde_json::from_str(&stderr_no_ws).unwrap();
    assert_eq!(err_val_no_ws["error"], "InvalidEnvelope");

    // 6. Internal target with empty/whitespace workspace
    let internal_empty_ws = temp.path().join("internal_empty_ws.json");
    fs::write(
        &internal_empty_ws,
        r#"{"id":"t5","created_at":"2026-10-11T18:30:00Z","caller":{"workspace":"/ws","agent":"a"},"target":{"identifier":"t","workspace":"   ","is_external":false},"intent":"bug","payload":{"title":"t","severity":"normal","reproduction":null,"expected":"exp"}}"#,
    ).unwrap();

    router_cmd()
        .args(["route", "-f", internal_empty_ws.to_str().unwrap()])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn test_exit_code_1_on_invalid_workspace() {
    let temp = tempdir().unwrap();

    // 1. Probe nonexistent directory
    router_cmd()
        .args(["probe", "-w", "/definitely/nonexistent/ws_98765"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));

    // 2. Probe nonexistent directory with --json
    let assert = router_cmd()
        .args(["probe", "-w", "/definitely/nonexistent/ws_98765", "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let err_val: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_val["status"], "error");
    assert_eq!(err_val["error"], "WorkspaceNotFound");

    // 3. Probe regular file instead of directory
    let regular_file = temp.path().join("not_a_dir.txt");
    fs::write(&regular_file, "content").unwrap();

    router_cmd()
        .args(["probe", "-w", regular_file.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));

    // 4. Route internal task pointing to nonexistent workspace
    let envelope_bad_ws = temp.path().join("env_bad_ws.json");
    write_trusted_envelope(
        &envelope_bad_ws,
        "env_bad_ws_task",
        Path::new("/nonexistent/ws/path"),
    );

    router_cmd()
        .args(["route", "-f", envelope_bad_ws.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));

    // 5. Route internal task pointing to regular file
    let envelope_file_ws = temp.path().join("env_file_ws.json");
    write_trusted_envelope(&envelope_file_ws, "env_file_ws_task", &regular_file);

    router_cmd()
        .args(["route", "-f", envelope_file_ws.to_str().unwrap(), "--json"])
        .assert()
        .failure()
        .code(1);

    // 6. Broken symlink as workspace
    #[cfg(unix)]
    {
        let broken_symlink = temp.path().join("broken_link");
        let _ = std::os::unix::fs::symlink("/nonexistent/destination_target", &broken_symlink);

        router_cmd()
            .args(["probe", "-w", broken_symlink.to_str().unwrap()])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("Target workspace not found"));
    }
}

// ============================================================================
// PART 2: JSON OUTPUT FORMATTING & SCHEMA COMPLIANCE STRESS TESTS
// ============================================================================

#[test]
fn test_json_formatting_across_all_subcommands() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("staged");
    let valid_ws = temp.path().join("valid_ws");
    fs::create_dir_all(&valid_ws).unwrap();

    // 1. status --json
    let assert_status = router_cmd()
        .args([
            "status",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_status = String::from_utf8(assert_status.get_output().stdout.clone()).unwrap();
    let val_status: serde_json::Value = serde_json::from_str(&stdout_status).unwrap();
    assert_eq!(val_status["version"], "0.1.0");
    assert_eq!(val_status["driver"]["name"], "tmux");
    assert!(val_status["driver"]["available"].is_boolean());
    assert_eq!(val_status["driver"]["sessions_count"], 0);
    assert_eq!(val_status["outbox"]["total"], 0);
    assert_eq!(val_status["outbox"]["pending"], 0);
    assert_eq!(val_status["outbox"]["approved"], 0);
    assert_eq!(val_status["outbox"]["rejected"], 0);

    // 2. probe --json
    let assert_probe = router_cmd()
        .args(["probe", "-w", valid_ws.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .code(0);
    let stdout_probe = String::from_utf8(assert_probe.get_output().stdout.clone()).unwrap();
    let val_probe: serde_json::Value = serde_json::from_str(&stdout_probe).unwrap();
    assert!(val_probe.get("ColdSpawn").is_some() || val_probe.get("HotSession").is_some());

    // 3. outbox list --json (empty array)
    let assert_list_empty = router_cmd()
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
    let stdout_list_empty =
        String::from_utf8(assert_list_empty.get_output().stdout.clone()).unwrap();
    let val_list_empty: serde_json::Value = serde_json::from_str(&stdout_list_empty).unwrap();
    assert!(val_list_empty.is_array());
    assert_eq!(val_list_empty.as_array().unwrap().len(), 0);

    // 4. route untrusted external task with --json
    let ext_env = temp.path().join("ext_envelope.json");
    write_untrusted_envelope(&ext_env, "task_json_test_1");

    let assert_route_ext = router_cmd()
        .args([
            "route",
            "-f",
            ext_env.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_route_ext = String::from_utf8(assert_route_ext.get_output().stdout.clone()).unwrap();
    let val_route_ext: serde_json::Value = serde_json::from_str(&stdout_route_ext).unwrap();
    assert!(val_route_ext["Staged"].is_object());
    assert!(val_route_ext["Staged"]["issue_path"].is_string());
    assert!(val_route_ext["Staged"]["reason"].is_string());

    // 5. outbox list --json (populated with 1 staged issue)
    let assert_list_pop = router_cmd()
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
    let stdout_list_pop = String::from_utf8(assert_list_pop.get_output().stdout.clone()).unwrap();
    let val_list_pop: serde_json::Value = serde_json::from_str(&stdout_list_pop).unwrap();
    assert!(val_list_pop.is_array());
    let items = val_list_pop.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "task_json_test_1");
    assert_eq!(items[0]["status"], "Pending");
    assert_eq!(items[0]["target"], "External-Lib");
    assert_eq!(items[0]["severity"], "critical");

    // 6. alias list-staged --json
    let assert_alias_list = router_cmd()
        .args([
            "list-staged",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_alias = String::from_utf8(assert_alias_list.get_output().stdout.clone()).unwrap();
    let val_alias: serde_json::Value = serde_json::from_str(&stdout_alias).unwrap();
    assert_eq!(val_alias, val_list_pop);

    // 7. outbox approve --json
    let assert_approve = router_cmd()
        .args([
            "outbox",
            "approve",
            "--id",
            "task_json_test_1",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_approve = String::from_utf8(assert_approve.get_output().stdout.clone()).unwrap();
    let val_approve: serde_json::Value = serde_json::from_str(&stdout_approve).unwrap();
    assert_eq!(val_approve["id"], "task_json_test_1");
    assert_eq!(val_approve["status"], "Approved");
    assert!(val_approve["file_path"].is_string());
    assert!(val_approve["message"].is_string());

    // 8. outbox reject --json on already approved task fails with OutboxError in JSON
    let assert_reject_err = router_cmd()
        .args([
            "outbox",
            "reject",
            "--id",
            "task_json_test_1",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .code(1);
    let stderr_reject = String::from_utf8(assert_reject_err.get_output().stderr.clone()).unwrap();
    let val_reject_err: serde_json::Value = serde_json::from_str(&stderr_reject).unwrap();
    assert_eq!(val_reject_err["status"], "error");
    assert_eq!(val_reject_err["error"], "OutboxError");

    // 9. Stage a second issue and reject it cleanly with alias reject --json
    let ext_env2 = temp.path().join("ext_envelope2.json");
    write_untrusted_envelope(&ext_env2, "task_json_test_2");
    router_cmd()
        .args([
            "route",
            "-f",
            ext_env2.to_str().unwrap(),
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .code(0);

    let assert_alias_reject = router_cmd()
        .args([
            "reject",
            "task_json_test_2",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_alias_reject =
        String::from_utf8(assert_alias_reject.get_output().stdout.clone()).unwrap();
    let val_alias_reject: serde_json::Value = serde_json::from_str(&stdout_alias_reject).unwrap();
    assert_eq!(val_alias_reject["id"], "task_json_test_2");
    assert_eq!(val_alias_reject["status"], "Rejected");
    assert!(val_alias_reject["message"].is_string());

    // 10. route internal task with --dry-run --json
    let int_env = temp.path().join("int_envelope.json");
    write_trusted_envelope(&int_env, "task_internal_dry_json", &valid_ws);

    let assert_dry_json = router_cmd()
        .args([
            "route",
            "-f",
            int_env.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .code(0);
    let stdout_dry = String::from_utf8(assert_dry_json.get_output().stdout.clone()).unwrap();
    let val_dry: serde_json::Value = serde_json::from_str(&stdout_dry).unwrap();
    assert!(val_dry["DryRun"].is_object());
    assert_eq!(val_dry["DryRun"]["verdict"], "TrustedInternal");
    assert!(val_dry["DryRun"]["resolution"].is_object());
}

// ============================================================================
// PART 3: DRY-RUN IMMUTABILITY STRESS TESTS
// ============================================================================

#[test]
fn test_dry_run_leaves_filesystem_and_outbox_completely_untouched() {
    let temp = tempdir().unwrap();

    // 1. External task dry-run pointing to a nonexistent outbox directory
    let nonexistent_outbox = temp.path().join("never_created_outbox");
    assert!(
        !nonexistent_outbox.exists(),
        "Pre-condition: outbox must not exist"
    );

    let ext_env = temp.path().join("ext_task.json");
    write_untrusted_envelope(&ext_env, "dry_run_task_alpha");

    router_cmd()
        .args([
            "route",
            "-f",
            ext_env.to_str().unwrap(),
            "--outbox-dir",
            nonexistent_outbox.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[DRY-RUN]"))
        .stdout(predicate::str::contains("UntrustedExternal"))
        .stdout(predicate::str::contains("simulated, no mutation"));

    assert!(
        !nonexistent_outbox.exists(),
        "Verification: dry-run must NOT create outbox directory on disk!"
    );

    // Also verify with --json
    let assert_json = router_cmd()
        .args([
            "route",
            "-f",
            ext_env.to_str().unwrap(),
            "--outbox-dir",
            nonexistent_outbox.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert_json.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(val["DryRun"]["verdict"]["UntrustedExternal"].is_object());
    assert!(val["DryRun"]["resolution"].is_null());

    assert!(
        !nonexistent_outbox.exists(),
        "Verification: dry-run with --json must NOT create outbox directory on disk!"
    );

    // 2. Pre-existing populated outbox directory remains bit-for-bit unchanged
    let existing_outbox = temp.path().join("existing_outbox");
    fs::create_dir_all(&existing_outbox).unwrap();
    let dummy_file = existing_outbox.join("canary.txt");
    fs::write(&dummy_file, "original content canary").unwrap();
    let original_mtime = fs::metadata(&dummy_file).unwrap().modified().unwrap();

    router_cmd()
        .args([
            "route",
            "-f",
            ext_env.to_str().unwrap(),
            "--outbox-dir",
            existing_outbox.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .code(0);

    let entries: Vec<_> = fs::read_dir(&existing_outbox).unwrap().collect();
    assert_eq!(
        entries.len(),
        1,
        "Existing outbox directory entry count must remain exactly 1"
    );
    assert_eq!(
        fs::read_to_string(&dummy_file).unwrap(),
        "original content canary"
    );
    let after_mtime = fs::metadata(&dummy_file).unwrap().modified().unwrap();
    assert_eq!(
        original_mtime, after_mtime,
        "Canary file must not be touched"
    );

    // 3. Internal task dry-run with valid directory
    let valid_ws = temp.path().join("my_internal_repo");
    fs::create_dir_all(&valid_ws).unwrap();
    let int_env = temp.path().join("int_task.json");
    write_trusted_envelope(&int_env, "dry_run_task_beta", &valid_ws);

    router_cmd()
        .args(["route", "-f", int_env.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[DRY-RUN]"))
        .stdout(predicate::str::contains("TrustedInternal"))
        .stdout(predicate::str::contains("simulated, no mutation"));

    // 4. Internal task dry-run fail-fast on missing directory
    let bad_ws = temp.path().join("nonexistent_repo");
    let bad_int_env = temp.path().join("bad_int_task.json");
    write_trusted_envelope(&bad_int_env, "dry_run_bad_ws", &bad_ws);

    router_cmd()
        .args(["route", "-f", bad_int_env.to_str().unwrap(), "--dry-run"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));
}

// ============================================================================
// PART 4: OUTBOX STATE MACHINE & EDGE CASES
// ============================================================================

#[test]
fn test_outbox_operations_on_nonexistent_ids_fail_with_exit_1() {
    // Inspect nonexistent
    router_cmd()
        .args(["inspect-staged", "nonexistent_task_xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));

    // Approve nonexistent text
    router_cmd()
        .args(["approve", "nonexistent_task_xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));

    // Approve nonexistent JSON
    let assert_app_json = router_cmd()
        .args(["approve", "nonexistent_task_xyz", "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr_app = String::from_utf8(assert_app_json.get_output().stderr.clone()).unwrap();
    let val_app: serde_json::Value = serde_json::from_str(&stderr_app).unwrap();
    assert_eq!(val_app["status"], "error");
    assert_eq!(val_app["error"], "IssueNotFound");

    // Reject nonexistent text
    router_cmd()
        .args(["reject", "nonexistent_task_xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));

    // Reject nonexistent JSON
    let assert_rej_json = router_cmd()
        .args(["reject", "nonexistent_task_xyz", "--json"])
        .assert()
        .failure()
        .code(1);
    let stderr_rej = String::from_utf8(assert_rej_json.get_output().stderr.clone()).unwrap();
    let val_rej: serde_json::Value = serde_json::from_str(&stderr_rej).unwrap();
    assert_eq!(val_rej["status"], "error");
    assert_eq!(val_rej["error"], "IssueNotFound");
}
