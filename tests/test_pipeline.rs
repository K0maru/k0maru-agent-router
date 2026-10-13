//! Tests for Optional Decoupled E2E Task Pipeline and Component Discovery.

use assert_cmd::Command;
use k0maru_router::pipeline::{execute_pipeline, probe_harness_components, PipelineConfig};
use tempfile::tempdir;

#[test]
fn test_probe_harness_components_structure() {
    let report = probe_harness_components();
    assert_eq!(report.components.len(), 4);

    let names: Vec<&str> = report.components.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"k0maru-router"));
    assert!(names.contains(&"k0maru-verifier"));
    assert!(names.contains(&"k0maru-workspace"));
    assert!(names.contains(&"k0maru-memory"));

    assert!(!report.receipts_dir.is_empty());
    assert!(!report.signals_dir.is_empty());
}

#[test]
fn test_pipeline_config_defaults() {
    let config = PipelineConfig::default();
    assert!(config.task_id.starts_with("task_pipeline_"));
    assert_eq!(config.target_workspace.to_string_lossy(), ".");
    assert!(!config.isolate);
    assert!(config.verify);
    assert!(config.emit_signal);
    assert!(!config.auto_merge);
}

#[tokio::test]
async fn test_execute_pipeline_in_place_without_patch() {
    let tmp = tempdir().unwrap();
    // Initialize a dummy git repo so git commands don't fail
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    std::fs::write(
        tmp.path().join("Cargo.toml"),
        r#"[package]
name = "dummy_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        r#"pub fn add(a: i32, b: i32) -> i32 { a + b }
#[test]
fn test_add() {
    assert_eq!(add(2, 2), 4);
}"#,
    )
    .unwrap();

    std::process::Command::new("git")
        .args(["config", "user.email", "test@k0maru.dev"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Tester"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let config = PipelineConfig {
        task_id: "test_pipeline_e2e_01".to_string(),
        target_workspace: tmp.path().to_path_buf(),
        intent: Some("unit_test".to_string()),
        patch: None,
        runner: None,
        isolate: false,
        verify: true,
        auto_merge: false,
        emit_signal: false,
        crystallize_memory: false,
        allowed_files: vec![],
        node: None,
        remote_backend: None,
    };

    let report = execute_pipeline(&config).await.unwrap();
    assert_eq!(report.task_id, "test_pipeline_e2e_01");
    assert_eq!(report.verdict, "success");
}

#[test]
fn test_cli_pipeline_check() {
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args(["pipeline", "check"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "K0maru-Harness Component Readiness",
        ))
        .stdout(predicates::str::contains("k0maru-router"));
}

#[test]
fn test_cli_pipeline_check_json() {
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args(["pipeline", "check", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"components\":"))
        .stdout(predicates::str::contains("\"receipts_dir\":"));
}

#[test]
fn test_cli_e2e_alias() {
    let mut cmd = Command::cargo_bin("k0maru-router").unwrap();
    cmd.args(["e2e", "check"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "K0maru-Harness Component Readiness",
        ));
}

#[test]
fn test_mcp_check_pipeline_health_tool() {
    use k0maru_router::coordinator::RouterCoordinator;
    use k0maru_router::driver::TmuxDriver;
    use k0maru_router::mcp::{JsonRpcRequest, McpServer};

    let coordinator = RouterCoordinator::new(TmuxDriver::new());
    let server = McpServer::new(coordinator);

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "check_pipeline_health",
            "arguments": {}
        })),
    };

    let resp = server.handle_request(req).expect("Response expected");
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let content = result["content"][0]["text"].as_str().unwrap();
    assert!(content.contains("\"components\""));
}
