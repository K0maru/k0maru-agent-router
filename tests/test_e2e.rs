//! Consolidated End-to-End Integration Tests for k0maru-router CLI and Coordinator.
//!
//! Covers:
//! - CLI status and probe commands
//! - CLI route commands (untrusted quarantine staging, internal dry-run, rule file, no-memory flag)
//! - CLI outbox lifecycle (list, inspect, approve, reject, double action rejections)
//! - CLI receipt lifecycle (ingest with patch verification, status, list)
//! - CLI error handling and exit codes (missing file, corrupt json, exit code 2)
//! - Coordinator Rust API closed loop (sandbox isolation, driver dispatch, receipt verification, memory enrichment)

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use assert_cmd::Command;
use async_trait::async_trait;
use predicates::prelude::*;
use sha2::{Digest, Sha256};

use k0maru_router::coordinator::{
    format_agent_prompt_with_context, RouteOutcome, RouterCoordinator,
};
use k0maru_router::decision::rule::{DeterministicRuleEngine, MatchPattern, Rule};
use k0maru_router::driver::{LiveSession, MultiplexerDriver, SessionStatus, TmuxDriver};
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;
use k0maru_router::memory::MemoryHook;
use k0maru_router::receipt::{
    CompilationCheck, ExecutionChecks, ExecutionReceipt, LinterCheck, ReceiptArtifacts,
    ReceiptVerdict, ScopeGuardCheck, TestCheck, DEFAULT_RECEIPT_SCHEMA,
};
use k0maru_router::workspace::WorkspaceProvider;

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
    fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
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
    fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn create_sample_receipt(
    task_id: &str,
    workspace: &Path,
    patch_content: &[u8],
) -> ExecutionReceipt {
    let mut hasher = Sha256::new();
    hasher.update(patch_content);
    let patch_sha256 = format!("{:x}", hasher.finalize());

    ExecutionReceipt {
        schema: DEFAULT_RECEIPT_SCHEMA.to_string(),
        task_id: task_id.to_string(),
        workspace: workspace.to_path_buf(),
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
                duration_ms: Some(500),
            },
            linter: LinterCheck {
                status: "passed".to_string(),
                warnings: 0,
            },
            scope_guard: ScopeGuardCheck {
                status: "passed".to_string(),
                modified_files: vec![],
            },
        },
        artifacts: ReceiptArtifacts {
            git_patch_sha256: Some(patch_sha256),
            receipt_timestamp: Some("2026-10-12T00:00:00Z".to_string()),
        },
    }
}

// ============================================================================
// Suite 1: Status & Probe
// ============================================================================

