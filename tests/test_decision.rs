//! Consolidated Decision Engine Integration Tests for k0maru-agent-router.
//! Covers:
//! - Deterministic Rule Engine (exact, prefix, regex, json load, action gate)
//! - Laya / System-1 Wire Protocol Client (SystemOne request/response, mock HTTP server, confidence gating)
//! - Cascading Decision Engine (multi-tier fallback, unhealthy engine skip, outbox fallback)

use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use k0maru_router::decision::cascading::CascadingDecisionEngine;
use k0maru_router::decision::laya::LayaDecisionClient;
use k0maru_router::decision::protocol::{ChoiceCandidate, SystemOneRequest, SystemOneResponse};
use k0maru_router::decision::rule::{
    DeterministicRuleEngine, MatchPattern, NoopDecisionEngine, Rule,
};
use k0maru_router::decision::{DecisionEngine, DecisionRoutingOutcome};
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;

fn create_test_envelope(caller_ws: &str, target_id: &str, title: &str) -> TaskEnvelope {
    TaskEnvelope::new(
        "task_decision_test",
        "2026-10-12T00:00:00Z",
        CallerInfo::new(caller_ws, "test-agent"),
        TargetInfo::new_internal(target_id, "/workspace/default"),
        "fix_bug",
        TaskPayload::new(title, "high").with_expected("Expected test behavior"),
    )
}

// =========================================================================
// Section 1: Deterministic Rule Engine
// =========================================================================

#[tokio::test]
async fn test_noop_decision_engine_lifecycle() {
    let engine = NoopDecisionEngine;
    assert_eq!(engine.name(), "noop");
    assert!(engine.is_healthy().await);

    let envelope = create_test_envelope("/ws/billing", "auto", "Fix checkout timeout");
    let candidates = [
        ("billing-service", "Handles payments"),
        ("auth-service", "Handles login"),
    ];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.engine_source, "noop");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_exact_match() {
    let mut engine = DeterministicRuleEngine::new();
    engine.add_rule(
        Rule::new("billing-service")
            .with_match_title(MatchPattern::exact("Stripe checkout error"))
            .with_confidence(0.99)
            .with_action("act"),
    );

    let envelope = create_test_envelope("/ws/unknown", "auto", "Stripe checkout error");
    let candidates = [
        ("billing-service", "Payments"),
        ("inventory-service", "Inventory"),
    ];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "billing-service");
    assert!((outcome.confidence - 0.99).abs() < f64::EPSILON);
    assert_eq!(outcome.engine_source, "rule");
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_prefix_match() {
    let mut engine = DeterministicRuleEngine::new();
    engine.add_rule(
        Rule::new("infra-service")
            .with_match_workspace(MatchPattern::prefix("/Users/k0maru3/workspace/infra"))
            .with_confidence(0.95)
            .with_action("act"),
    );

    let envelope = create_test_envelope(
        "/Users/k0maru3/workspace/infra/k8s-configs",
        "auto",
        "Deploy manifest syntax error",
    );
    let candidates = [("infra-service", "Infra"), ("app-service", "Apps")];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "infra-service");
    assert!((outcome.confidence - 0.95).abs() < f64::EPSILON);
    assert!(!outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_regex_match() {
    let mut engine = DeterministicRuleEngine::new();
    engine.add_rule(
        Rule::new("auth-service")
            .with_match_title(MatchPattern::regex(r"(?i)jwt|oauth|authentication"))
            .with_confidence(0.92)
            .with_action("act"),
    );

    let envelope = create_test_envelope("/ws/client", "auto", "OAuth token expired unexpectedly");
    let candidates = [
        ("auth-service", "Authentication"),
        ("billing-service", "Billing"),
    ];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "auth-service");
    assert!(!outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_escalate_action_flags_outbox() {
    let mut engine = DeterministicRuleEngine::new();
    engine.add_rule(
        Rule::new("fraud-detection")
            .with_match_title(MatchPattern::regex(r"(?i)suspicious"))
            .with_confidence(0.75)
            .with_action("escalate"),
    );

    let envelope = create_test_envelope("/ws/client", "auto", "Suspicious transaction detected");
    let candidates = [("fraud-detection", "Fraud"), ("general", "General")];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "fraud-detection");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_json_serde() {
    let json_rules = r#"{
        "rules": [
            {
                "target": "billing-service",
                "match_title": {
                    "type": "regex",
                    "value": "(?i)payment|billing"
                },
                "confidence": 0.98,
                "action": "act"
            }
        ]
    }"#;

    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), json_rules).unwrap();

    let engine = DeterministicRuleEngine::from_file(temp_file.path()).unwrap();
    let envelope = create_test_envelope("/ws/client", "auto", "Payment webhook timeout");
    let candidates = [("billing-service", "Billing"), ("auth", "Auth")];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "billing-service");
    assert!(!outcome.should_stage_outbox);
}

// =========================================================================
// Section 2: Laya / System-1 Wire Protocol Client
// =========================================================================

type RequestHandler = Arc<dyn Fn(&str, &str) -> (u16, String) + Send + Sync>;

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

                let first_line = request_str.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                let path = parts.get(1).copied().unwrap_or("/");

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
        "rationale": "High semantic similarity"
    }"#;

    let response: SystemOneResponse = serde_json::from_str(response_json).unwrap();
    assert_eq!(response.selected_id, "repo-billing");
    assert!((response.confidence - 0.965).abs() < f64::EPSILON);
    assert_eq!(response.action, "act");
}

