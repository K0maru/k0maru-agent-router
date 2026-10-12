//! Comprehensive integration tests for k0maru-agent-router MCP stdio server.

use assert_cmd::Command;
use k0maru_router::coordinator::RouterCoordinator;
use k0maru_router::decision::rule::{DeterministicRuleEngine, MatchPattern, Rule};
use k0maru_router::driver::TmuxDriver;
use k0maru_router::mcp::McpServer;
use std::io::Cursor;
use tempfile::tempdir;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("k0maru-router binary must be buildable via cargo_bin")
}

#[test]
fn test_mcp_initialize_handshake() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // 1. initialize request
    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}"#;
    let resp_str = server.handle_line(init_req).expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert!(resp["result"]["capabilities"]["tools"].is_object());
    assert_eq!(resp["result"]["serverInfo"]["name"], "k0maru-agent-router");
    assert!(!resp["result"]["serverInfo"]["version"]
        .as_str()
        .unwrap()
        .is_empty());

    // 2. notifications/initialized handshake notification (must produce no response)
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert!(server.handle_line(notif).is_none());

    // 3. ping
    let ping_req = r#"{"jsonrpc":"2.0","id":"req-ping","method":"ping"}"#;
    let ping_resp_str = server.handle_line(ping_req).expect("must return response");
    let ping_resp: serde_json::Value = serde_json::from_str(&ping_resp_str).unwrap();
    assert_eq!(ping_resp["jsonrpc"], "2.0");
    assert_eq!(ping_resp["id"], "req-ping");
    assert!(ping_resp["result"].is_object());
}

#[test]
fn test_mcp_tools_list_schema_contract() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    let req = r#"{"jsonrpc":"2.0","id":42,"method":"tools/list"}"#;
    let resp_str = server.handle_line(req).expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 42);

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools must be array");
    assert_eq!(tools.len(), 9, "Expected exactly 9 exposed MCP tools");

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

    assert!(tool_names.contains(&"route_task"));
    assert!(tool_names.contains(&"probe_workspace"));
    assert!(tool_names.contains(&"list_outbox"));
    assert!(tool_names.contains(&"approve_issue"));
    assert!(tool_names.contains(&"reject_issue"));
    assert!(tool_names.contains(&"ingest_receipt"));
    assert!(tool_names.contains(&"query_receipt"));
    assert!(tool_names.contains(&"emit_signal"));
    assert!(tool_names.contains(&"orchestrator_status"));

    for tool in tools {
        assert!(tool["description"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(tool["inputSchema"]["properties"].is_object());
    }

    // Verify probe_workspace requires workspace
    let probe_tool = tools
        .iter()
        .find(|t| t["name"] == "probe_workspace")
        .unwrap();
    let required_probe: Vec<&str> = probe_tool["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(required_probe, vec!["workspace"]);

    // Verify approve_issue requires id
    let approve_tool = tools.iter().find(|t| t["name"] == "approve_issue").unwrap();
    let required_approve: Vec<&str> = approve_tool["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(required_approve, vec!["id"]);

    // Verify reject_issue requires id
    let reject_tool = tools.iter().find(|t| t["name"] == "reject_issue").unwrap();
    let required_reject: Vec<&str> = reject_tool["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(required_reject, vec!["id"]);
}

#[test]
fn test_mcp_tools_call_route_task_untrusted_stages_to_outbox() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": {
                    "id": "task_external_mcp_1",
                    "created_at": "2026-10-11T20:00:00Z",
                    "caller": {
                        "workspace": "/tmp/test_workspace",
                        "agent": "gemini-flash"
                    },
                    "target": {
                        "identifier": "upstream-oss-repo",
                        "is_external": true
                    },
                    "intent": "bug_report",
                    "payload": {
                        "title": "Null pointer in JSON serializer",
                        "severity": "critical",
                        "reproduction": {
                            "snippet": "serialize(null)",
                            "error": "NPE at line 42"
                        },
                        "expected": "Graceful None handling"
                    }
                },
                "dry_run": false
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&call_req).unwrap())
        .expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["id"], 100);
    assert_eq!(resp["result"]["isError"], false);

    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let content_json: serde_json::Value = serde_json::from_str(content_text).unwrap();

    assert_eq!(content_json["task_id"], "task_external_mcp_1");
    assert_eq!(content_json["status"], "staged");
    assert!(content_json["outcome"]["Staged"]["issue_path"].is_string());

    // Verify the staged markdown file was written to the outbox directory
    let staged_file = outbox.path().join("issue_task_external_mcp_1.md");
    assert!(staged_file.is_file(), "Staged markdown file must exist");
    let file_content = std::fs::read_to_string(&staged_file).unwrap();
    assert!(file_content.contains("Pending Human Authorization (HITL)"));
    assert!(file_content.contains("Null pointer in JSON serializer"));
    assert!(file_content.contains("upstream-oss-repo"));
}