#[test]
fn test_cli_status_text_and_json() {
    // Text output
    router_cmd()
        .arg("status")
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("k0maru-router"))
        .stdout(predicate::str::contains("Multiplexer Driver: tmux"))
        .stdout(predicate::str::contains("HITL Outbox"));

    // JSON output
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
fn test_cli_probe_workspace() {
    let temp = tempdir().unwrap();
    let ws_path = temp.path().to_str().unwrap();

    // Valid directory text
    router_cmd()
        .args(["probe", "--workspace", ws_path])
        .assert()
        .success()
        .code(0)
        .stdout(
            predicate::str::contains("no active session")
                .or(predicate::str::contains("Live Sessions:")),
        );

    // Valid directory JSON
    let assert = router_cmd()
        .args(["probe", "--workspace", ws_path, "--json"])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(val.get("ColdSpawn").is_some() || val.get("HotSession").is_some());

    // Nonexistent directory error
    router_cmd()
        .args(["probe", "--workspace", "/nonexistent/dir/xyz"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Target workspace not found"));
}

// ============================================================================
// Suite 2: Route Command
// ============================================================================

#[test]
fn test_cli_route_untrusted_stages_to_outbox() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env_path = temp.path().join("task_ext.json");
    write_external_envelope(&env_path, "task_route_ext_01");

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
        .stdout(predicate::str::contains("task_route_ext_01"))
        .stdout(predicate::str::contains("Terra-UI"));

    let staged_file = outbox_dir.join("issue_task_route_ext_01.md");
    assert!(staged_file.exists());
}

#[test]
fn test_cli_route_internal_dry_run() {
    let temp = tempdir().unwrap();
    let ws_dir = temp.path().join("workspace");
    fs::create_dir(&ws_dir).unwrap();
    let env_path = temp.path().join("task_int.json");
    write_internal_envelope(&env_path, "task_route_int_01", &ws_dir);

    // Text dry run
    router_cmd()
        .args(["route", "-f", env_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[DRY-RUN]"))
        .stdout(predicate::str::contains("TrustedInternal"));

    // JSON dry run
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
fn test_cli_route_with_rule_file_and_no_memory() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let target_dir = temp.path().join("target_ws");
    fs::create_dir(&target_dir).unwrap();

    let rule_file = temp.path().join("rules.json");
    let rules_json = serde_json::json!({
        "rules": [
            {
                "target": "billing-service",
                "match_title": {
                    "type": "regex",
                    "value": "(?i)stripe|invoice"
                },
                "confidence": 0.95,
                "action": "act"
            }
        ]
    });
    fs::write(&rule_file, serde_json::to_string(&rules_json).unwrap()).unwrap();

    let env_path = temp.path().join("task_rule.json");
    let env_json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_rule_01",
        "created_at": "2026-10-12T00:00:00Z",
        "caller": { "workspace": "/tmp", "agent": "worker" },
        "target": {
            "identifier": "auto",
            "workspace": target_dir.to_str().unwrap(),
            "is_external": false
        },
        "intent": "bug_report",
        "payload": {
            "title": "Stripe webhook dropped event",
            "severity": "high"
        }
    });
    fs::write(&env_path, serde_json::to_string(&env_json).unwrap()).unwrap();

    let assert = router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--rule-file",
            rule_file.to_str().unwrap(),
            "--no-memory",
            "--dry-run",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
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

// ============================================================================
// Suite 3: Outbox Lifecycle (Standardized outbox [list|inspect|approve|reject])
// ============================================================================

#[test]
fn test_cli_outbox_lifecycle() {
    let temp = tempdir().unwrap();
    let outbox_dir = temp.path().join("outbox");
    let env1 = temp.path().join("env1.json");
    let env2 = temp.path().join("env2.json");
    write_external_envelope(&env1, "task_outbox_01");
    write_external_envelope(&env2, "task_outbox_02");

    // Stage both
    for path in [&env1, &env2] {
        router_cmd()
            .args([
                "route",
                "-f",
                path.to_str().unwrap(),
                "--outbox-dir",
                outbox_dir.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    // 1. outbox list
    router_cmd()
        .args([
            "outbox",
            "list",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("task_outbox_01"))
        .stdout(predicate::str::contains("task_outbox_02"));

    // 2. outbox inspect
    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            "task_outbox_01",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "# Staged Issue Draft: TerraButton aria-label bug",
        ));

    // 3. outbox approve
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_outbox_01",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Approved staged issue 'task_outbox_01'",
        ));

    // Re-approve fails
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_outbox_01",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already approved"));

    // 4. outbox reject
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "task_outbox_02",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Rejected staged issue 'task_outbox_02'",
        ));

    // Re-reject fails
    router_cmd()
        .args([
            "outbox",
            "reject",
            "-i",
            "task_outbox_02",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already rejected"));

    // Approve rejected fails
    router_cmd()
        .args([
            "outbox",
            "approve",
            "-i",
            "task_outbox_02",
            "--outbox-dir",
            outbox_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("status is Rejected"));
}

// ============================================================================
// Suite 4: Receipt Lifecycle (Standardized receipt [ingest|status|list])
// ============================================================================

#[test]
fn test_cli_receipt_lifecycle() {
    let temp = tempdir().unwrap();
    let receipts_dir = temp.path().join("receipts");
    let receipt_file = temp.path().join("receipt.json");
    let patch_file = temp.path().join("diff.patch");

    let patch_bytes = b"index 123..456\n+pub fn test_receipt() {}";
    fs::write(&patch_file, patch_bytes).unwrap();

    let receipt = create_sample_receipt(
        "task_receipt_01",
        &PathBuf::from("/workspaces/app"),
        patch_bytes,
    );
    fs::write(
        &receipt_file,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    // 1. receipt ingest
    router_cmd()
        .args([
            "receipt",
            "ingest",
            "--file",
            receipt_file.to_str().unwrap(),
            "--patch",
            patch_file.to_str().unwrap(),
            "--receipt-dir",
            receipts_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    // 2. receipt status
    let assert = router_cmd()
        .args([
            "receipt",
            "status",
            "task_receipt_01",
            "--receipt-dir",
            receipts_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["task_id"], "task_receipt_01");
    assert_eq!(val["verdict"], "success");

    // 3. receipt list
    let assert_list = router_cmd()
        .args([
            "receipt",
            "list",
            "--receipt-dir",
            receipts_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout_list = String::from_utf8(assert_list.get_output().stdout.clone()).unwrap();
    let list_val: Vec<serde_json::Value> = serde_json::from_str(&stdout_list).unwrap();
    assert_eq!(list_val.len(), 1);
    assert_eq!(list_val[0]["task_id"], "task_receipt_01");
}

#[test]
fn test_cli_receipt_lifecycle_with_env_var() {
    let temp = tempdir().unwrap();
    let env_receipts_dir = temp.path().join("env_receipts");
    let cli_override_dir = temp.path().join("cli_override_receipts");
    let receipt_file = temp.path().join("receipt_env.json");
    let patch_file = temp.path().join("diff_env.patch");

    let patch_bytes = b"index 789..abc\n+pub fn test_env_receipt() {}";
    fs::write(&patch_file, patch_bytes).unwrap();

    let receipt = create_sample_receipt(
        "task_receipt_env_01",
        &PathBuf::from("/workspaces/env_app"),
        patch_bytes,
    );
    fs::write(
        &receipt_file,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    // 1. Ingest using K0MARU_RECEIPT_DIR env var (no --receipt-dir flag)
    router_cmd()
        .env("K0MARU_RECEIPT_DIR", env_receipts_dir.to_str().unwrap())
        .args([
            "receipt",
            "ingest",
            "--file",
            receipt_file.to_str().unwrap(),
            "--patch",
            patch_file.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    // Verify it was stored in env_receipts_dir
    assert!(env_receipts_dir.join("task_receipt_env_01.json").is_file());

    // 2. Query status via env var
    let assert = router_cmd()
        .env("K0MARU_RECEIPT_DIR", env_receipts_dir.to_str().unwrap())
        .args(["receipt", "status", "task_receipt_env_01", "--json"])
        .assert()
        .success()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["task_id"], "task_receipt_env_01");
    assert_eq!(val["verdict"], "success");

    // 3. List via env var
    let assert_list = router_cmd()
        .env("K0MARU_RECEIPT_DIR", env_receipts_dir.to_str().unwrap())
        .args(["receipt", "list", "--json"])
        .assert()
        .success()
        .code(0);

    let stdout_list = String::from_utf8(assert_list.get_output().stdout.clone()).unwrap();
    let list_val: Vec<serde_json::Value> = serde_json::from_str(&stdout_list).unwrap();
    assert_eq!(list_val.len(), 1);
    assert_eq!(list_val[0]["task_id"], "task_receipt_env_01");

    // 4. Priority 1 override: CLI --receipt-dir overrides K0MARU_RECEIPT_DIR
    router_cmd()
        .env("K0MARU_RECEIPT_DIR", env_receipts_dir.to_str().unwrap())
        .args([
            "receipt",
            "ingest",
            "--file",
            receipt_file.to_str().unwrap(),
            "--receipt-dir",
            cli_override_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    // Stored in cli_override_dir, not env_receipts_dir
    assert!(cli_override_dir.join("task_receipt_env_01.json").is_file());
}

// ============================================================================
// Suite 5: Error Handling & Exit Codes
// ============================================================================

#[test]
fn test_cli_error_handling_and_exit_codes() {
    let temp = tempdir().unwrap();

    // Missing file
    router_cmd()
        .args(["route", "-f", "/path/to/missing_xyz.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("No such file or directory"));

    // Corrupt JSON
    let bad_json = temp.path().join("bad.json");
    fs::write(&bad_json, b"{ not json").unwrap();
    router_cmd()
        .args(["route", "-f", bad_json.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "JSON serialization/deserialization error",
        ));

    // Empty envelope
    let empty_json = temp.path().join("empty.json");
    fs::write(&empty_json, b"").unwrap();
    router_cmd()
        .args(["route", "-f", empty_json.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Task envelope JSON string cannot be empty",
        ));

    // Unsupported driver
    let env_path = temp.path().join("env.json");
    write_external_envelope(&env_path, "task_err_drv");
    router_cmd()
        .args([
            "route",
            "-f",
            env_path.to_str().unwrap(),
            "--driver",
            "bogus_driver",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Driver 'bogus_driver' is not supported",
        ));

    // Nonexistent outbox issue inspect
    router_cmd()
        .args([
            "outbox",
            "inspect",
            "-i",
            "task_nonexistent_999",
            "--outbox-dir",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Staged issue not found: 'task_nonexistent_999'",
        ));

    // Missing required args => exit code 2
    router_cmd().arg("route").assert().failure().code(2);
    router_cmd().arg("unknown-cmd").assert().failure().code(2);
    router_cmd()
        .args(["status", "--unknown-opt"])
        .assert()
        .failure()
        .code(2);
}

// ============================================================================
// Suite 6: Coordinator API & Closed-Loop Integration
// ============================================================================

#[derive(Debug)]
struct MockMemoryHub;

impl MemoryHook for MockMemoryHub {
    fn name(&self) -> &'static str {
        "k0maru-agent-memory"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn preload_context(
        &self,
        _workspace: &Path,
        project_name: &str,
    ) -> Result<Option<String>, RouterError> {
        Ok(Some(format!(
            "Project: {}\nBlueprint Invariant: Zero synthetic fallback",
            project_name
        )))
    }
}

#[test]
fn test_coordinator_prompt_memory_enrichment() {
    let envelope = TaskEnvelope::new(
        "task_mem_001",
        "2026-10-12T00:00:00Z",
        CallerInfo::new("/ws/caller", "worker-1"),
        TargetInfo::new_internal("core-service", "/ws/core"),
        "feature",
        TaskPayload::new("Implement cache", "normal"),
    );

    let memory_hub = MockMemoryHub;
    let preloaded = memory_hub
        .preload_context(Path::new("/ws/core"), "core-service")
        .unwrap();

    let enriched_prompt = format_agent_prompt_with_context(&envelope, preloaded.as_deref());

    assert!(enriched_prompt.contains("Architectural Context & Blueprint:"));
    assert!(enriched_prompt.contains("Project: core-service"));
    assert!(enriched_prompt.contains("Zero synthetic fallback"));
}

#[test]
fn test_coordinator_route_envelope_with_rule_engine() {
    let outbox = tempdir().unwrap();
    let target_dir = tempdir().unwrap();

    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(
        Rule::new("payment-service")
            .with_match_title(MatchPattern::regex(r"(?i)payment|stripe"))
            .with_confidence(0.96)
            .with_action("act"),
    );

    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver)
        .with_outbox_dir(outbox.path())
        .with_decision_engine(rule_engine);

    let envelope = TaskEnvelope::new(
        "task_ambiguous_01",
        "2026-10-12T00:00:00Z",
        CallerInfo::new("/ws/caller", "test-agent"),
        TargetInfo::new_internal("auto", target_dir.path().to_str().unwrap()),
        "fix_bug",
        TaskPayload::new("Fix stripe webhook timeout", "high"),
    );

    let candidates = [
        ("payment-service", "Handles Stripe payments"),
        ("auth-service", "Handles OAuth logins"),
    ];

    let outcome = coordinator
        .route_envelope_with_candidates(&envelope, &candidates, true)
        .expect("Routing with candidates must succeed");

    assert!(outcome.is_dry_run());
    if let RouteOutcome::DryRun {
        verdict,
        resolution,
    } = outcome
    {
        assert_eq!(
            verdict,
            k0maru_router::policy::PolicyVerdict::TrustedInternal
        );
        assert!(resolution.is_some());
    } else {
        panic!("Expected DryRun outcome");
    }
}

type SpawnedPaneRecord = (PathBuf, Option<String>);

#[derive(Debug, Clone, Default)]
struct MockMuxDriver {
    pub live_sessions: Arc<Mutex<Vec<LiveSession>>>,
    pub sent_prompts: Arc<Mutex<Vec<(String, String)>>>,
    pub spawned_panes: Arc<Mutex<Vec<SpawnedPaneRecord>>>,
}

impl MultiplexerDriver for MockMuxDriver {
    fn name(&self) -> &'static str {
        "mock-mux"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        Ok(self.live_sessions.lock().unwrap().clone())
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        self.sent_prompts
            .lock()
            .unwrap()
            .push((pane_id.to_string(), prompt.to_string()));
        Ok(())
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        let pane_id = format!("%{}", self.spawned_panes.lock().unwrap().len() + 100);
        self.spawned_panes
            .lock()
            .unwrap()
            .push((workspace.to_path_buf(), command.map(String::from)));
        let session = LiveSession::new(
            &pane_id,
            workspace.to_path_buf(),
            SessionStatus::Idle,
            Some("bash".into()),
        );
        self.live_sessions.lock().unwrap().push(session.clone());
        Ok(session)
    }
}

#[derive(Debug, Clone)]
struct MockWorkspaceProvider {
    sandbox_root: PathBuf,
}

#[async_trait]
impl WorkspaceProvider for MockWorkspaceProvider {
    fn name(&self) -> &'static str {
        "mock_workspace"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn provision_sandbox(
        &self,
        task_id: &str,
        _repo_root: &Path,
        _base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError> {
        let sandbox = self.sandbox_root.join(format!("worktree_{task_id}"));
        fs::create_dir_all(&sandbox).map_err(RouterError::Io)?;
        Ok(sandbox)
    }

    async fn teardown_sandbox(&self, task_id: &str, _repo_root: &Path) -> Result<(), RouterError> {
        let sandbox = self.sandbox_root.join(format!("worktree_{task_id}"));
        if sandbox.is_dir() {
            let _ = fs::remove_dir_all(&sandbox);
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_coordinator_closed_loop_isolation_and_receipt_verification() {
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let sandboxes_dir = temp.path().join("sandboxes");
    let receipts_dir = temp.path().join("receipts");
    fs::create_dir_all(&repo_root).unwrap();
    fs::create_dir_all(&sandboxes_dir).unwrap();
    let repo_root = fs::canonicalize(&repo_root).unwrap();
    let sandboxes_dir = fs::canonicalize(&sandboxes_dir).unwrap();

    let driver = MockMuxDriver::default();
    let ws_provider = MockWorkspaceProvider {
        sandbox_root: sandboxes_dir.clone(),
    };

    let coordinator = RouterCoordinator::new(driver.clone())
        .with_workspace_provider(ws_provider)
        .with_receipt_dir(&receipts_dir);

    // Step 1: Route task requesting isolation
    let envelope = TaskEnvelope::new(
        "task_phase4_loop",
        "2026-10-12T00:00:00Z",
        CallerInfo::new("/ws/caller", "worker-1"),
        TargetInfo::new_internal("core", repo_root.to_str().unwrap()),
        "isolate",
        TaskPayload::new("Isolate task execution", "high"),
    );
    let outcome = coordinator
        .route_task(&envelope, &[], false, true)
        .expect("Routing with isolation must succeed");

    assert!(outcome.is_dispatched());
    let sandbox_path = outcome
        .sandbox()
        .expect("Outcome must contain sandbox path");
    assert!(sandbox_path.starts_with(&sandboxes_dir));
    assert!(sandbox_path.is_dir());

    // Verify driver dispatched into the sandbox
    let session = outcome.session().unwrap();
    assert_eq!(session.cwd, sandbox_path);

    // Step 2: Ingest execution receipt generated by downstream worker
    let patch = b"diff --git a/src/lib.rs b/src/lib.rs\n+pub fn verified() {}\n";
    let receipt = create_sample_receipt("task_phase4_loop", sandbox_path, patch);

    let summary = coordinator
        .ingest_receipt_with_patch(receipt.clone(), Some(patch))
        .expect("Receipt ingestion with matching SHA must succeed");

    assert_eq!(summary.task_id, "task_phase4_loop");
    assert!(summary.is_passing);
    assert_eq!(summary.verdict, ReceiptVerdict::Success);

    // Step 3: Verify tampered patch rejection
    let tampered_patch = b"malicious injected code";
    let reject_res = coordinator.ingest_receipt_with_patch(receipt, Some(tampered_patch));
    assert!(matches!(reject_res, Err(RouterError::InvalidReceipt(msg)) if msg.contains("SHA-256")));

    // Step 4: Query receipt from store
    let queried = coordinator
        .get_receipt("task_phase4_loop")
        .unwrap()
        .expect("Receipt must exist in store");
    assert_eq!(queried.task_id, "task_phase4_loop");
    assert!(queried.is_passing());
}
