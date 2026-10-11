use tempfile::tempdir;

use k0maru_router::coordinator::RouterCoordinator;
use k0maru_router::decision::rule::{DeterministicRuleEngine, MatchPattern, Rule};
use k0maru_router::driver::TmuxDriver;
use k0maru_router::mcp::McpServer;

#[test]
fn test_mcp_tools_call_route_task_with_candidates_and_rule_engine() {
    let outbox = tempdir().unwrap();
    let ws_dir = tempdir().unwrap();

    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(
        Rule::new("payment-service")
            .with_match_title(MatchPattern::regex(r"(?i)checkout|invoice"))
            .with_confidence(0.99)
            .with_action("act"),
    );

    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver)
        .with_outbox_dir(outbox.path())
        .with_decision_engine(rule_engine);
    let server = McpServer::new(coordinator);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 201,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": {
                    "id": "task_mcp_candidate_01",
                    "created_at": "2026-10-12T00:00:00Z",
                    "caller": {
                        "workspace": "/tmp/test_workspace",
                        "agent": "worker-mcp"
                    },
                    "target": {
                        "identifier": "auto",
                        "workspace": ws_dir.path().to_string_lossy(),
                        "is_external": false
                    },
                    "intent": "bug_report",
                    "payload": {
                        "title": "Invoice calculation rounding error",
                        "severity": "high"
                    }
                },
                "candidates": [
                    { "identifier": "payment-service", "description": "Payment and invoicing" },
                    { "identifier": "auth-service", "description": "User authentication" }
                ],
                "dry_run": true
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&call_req).unwrap())
        .expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["id"], 201);
    assert_eq!(resp["result"]["isError"], false);

    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let content_json: serde_json::Value = serde_json::from_str(content_text).unwrap();

    assert_eq!(content_json["task_id"], "task_mcp_candidate_01");
    assert_eq!(content_json["status"], "dry_run");
    assert_eq!(
        content_json["outcome"]["DryRun"]["verdict"],
        "TrustedInternal"
    );
}

#[test]
fn test_mcp_tools_call_route_task_unmatched_candidates_stages_outbox() {
    let outbox = tempdir().unwrap();
    let ws_dir = tempdir().unwrap();

    let rule_engine = DeterministicRuleEngine::new();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver)
        .with_outbox_dir(outbox.path())
        .with_decision_engine(rule_engine);
    let server = McpServer::new(coordinator);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 202,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": {
                    "id": "task_mcp_candidate_unmatched",
                    "created_at": "2026-10-12T00:00:00Z",
                    "caller": {
                        "workspace": "/tmp/test_workspace",
                        "agent": "worker-mcp"
                    },
                    "target": {
                        "identifier": "auto",
                        "workspace": ws_dir.path().to_string_lossy(),
                        "is_external": false
                    },
                    "intent": "bug_report",
                    "payload": {
                        "title": "Mysterious anomaly",
                        "severity": "medium"
                    }
                },
                "candidates": [
                    { "identifier": "candidate-a", "description": "Candidate A" },
                    { "identifier": "candidate-b", "description": "Candidate B" }
                ],
                "dry_run": false
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&call_req).unwrap())
        .expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["id"], 202);
    assert_eq!(resp["result"]["isError"], false);

    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let content_json: serde_json::Value = serde_json::from_str(content_text).unwrap();

    assert_eq!(content_json["status"], "staged");
    let staged_file = outbox.path().join("issue_task_mcp_candidate_unmatched.md");
    assert!(staged_file.is_file());
}
