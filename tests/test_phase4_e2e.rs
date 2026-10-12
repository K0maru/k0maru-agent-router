//! End-to-End Regression and Verification Suite for Phase 4:
//! Harness Synergy, Execution Receipts & Workspace Loop.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use assert_cmd::Command;
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use k0maru_router::coordinator::RouterCoordinator;
use k0maru_router::driver::{LiveSession, MultiplexerDriver, SessionStatus};
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;
use k0maru_router::mcp::{JsonRpcRequest, McpServer};
use k0maru_router::receipt::{
    CompilationCheck, ExecutionChecks, ExecutionReceipt, LinterCheck, ReceiptArtifacts,
    ReceiptVerdict, ScopeGuardCheck, TestCheck, DEFAULT_RECEIPT_SCHEMA,
};
use k0maru_router::workspace::WorkspaceProvider;

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

fn create_sample_envelope(task_id: &str, repo_root: &Path, intent: &str) -> TaskEnvelope {
    TaskEnvelope {
        schema: Some(k0maru_router::DEFAULT_SCHEMA.to_string()),
        id: task_id.to_string(),
        created_at: "2026-10-12T14:00:00Z".to_string(),
        caller: CallerInfo {
            workspace: repo_root.display().to_string(),
            agent: "test_master_agent".to_string(),
            pane_id: Some("%1".to_string()),
        },
        target: TargetInfo {
            identifier: "core_service".to_string(),
            workspace: Some(repo_root.display().to_string()),
            is_external: false,
        },
        intent: intent.to_string(),
        payload: TaskPayload {
            title: "Implement Closed-Loop Receipt Verification".to_string(),
            severity: "high".to_string(),
            reproduction: None,
            expected: Some("Clean execution with receipt".to_string()),
        },
    }
}

fn create_sample_receipt(task_id: &str, workspace: &Path, patch_bytes: &[u8]) -> ExecutionReceipt {
    let mut hasher = Sha256::new();
    hasher.update(patch_bytes);
    let patch_sha = hex::encode(hasher.finalize());

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
                total: 42,
                failed: 0,
                duration_ms: Some(150),
            },
            linter: LinterCheck {
                status: "passed".to_string(),
                warnings: 0,
            },
            scope_guard: ScopeGuardCheck {
                status: "passed".to_string(),
                modified_files: vec!["src/lib.rs".to_string()],
            },
        },
        artifacts: ReceiptArtifacts {
            git_patch_sha256: Some(patch_sha),
            receipt_timestamp: Some("2026-10-12T14:30:00Z".to_string()),
        },
    }
}

#[tokio::test]
async fn test_phase4_closed_loop_isolation_and_receipt_verification() {
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
    let envelope = create_sample_envelope("task_phase4_loop", &repo_root, "isolate");
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

#[test]
fn test_phase4_cli_receipt_subcommands() {
    let temp = tempdir().unwrap();
    let receipts_dir = temp.path().join("receipts");
    let receipt_file = temp.path().join("demo_receipt.json");
    let patch_file = temp.path().join("diff.patch");

    let patch_bytes = b"index a1b2c3d..e4f5g6h\n+fn new_feature() {}";
    fs::write(&patch_file, patch_bytes).unwrap();

    let receipt = create_sample_receipt(
        "task_cli_test_99",
        &PathBuf::from("/workspaces/demo"),
        patch_bytes,
    );
    let receipt_json = serde_json::to_string_pretty(&receipt).unwrap();
    fs::write(&receipt_file, receipt_json).unwrap();

    // 1. Ingest receipt via CLI with patch verification
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args([
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

    // 2. Query receipt status via CLI
    let mut cmd_status = Command::cargo_bin("k0maru-router").unwrap();
    let assert_status = cmd_status
        .args([
            "receipt",
            "status",
            "task_cli_test_99",
            "--receipt-dir",
            receipts_dir.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .code(0);

    let stdout_status = String::from_utf8(assert_status.get_output().stdout.clone()).unwrap();
    let val_status: serde_json::Value = serde_json::from_str(&stdout_status).unwrap();
    assert_eq!(val_status["task_id"], "task_cli_test_99");
    assert_eq!(val_status["verdict"], "success");

    // 3. List receipts via CLI
    let mut cmd_list = Command::cargo_bin("k0maru-router").unwrap();
    let assert_list = cmd_list
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
    assert_eq!(list_val[0]["task_id"], "task_cli_test_99");
}

#[tokio::test]
async fn test_phase4_mcp_receipt_tools() {
    let temp = tempdir().unwrap();
    let receipts_dir = temp.path().join("mcp_receipts");

    let driver = MockMuxDriver::default();
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&receipts_dir);
    let server = McpServer::new(coordinator);

    // 1. Verify tools/list contains ingest_receipt and query_receipt
    let list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(1)),
        method: "tools/list".to_string(),
        params: None,
    };
    let list_resp = server.handle_request(list_req).expect("Response required");
    let result_tools = list_resp.result.unwrap();
    let tools = result_tools["tools"].as_array().unwrap();
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"ingest_receipt"));
    assert!(tool_names.contains(&"query_receipt"));
    assert!(tool_names.contains(&"route_task"));

    // 2. Call ingest_receipt tool
    let patch_bytes = b"+test_patch";
    let receipt = create_sample_receipt(
        "task_mcp_receipt_01",
        &PathBuf::from("/ws/mcp"),
        patch_bytes,
    );
    let ingest_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(2)),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "ingest_receipt",
            "arguments": {
                "receipt": receipt,
                "receipt_dir": receipts_dir.to_str().unwrap(),
                "patch": "+test_patch"
            }
        })),
    };
    let ingest_resp = server.handle_request(ingest_req).unwrap();
    assert!(ingest_resp.error.is_none());
    let ingest_result = ingest_resp.result.unwrap();
    let is_error = ingest_result["isError"].as_bool().unwrap();
    assert!(!is_error, "ingest_receipt must succeed");

    // 3. Call query_receipt tool
    let query_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(3)),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "query_receipt",
            "arguments": {
                "task_id": "task_mcp_receipt_01",
                "receipt_dir": receipts_dir.to_str().unwrap()
            }
        })),
    };
    let query_resp = server.handle_request(query_req).unwrap();
    assert!(query_resp.error.is_none());
    let query_result = query_resp.result.unwrap();
    assert!(!query_result["isError"].as_bool().unwrap());
    let content_text = query_result["content"][0]["text"].as_str().unwrap();
    let receipt_data: serde_json::Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(receipt_data["task_id"], "task_mcp_receipt_01");
    assert_eq!(receipt_data["found"], true);
}
