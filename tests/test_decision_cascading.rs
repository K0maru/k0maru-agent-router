use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use k0maru_router::decision::cascading::CascadingDecisionEngine;
use k0maru_router::decision::rule::{DeterministicRuleEngine, MatchPattern, Rule};
use k0maru_router::decision::{DecisionEngine, DecisionRoutingOutcome};
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;

fn create_test_envelope(title: &str, caller_ws: &str) -> TaskEnvelope {
    TaskEnvelope::new(
        "task_cascade_001",
        "2026-10-12T00:00:00Z",
        CallerInfo::new(caller_ws, "cascade-agent"),
        TargetInfo::new_internal("auto", "/ws/default"),
        "fix_bug",
        TaskPayload::new(title, "high"),
    )
}

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

    let laya_called = Arc::new(AtomicBool::new(false));
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

    let envelope = create_test_envelope("Fix stripe webhook timeout", "/ws/client");
    let candidates = [("billing-service", "Payments"), ("other-service", "Other")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "billing-service");
    assert!((outcome.confidence - 0.98).abs() < f64::EPSILON);
    assert_eq!(outcome.engine_source, "rule");
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);

    // Verify Tier 2 was never invoked due to fast-path short-circuit
    assert!(!laya_called_tracker.load(Ordering::SeqCst));
    assert!(!laya_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cascade_tier1_miss_tier2_laya_wins() {
    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(
        Rule::new("auth-service")
            .with_match_title(MatchPattern::exact("login failure"))
            .with_confidence(0.95),
    );

    let laya_engine = MockEngine::new("laya").with_outcome(Ok(DecisionRoutingOutcome::act(
        "inventory-service",
        0.92,
        "laya",
    )));
    let laya_called = laya_engine.called.clone();

    let cascade = CascadingDecisionEngine::new()
        .with_engine(rule_engine)
        .with_engine(laya_engine);

    let envelope = create_test_envelope("Out of stock exception in warehouse", "/ws/client");
    let candidates = [("inventory-service", "Inventory"), ("auth-service", "Auth")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "inventory-service");
    assert!((outcome.confidence - 0.92).abs() < f64::EPSILON);
    assert_eq!(outcome.engine_source, "laya");
    assert_eq!(outcome.action_gate, "act");
    assert!(!outcome.should_stage_outbox);
    assert!(laya_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_cascade_all_engines_fail_falls_back_to_outbox() {
    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(Rule::new("unmatched").with_match_title(MatchPattern::exact("nomatch")));

    let broken_laya = MockEngine::new("laya").with_outcome(Err(
        RouterError::DriverExecutionFailed("Network down".into()),
    ));

    let cascade = CascadingDecisionEngine::new()
        .with_engine(rule_engine)
        .with_engine(broken_laya);

    let envelope = create_test_envelope("Random mysterious glitch", "/ws/unknown");
    let candidates = [("default-service", "Default")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_cascade_low_confidence_gating() {
    // Both engines return below the 0.85 threshold
    let engine1 = MockEngine::new("rule")
        .with_outcome(Ok(DecisionRoutingOutcome::act("target-a", 0.60, "rule")));
    let engine2 = MockEngine::new("laya")
        .with_outcome(Ok(DecisionRoutingOutcome::act("target-b", 0.78, "laya")));

    let cascade = CascadingDecisionEngine::new()
        .with_engine(engine1)
        .with_engine(engine2)
        .with_threshold(0.85);

    let envelope = create_test_envelope("Ambiguous request", "/ws");
    let candidates = [("target-a", "A"), ("target-b", "B")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    // Keeps target-b because 0.78 > 0.60, but gates it to escalate
    assert_eq!(outcome.target_identifier, "target-b");
    assert!((outcome.confidence - 0.78).abs() < f64::EPSILON);
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}

#[tokio::test]
async fn test_cascade_unhealthy_engine_skipped() {
    let offline_engine = MockEngine::new("offline")
        .with_healthy(false)
        .with_outcome(Ok(DecisionRoutingOutcome::act(
            "should-not-reach",
            0.99,
            "offline",
        )));
    let offline_called = offline_engine.called.clone();

    let online_engine = MockEngine::new("online")
        .with_healthy(true)
        .with_outcome(Ok(DecisionRoutingOutcome::act(
            "online-target",
            0.95,
            "online",
        )));

    let cascade = CascadingDecisionEngine::new()
        .with_engine(offline_engine)
        .with_engine(online_engine);

    let envelope = create_test_envelope("Normal request", "/ws");
    let candidates = [("online-target", "Online")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "online-target");
    assert_eq!(outcome.engine_source, "online");
    assert!(!offline_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_empty_cascade_escalates() {
    let cascade = CascadingDecisionEngine::new();
    assert!(!cascade.is_healthy().await);

    let envelope = create_test_envelope("Empty cascade test", "/ws");
    let candidates = [("service-fallback", "Fallback")];

    let outcome = cascade.evaluate_task(&envelope, &candidates).await.unwrap();
    assert_eq!(outcome.target_identifier, "service-fallback");
    assert_eq!(outcome.action_gate, "escalate");
    assert!(outcome.should_stage_outbox);
}