#[test]
fn test_mcp_tools_call_route_task_internal_dry_run() {
    let outbox = tempdir().unwrap();
    let ws_dir = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 101,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": {
                    "id": "task_internal_dryrun",
                    "created_at": "2026-10-11T20:00:00Z",
                    "caller": {
                        "workspace": "/tmp/test_workspace",
                        "agent": "worker-subagent"
                    },
                    "target": {
                        "identifier": "internal-module",
                        "workspace": ws_dir.path().to_string_lossy(),
                        "is_external": false
                    },
                    "intent": "feature_request",
                    "payload": {
                        "title": "Add telemetry exporter",
                        "severity": "medium"
                    }
                },
                "dry_run": true
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&call_req).unwrap())
        .expect("must return response");
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();

    assert_eq!(resp["id"], 101);
    assert_eq!(resp["result"]["isError"], false);

    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let content_json: serde_json::Value = serde_json::from_str(content_text).unwrap();

    assert_eq!(content_json["task_id"], "task_internal_dryrun");
    assert_eq!(content_json["status"], "dry_run");
    assert_eq!(
        content_json["outcome"]["DryRun"]["verdict"],
        "TrustedInternal"
    );
}

#[test]
fn test_mcp_tools_call_route_task_flat_and_string_arguments() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // Test flat argument format
    let flat_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 102,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "id": "task_flat_format",
                "caller": {
                    "workspace": "/tmp/flat_ws",
                    "agent": "flat-agent"
                },
                "target": {
                    "identifier": "flat-external",
                    "is_external": true
                },
                "intent": "bug_report",
                "payload": {
                    "title": "Flat arguments test",
                    "severity": "low"
                },
                "dry_run": false
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&flat_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let staged_file = outbox.path().join("issue_task_flat_format.md");
    assert!(staged_file.is_file());

    // Test JSON string envelope format
    let string_envelope = serde_json::json!({
        "id": "task_string_format",
        "caller": { "workspace": "/tmp/str_ws", "agent": "str-agent" },
        "target": { "identifier": "str-external", "is_external": true },
        "intent": "bug_report",
        "payload": { "title": "String envelope test", "severity": "normal" }
    })
    .to_string();

    let str_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 103,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": string_envelope,
                "dry_run": false
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&str_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let staged_file2 = outbox.path().join("issue_task_string_format.md");
    assert!(staged_file2.is_file());
}

