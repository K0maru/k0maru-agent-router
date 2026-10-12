use k0maru_router::decision::rule::{
    DeterministicRuleEngine, MatchPattern, NoopDecisionEngine, Rule,
};
use k0maru_router::decision::DecisionEngine;
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};

fn create_test_envelope(caller_ws: &str, target_id: &str, title: &str) -> TaskEnvelope {
    TaskEnvelope::new(
        "task_test_001",
        "2026-10-12T00:00:00Z",
        CallerInfo::new(caller_ws, "test-agent"),
        TargetInfo::new_internal(target_id, "/workspace/default"),
        "fix_bug",
        TaskPayload::new(title, "high").with_expected("Expected test behavior"),
    )
}

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
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_escalate_action_flags_outbox() {
    let mut engine = DeterministicRuleEngine::new();
    engine.add_rule(
        Rule::new("dangerous-service")
            .with_match_title(MatchPattern::regex(r"(?i)delete|purge|drop database"))
            .with_confidence(0.99)
            .with_action("escalate"),
    );

    let envelope = create_test_envelope("/ws/admin", "auto", "Drop database tables in production");
    let candidates = [("dangerous-service", "Danger")];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "dangerous-service");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_empty_and_no_match_fallback() {
    let engine = DeterministicRuleEngine::new();
    assert!(engine.is_healthy().await);

    let envelope = create_test_envelope("/ws/unknown", "auto", "Unmatched title");
    let candidates = [("service-a", "A"), ("service-b", "B")];

    let outcome = engine.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.engine_source, "rule");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_deterministic_rule_engine_json_serde() {
    let json_rules = r#"{
        "rules": [
            {
                "target": "payment-hub",
                "match_workspace": { "type": "prefix", "value": "/workspace/payments" },
                "match_title": { "type": "regex", "value": "invoice|refund" },
                "confidence": 0.98,
                "action": "act"
            }
        ]
    }"#;

    let engine = DeterministicRuleEngine::from_json_str(json_rules).unwrap();
    assert_eq!(engine.rules.len(), 1);

    let envelope = create_test_envelope(
        "/workspace/payments/checkout",
        "auto",
        "Process customer refund request",
    );
    let outcome = engine.evaluate_task(&envelope, &[]).await.unwrap();
    assert_eq!(outcome.target_identifier, "payment-hub");
    assert!((outcome.confidence - 0.98).abs() < f64::EPSILON);
    assert!(!outcome.should_stage_outbox);
}
