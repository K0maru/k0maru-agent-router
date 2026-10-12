use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use k0maru_router::decision::laya::LayaDecisionClient;
use k0maru_router::decision::protocol::{ChoiceCandidate, SystemOneRequest, SystemOneResponse};
use k0maru_router::decision::DecisionEngine;
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;

fn create_test_envelope(title: &str) -> TaskEnvelope {
    TaskEnvelope::new(
        "task_laya_001",
        "2026-10-12T00:00:00Z",
        CallerInfo::new("/ws/caller", "laya-tester"),
        TargetInfo::new_internal("auto", "/ws/target"),
        "fix_bug",
        TaskPayload::new(title, "high").with_expected("Correct behavior"),
    )
}

type RequestHandler = Arc<dyn Fn(&str, &str) -> (u16, String) + Send + Sync>;

/// Lightweight mock HTTP server for testing wire protocol contracts.
async fn run_mock_server(handler: RequestHandler) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };

            let handler_fn = handler.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let request_str = String::from_utf8_lossy(&buffer[..n]);

                // Split method and path
                let first_line = request_str.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                let _method = parts.first().copied().unwrap_or("GET");
                let path = parts.get(1).copied().unwrap_or("/");

                // Extract body after double newline if present
                let body = if let Some(idx) = request_str.find("\r\n\r\n") {
                    &request_str[idx + 4..]
                } else if let Some(idx) = request_str.find("\n\n") {
                    &request_str[idx + 2..]
                } else {
                    ""
                };

                let (status_code, response_body) = handler_fn(path, body);
                let status_text = match status_code {
                    200 => "OK",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Response",
                };

                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status_code,
                    status_text,
                    response_body.len(),
                    response_body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });

    (addr, server_handle)
}

#[test]
fn test_system_one_request_and_response_serde_contract() {
    let candidates = vec![
        ChoiceCandidate::new("repo-billing", "Handles payments and invoices"),
        ChoiceCandidate::new("repo-auth", "Handles authentication and tokens"),
    ];

    let request = SystemOneRequest::choice("Fix stripe checkout crash", candidates);
    assert_eq!(request.primitive, "choice");
    assert_eq!(request.candidates.len(), 2);

    let serialized = serde_json::to_string(&request).unwrap();
    assert!(serialized.contains("\"primitive\":\"choice\""));
    assert!(serialized.contains("repo-billing"));

    let response_json = r#"{
        "selected_id": "repo-billing",
        "confidence": 0.965,
        "action": "act",
        "explanation": "High semantic affinity with payment keywords",
        "raw_probabilities": {
            "repo-billing": 0.965,
            "repo-auth": 0.035
        }
    }"#;

    let response: SystemOneResponse = serde_json::from_str(response_json).unwrap();
    assert_eq!(response.selected_id, "repo-billing");
    assert!((response.confidence - 0.965).abs() < f64::EPSILON);
    assert_eq!(response.action, "act");
    assert_eq!(
        response.explanation.as_deref(),
        Some("High semantic affinity with payment keywords")
    );
    assert_eq!(
        response
            .raw_probabilities
            .as_ref()
            .and_then(|m| m.get("repo-billing")),
        Some(&0.965)
    );
}

#[test]
fn test_native_laya_serve_response_deserialization() {
    let native_laya_json = r#"{
        "model": "laya-rl-agent",
        "answers": {
            "target": {
                "type": "choice",
                "choice": "/workspace/terra-ui",
                "probabilities": {
                    "/workspace/terra-ui": 0.9526,
                    "/workspace/payment-backend": 0.0474
                },
                "confidence": 0.7248,
                "answer_confidence": 0.9526,
                "action": {
                    "act_probability": 1.0
                },
                "x_jev_confidence": 0.9052
            }
        },
        "usage": {
            "input_tokens": 64,
            "output_tokens": 0
        }
    }"#;

    let response: SystemOneResponse = serde_json::from_str(native_laya_json).unwrap();
    assert_eq!(response.selected_id, "/workspace/terra-ui");
    assert!((response.confidence - 0.9526).abs() < 1e-4);
    assert_eq!(response.action, "act");
    assert_eq!(response.model.as_deref(), Some("laya-rl-agent"));
    let probs = response.raw_probabilities.as_ref().unwrap();
    assert!((probs.get("/workspace/terra-ui").unwrap() - 0.9526).abs() < 1e-4);
}