#[tokio::test]
async fn test_laya_client_high_confidence_act() {
    let handler: RequestHandler = Arc::new(|path, body| {
        assert_eq!(path, "/v1/systemone");
        assert!(body.contains("Stripe payment webhook HMAC mismatch"));
        (
            200,
            r#"{
                "selected_id": "payment-service",
                "confidence": 0.96,
                "action": "act",
                "rationale": "Matched Stripe webhook signature issue"
            }"#
            .to_string(),
        )
    });

    let (addr, _server) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new()
        .with_base_url(format!("http://{}", addr))
        .with_confidence_threshold(0.85);

    let envelope =
        create_test_envelope("/ws/client", "auto", "Stripe payment webhook HMAC mismatch");
    let candidates = [("payment-service", "Payments"), ("auth-service", "Auth")];

    let outcome = client.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "payment-service");
    assert!((outcome.confidence - 0.96).abs() < f64::EPSILON);
    assert_eq!(outcome.engine_source, "laya");
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_laya_client_low_confidence_escalates() {
    let handler: RequestHandler = Arc::new(|_path, _body| {
        (
            200,
            r#"{
                "selected_id": "auth-service",
                "confidence": 0.72,
                "action": "act",
                "rationale": "Ambiguous defect description"
            }"#
            .to_string(),
        )
    });

    let (addr, _server) = run_mock_server(handler).await;
    let client = LayaDecisionClient::new()
        .with_base_url(format!("http://{}", addr))
        .with_confidence_threshold(0.85);

    let envelope = create_test_envelope("/ws/client", "auto", "Unclear miscellaneous chore");
    let candidates = [("payment-service", "Payments"), ("auth-service", "Auth")];

    let outcome = client.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

// =========================================================================
// Section 3: Cascading Decision Engine
// =========================================================================

#[derive(Debug)]
struct MockEngine {
    name: &'static str,
    healthy: bool,
    outcome: Option<Result<DecisionRoutingOutcome, RouterError>>,
    called: Arc<AtomicBool>,
}

impl MockEngine {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            healthy: true,
            outcome: None,
            called: Arc::new(AtomicBool::new(false)),
        }
    }

    fn with_healthy(mut self, healthy: bool) -> Self {
        self.healthy = healthy;
        self
    }

    fn with_outcome(mut self, outcome: Result<DecisionRoutingOutcome, RouterError>) -> Self {
        self.outcome = Some(outcome);
        self
    }
}

#[async_trait]
impl DecisionEngine for MockEngine {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn is_healthy(&self) -> bool {
        self.healthy
    }

    async fn evaluate_task(
        &self,
        _envelope: &TaskEnvelope,
        _candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        self.called.store(true, Ordering::SeqCst);
        match &self.outcome {
            Some(Ok(o)) => Ok(o.clone()),
            Some(Err(_)) => Err(RouterError::DriverExecutionFailed("Simulated error".into())),
            None => Ok(DecisionRoutingOutcome::escalate("", 0.0, self.name)),
        }
    }
}

#[tokio::test]
async fn test_cascade_tier1_rule_engine_wins() {
    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(
        Rule::new("billing-service")
            .with_match_title(MatchPattern::regex(r"(?i)checkout|stripe"))
            .with_confidence(0.98)
            .with_action("act"),
    );

    let laya_engine = MockEngine::new("laya").with_outcome(Ok(DecisionRoutingOutcome::act(
        "other-service",
        0.99,
        "laya",
    )));
    let laya_called_tracker = laya_engine.called.clone();

    let cascade = CascadingDecisionEngine::new()
        .with_engine(rule_engine)
        .with_engine(laya_engine);

    assert!(cascade.is_healthy().await);

    let envelope = create_test_envelope("/ws/client", "auto", "Fix stripe webhook timeout");
    let candidates = [("billing-service", "Payments"), ("other-service", "Other")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "billing-service");
    assert_eq!(outcome.engine_source, "rule");
    assert!(!outcome.should_stage_outbox);
    assert!(!laya_called_tracker.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cascade_tier1_miss_tier2_laya_wins() {
    let rule_engine = DeterministicRuleEngine::new(); // empty, will miss

    let laya_engine = MockEngine::new("laya").with_outcome(Ok(DecisionRoutingOutcome::act(
        "payment-service",
        0.94,
        "laya",
    )));
    let laya_called_tracker = laya_engine.called.clone();

    let cascade = CascadingDecisionEngine::new()
        .with_engine(rule_engine)
        .with_engine(laya_engine);

    let envelope = create_test_envelope("/ws/client", "auto", "Novel defect without rules");
    let candidates = [("payment-service", "Payments")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "payment-service");
    assert_eq!(outcome.engine_source, "laya");
    assert!(!outcome.should_stage_outbox);
    assert!(laya_called_tracker.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cascade_all_engines_fail_falls_back_to_outbox() {
    let rule_engine = DeterministicRuleEngine::new();
    let laya_engine = MockEngine::new("laya").with_outcome(Err(
        RouterError::DriverExecutionFailed("laya server down".into()),
    ));

    let cascade = CascadingDecisionEngine::new()
        .with_engine(rule_engine)
        .with_engine(laya_engine);

    let envelope = create_test_envelope("/ws/client", "auto", "Ambiguous defect");
    let candidates = [("service-a", "A"), ("service-b", "B")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_cascade_unhealthy_engine_skipped() {
    let unhealthy_engine = MockEngine::new("unhealthy").with_healthy(false);
    let healthy_engine = MockEngine::new("healthy").with_outcome(Ok(DecisionRoutingOutcome::act(
        "target-service",
        0.95,
        "healthy",
    )));

    let cascade = CascadingDecisionEngine::new()
        .with_engine(unhealthy_engine)
        .with_engine(healthy_engine);

    let envelope = create_test_envelope("/ws/client", "auto", "Some task");
    let candidates = [("target-service", "Target")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "target-service");
    assert_eq!(outcome.engine_source, "healthy");
}
