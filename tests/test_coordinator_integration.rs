use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

use k0maru_router::coordinator::{
    format_agent_prompt_with_context, RouteOutcome, RouterCoordinator,
};
use k0maru_router::decision::rule::{DeterministicRuleEngine, MatchPattern, Rule};
use k0maru_router::driver::TmuxDriver;
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;
use k0maru_router::memory::MemoryHook;

fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router").unwrap()
}

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
fn test_coordinator_memory_hook_enrichment_in_prompt() {
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
fn test_coordinator_route_envelope_with_rule_engine_resolution() {
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

    // Envelope with ambiguous target "auto", but specifying target workspace
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
        .unwrap();

    // In dry-run mode, trusted internal target gives DryRun with TrustedInternal
    assert!(outcome.is_dry_run());
    if let RouteOutcome::DryRun {
        verdict,
        resolution,
    } = outcome
    {
        assert_eq!(verdict, k0maru_router::PolicyVerdict::TrustedInternal);
        assert!(resolution.is_some());
    } else {
        panic!("Expected DryRun outcome");
    }
}

#[test]
fn test_coordinator_route_envelope_low_confidence_escalates_to_outbox() {
    let outbox = tempdir().unwrap();
    let target_dir = tempdir().unwrap();

    let mut rule_engine = DeterministicRuleEngine::new();
    rule_engine.add_rule(
        Rule::new("untrusted-service")
            .with_match_title(MatchPattern::exact("Vague defect"))
            .with_confidence(0.60) // low confidence (< 0.85) triggers escalation
            .with_action("act"),
    );

    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver)
        .with_outbox_dir(outbox.path())
        .with_decision_engine(rule_engine);

    let envelope = TaskEnvelope::new(
        "task_vague_01",
        "2026-10-12T00:00:00Z",
        CallerInfo::new("/ws/caller", "test-agent"),
        TargetInfo::new_internal("auto", target_dir.path().to_str().unwrap()),
        "fix_bug",
        TaskPayload::new("Vague defect", "low"),
    );

    let candidates = [("untrusted-service", "Something unknown")];

    let outcome = coordinator
        .route_envelope_with_candidates(&envelope, &candidates, false)
        .unwrap();

    assert!(outcome.is_staged());
    let staged_file = outbox.path().join("issue_task_vague_01.md");
    assert!(staged_file.is_file());
}

#[test]
fn test_cli_route_with_rule_file() {
    let temp = tempdir().unwrap();
    let outbox = temp.path().join("outbox");
    let rules_file = temp.path().join("rules.json");
    let env_file = temp.path().join("envelope.json");
    let ws_dir = temp.path().join("billing_ws");
    fs::create_dir_all(&ws_dir).unwrap();

    let rules_json = serde_json::json!({
        "rules": [
            {
                "target": "billing-app",
                "match_title": { "type": "prefix", "value": "Stripe" },
                "confidence": 0.99,
                "action": "act"
            }
        ]
    });
    fs::write(&rules_file, serde_json::to_string(&rules_json).unwrap()).unwrap();

    let envelope_json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_cli_rule_test",
        "created_at": "2026-10-12T00:00:00Z",
        "caller": { "workspace": "/ws/caller", "agent": "cli-test" },
        "target": {
            "identifier": "auto",
            "workspace": ws_dir.to_str().unwrap(),
            "is_external": false
        },
        "intent": "bug_fix",
        "payload": {
            "title": "Stripe chargeback webhook 500 error",
            "severity": "high"
        }
    });
    fs::write(&env_file, serde_json::to_string(&envelope_json).unwrap()).unwrap();

    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--outbox-dir",
            outbox.to_str().unwrap(),
            "--rule-file",
            rules_file.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("DryRun"))
        .stdout(predicate::str::contains("TrustedInternal"));
}

#[test]
fn test_cli_route_with_no_memory_flag() {
    let temp = tempdir().unwrap();
    let env_file = temp.path().join("envelope_nomem.json");
    let ws_dir = temp.path().join("ws");
    fs::create_dir_all(&ws_dir).unwrap();

    let envelope_json = serde_json::json!({
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_cli_nomem_test",
        "created_at": "2026-10-12T00:00:00Z",
        "caller": { "workspace": "/ws/caller", "agent": "cli-test" },
        "target": {
            "identifier": "internal-app",
            "workspace": ws_dir.to_str().unwrap(),
            "is_external": false
        },
        "intent": "feature",
        "payload": {
            "title": "Add feature without memory",
            "severity": "normal"
        }
    });
    fs::write(&env_file, serde_json::to_string(&envelope_json).unwrap()).unwrap();

    router_cmd()
        .args([
            "route",
            "-f",
            env_file.to_str().unwrap(),
            "--no-memory",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "[DRY-RUN] Task 'task_cli_nomem_test'",
        ));
}