#[tokio::test]
async fn test_laya_client_health_check() {
    let handler = Arc::new(|path: &str, _: &str| -> (u16, String) {
        if path == "/health" || path == "/v1/health" {
            (200, r#"{"status":"healthy"}"#.to_string())
        } else {
            (404, "{}".to_string())
        }
    });

    let (addr, handle) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new().with_base_url(format!("http://{}", addr));

    assert!(client.is_healthy().await);

    // Unreachable client
    let dead_client = LayaDecisionClient::new().with_base_url("http://127.0.0.1:59999");
    assert!(!dead_client.is_healthy().await);

    handle.abort();
}

#[tokio::test]
async fn test_laya_client_high_confidence_act() {
    let handler = Arc::new(|path: &str, body: &str| -> (u16, String) {
        if path == "/v1/systemone" {
            assert!(body.contains("\"primitive\":\"choice\""));
            (
                200,
                r#"{
                    "selected_id": "billing-service",
                    "confidence": 0.94,
                    "action": "act"
                }"#
                .to_string(),
            )
        } else {
            (404, "{}".to_string())
        }
    });

    let (addr, handle) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new()
        .with_base_url(format!("http://{}", addr))
        .with_confidence_threshold(0.85);

    let envelope = create_test_envelope("Stripe webhook failure");
    let candidates = [("billing-service", "Payments"), ("auth-service", "Auth")];

    let outcome = client.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "billing-service");
    assert!((outcome.confidence - 0.94).abs() < f64::EPSILON);
    assert_eq!(outcome.engine_source, "laya");
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);

    handle.abort();
}

#[tokio::test]
async fn test_laya_client_low_confidence_escalates() {
    let handler = Arc::new(|path: &str, _: &str| -> (u16, String) {
        if path == "/v1/systemone" {
            (
                200,
                r#"{
                    "selected_id": "analytics-service",
                    "confidence": 0.72,
                    "action": "act"
                }"#
                .to_string(),
            )
        } else {
            (404, "{}".to_string())
        }
    });

    let (addr, handle) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new()
        .with_base_url(format!("http://{}", addr))
        .with_confidence_threshold(0.85);

    let envelope = create_test_envelope("Ambiguous event metric drop");
    let candidates = [
        ("analytics-service", "Analytics"),
        ("billing-service", "Billing"),
    ];

    let outcome = client.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "analytics-service");
    assert!((outcome.confidence - 0.72).abs() < f64::EPSILON);
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);

    handle.abort();
}

#[tokio::test]
async fn test_laya_client_explicit_escalate_action() {
    let handler = Arc::new(|path: &str, _: &str| -> (u16, String) {
        if path == "/v1/systemone" {
            (
                200,
                r#"{
                    "selected_id": "kernel-service",
                    "confidence": 0.99,
                    "action": "escalate"
                }"#
                .to_string(),
            )
        } else {
            (404, "{}".to_string())
        }
    });

    let (addr, handle) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new().with_base_url(format!("http://{}", addr));

    let envelope = create_test_envelope("Kernel memory corrupt panic");
    let candidates = [("kernel-service", "Core OS kernel")];

    let outcome = client.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "kernel-service");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);

    handle.abort();
}

#[tokio::test]
async fn test_laya_client_server_error_handling() {
    let handler = Arc::new(|path: &str, _: &str| -> (u16, String) {
        if path == "/v1/systemone" {
            (500, "Inference engine out of memory".to_string())
        } else {
            (404, "{}".to_string())
        }
    });

    let (addr, handle) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new().with_base_url(format!("http://{}", addr));

    let envelope = create_test_envelope("Crash test");
    let candidates = [("service-a", "A")];

    let res = client.evaluate_task(&envelope, &candidates).await;
    assert!(res.is_err());
    match res.unwrap_err() {
        RouterError::DriverExecutionFailed(msg) => {
            assert!(msg.contains("returned error status 500"));
        }
        other => panic!("Expected DriverExecutionFailed, got: {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_jev_client_cloud_attribution() {
    let client = LayaDecisionClient::new_jev("https://api.jev.ai");
    assert_eq!(client.name(), "jev");
    assert_eq!(client.base_url(), "https://api.jev.ai");
}