#[test]
fn test_mcp_tools_call_route_task_validation_errors() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // Invalid severity
    let bad_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 104,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "envelope": {
                    "id": "task_bad_sev",
                    "caller": { "workspace": "/tmp/ws", "agent": "agent" },
                    "target": { "identifier": "t", "is_external": true },
                    "intent": "fix",
                    "payload": { "title": "bad", "severity": "ultra_fatal" }
                }
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&bad_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);
    let err_msg = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(err_msg.contains("Invalid severity 'ultra_fatal'"));

    // Missing envelope
    let empty_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 105,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {}
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&empty_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn test_mcp_tools_call_probe_workspace() {
    let outbox = tempdir().unwrap();
    let existing_dir = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // Valid workspace
    let probe_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 200,
        "method": "tools/call",
        "params": {
            "name": "probe_workspace",
            "arguments": {
                "workspace": existing_dir.path().to_string_lossy()
            }
        }
    });

    let resp_str = server
        .handle_line(&serde_json::to_string(&probe_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("resolution"));

    // Nonexistent workspace
    let bad_probe_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 201,
        "method": "tools/call",
        "params": {
            "name": "probe_workspace",
            "arguments": {
                "workspace": "/nonexistent/directory/that/never/exists/12345"
            }
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&bad_probe_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);

    // Missing workspace parameter
    let missing_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 202,
        "method": "tools/call",
        "params": {
            "name": "probe_workspace",
            "arguments": {}
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&missing_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn test_mcp_outbox_lifecycle_list_approve_reject() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // 1. Initially empty list
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 300,
        "method": "tools/call",
        "params": {
            "name": "list_outbox",
            "arguments": {}
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&list_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let list_json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(list_json["total"], 0);

    // 2. Stage two external issues
    let stage1 = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 301,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "id": "task_lifecycle_approve",
                "caller": { "workspace": "/tmp/ws", "agent": "a" },
                "target": { "identifier": "upstream-lib1", "is_external": true },
                "intent": "bug_report",
                "payload": { "title": "Issue to approve", "severity": "high" }
            }
        }
    });
    server
        .handle_line(&serde_json::to_string(&stage1).unwrap())
        .unwrap();

    let stage2 = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 302,
        "method": "tools/call",
        "params": {
            "name": "route_task",
            "arguments": {
                "id": "task_lifecycle_reject",
                "caller": { "workspace": "/tmp/ws", "agent": "a" },
                "target": { "identifier": "upstream-lib2", "is_external": true },
                "intent": "bug_report",
                "payload": { "title": "Issue to reject", "severity": "medium" }
            }
        }
    });
    server
        .handle_line(&serde_json::to_string(&stage2).unwrap())
        .unwrap();

    // 3. List outbox should now report 2 issues
    let resp_str = server
        .handle_line(&serde_json::to_string(&list_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let list_json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(list_json["total"], 2);

    // 4. Approve task_lifecycle_approve
    let approve_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 303,
        "method": "tools/call",
        "params": {
            "name": "approve_issue",
            "arguments": {
                "id": "task_lifecycle_approve"
            }
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&approve_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Approved"));

    // Verify file on disk updated
    let path = outbox.path().join("issue_task_lifecycle_approve.md");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("Approved by Human"));

    // 5. Reject task_lifecycle_reject
    let reject_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 304,
        "method": "tools/call",
        "params": {
            "name": "reject_issue",
            "arguments": {
                "id": "task_lifecycle_reject"
            }
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&reject_req).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], false);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Rejected"));

    // Verify file on disk updated
    let path2 = outbox.path().join("issue_task_lifecycle_reject.md");
    let content2 = std::fs::read_to_string(path2).unwrap();
    assert!(content2.contains("Rejected by Human"));

    // 6. Nonexistent ID handling for approve and reject
    let nonexist_approve = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 305,
        "method": "tools/call",
        "params": {
            "name": "approve_issue",
            "arguments": { "id": "nonexistent_task_9999" }
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&nonexist_approve).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);

    let nonexist_reject = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 306,
        "method": "tools/call",
        "params": {
            "name": "reject_issue",
            "arguments": { "id": "nonexistent_task_9999" }
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&nonexist_reject).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn test_mcp_unknown_tool_and_protocol_errors() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // Unknown tool
    let unknown_tool = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 400,
        "method": "tools/call",
        "params": {
            "name": "totally_unknown_tool",
            "arguments": {}
        }
    });
    let resp_str = server
        .handle_line(&serde_json::to_string(&unknown_tool).unwrap())
        .unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["result"]["isError"], true);
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Unknown tool"));

    // Unknown JSON-RPC method (-32601)
    let unknown_method = r#"{"jsonrpc":"2.0","id":401,"method":"magic/teleport"}"#;
    let resp_str = server.handle_line(unknown_method).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["error"]["code"], -32601);

    // Invalid JSON syntax (-32700)
    let malformed_json = r#"{"jsonrpc":"2.0","id":402,"broken"#;
    let resp_str = server.handle_line(malformed_json).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["error"]["code"], -32700);

    // Invalid Request object (-32600)
    let invalid_req = r#"{"id":403,"not_jsonrpc":true}"#;
    let resp_str = server.handle_line(invalid_req).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp["error"]["code"], -32600);
    assert_eq!(resp["id"], 403);
}

#[test]
fn test_mcp_streaming_stdio_loop_and_batch_requests() {
    let outbox = tempdir().unwrap();
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(outbox.path());
    let server = McpServer::new(coordinator);

    // Multi-line streaming over BufRead and Write
    let input = format!(
        "{}\n{}\n{}\n",
        r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
    );

    let reader = Cursor::new(input.into_bytes());
    let mut output = Vec::new();

    server.run(reader, &mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = output_str.trim().lines().collect();

    // 2 responses (ping and tools/list; notifications/initialized produces no response)
    assert_eq!(lines.len(), 2);
    let ping_resp: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(ping_resp["id"], 1);
    let list_resp: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(list_resp["id"], 2);

    // Batch request handling
    let batch =
        r#"[{"jsonrpc":"2.0","id":10,"method":"ping"},{"jsonrpc":"2.0","id":11,"method":"ping"}]"#;
    let batch_resp_str = server.handle_line(batch).unwrap();
    let batch_resp: serde_json::Value = serde_json::from_str(&batch_resp_str).unwrap();
    let batch_arr = batch_resp.as_array().unwrap();
    assert_eq!(batch_arr.len(), 2);
    assert_eq!(batch_arr[0]["id"], 10);
    assert_eq!(batch_arr[1]["id"], 11);
}

#[test]
fn test_mcp_cli_binary_stdio_interaction() {
    let outbox = tempdir().unwrap();

    // Test --help output contains mcp subcommand
    router_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("mcp"));

    // Test mcp --help contains driver and outbox-dir
    router_cmd()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--driver"))
        .stdout(predicates::str::contains("--outbox-dir"));

    // Pipe JSON-RPC request to stdin of k0maru-router mcp
    let ping_request = "{\"jsonrpc\":\"2.0\",\"id\":999,\"method\":\"ping\"}\n";
    router_cmd()
        .args(["mcp", "--outbox-dir", outbox.path().to_str().unwrap()])
        .write_stdin(ping_request)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            r#"{"jsonrpc":"2.0","id":999,"result":{}}"#,
        ));
}

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
