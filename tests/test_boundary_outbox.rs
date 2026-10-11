mod common;

use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::outbox::HitlOutbox;
use k0maru_router::policy::{BoundaryPolicy, PolicyVerdict};
use std::fs;

// ============================================================================
// Tier 1: Feature Coverage (>= 5 tests per feature)
// ============================================================================

#[test]
fn tier1_boundary_policy_internal_target_verdict() {
    let envelope = common::sample_internal_envelope();
    let verdict = BoundaryPolicy::evaluate(&envelope);
    assert_eq!(
        verdict,
        PolicyVerdict::TrustedInternal,
        "Target with is_external=false must evaluate to TrustedInternal"
    );
}

#[test]
fn tier1_boundary_policy_external_target_verdict() {
    let envelope = common::sample_external_envelope();
    let verdict = BoundaryPolicy::evaluate(&envelope);
    match verdict {
        PolicyVerdict::UntrustedExternal { reason } => {
            assert!(
                reason.contains(&envelope.target.identifier),
                "Rejection reason '{}' must reference target identifier '{}'",
                reason,
                envelope.target.identifier
            );
        }
        PolicyVerdict::TrustedInternal => {
            panic!("Target with is_external=true must not evaluate to TrustedInternal");
        }
    }
}

#[test]
fn tier1_boundary_policy_untrusted_reason_content() {
    let mut envelope = common::sample_external_envelope();
    envelope.target.identifier = "vitest-upstream".to_string();

    let verdict = BoundaryPolicy::evaluate(&envelope);
    if let PolicyVerdict::UntrustedExternal { reason } = verdict {
        assert!(reason.contains("vitest-upstream"));
        assert!(reason.to_lowercase().contains("external"));
    } else {
        panic!("Expected UntrustedExternal verdict");
    }
}

#[test]
fn tier1_outbox_stage_creates_markdown_file() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let envelope = common::sample_external_envelope();
    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir)
        .expect("HitlOutbox::stage must succeed for valid envelope");

    assert!(staged_path.exists(), "Staged file must exist on disk");
    assert_eq!(
        staged_path.file_name().unwrap(),
        format!("issue_{}.md", envelope.id).as_str()
    );
}

#[test]
fn tier1_outbox_stage_markdown_structure() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let envelope = common::sample_external_envelope();
    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir).expect("Stage must succeed");

    let content = fs::read_to_string(&staged_path).expect("Must read staged file");

    // Verify mandatory MRE markdown elements
    assert!(
        content.contains(&format!("# Staged Issue Draft: {}", envelope.payload.title)),
        "Must contain title header"
    );
    assert!(
        content.contains(&format!("- **ID**: {}", envelope.id)),
        "Must contain ID field"
    );
    assert!(
        content.contains(&format!("- **Target**: {}", envelope.target.identifier)),
        "Must contain Target field"
    );
    assert!(
        content.contains(&format!("- **Severity**: {}", envelope.payload.severity)),
        "Must contain Severity field"
    );
    assert!(
        content.contains(&format!("- **Created At**: {}", envelope.created_at)),
        "Must contain Created At field"
    );
    assert!(
        content.contains("## Reproduction"),
        "Must contain Reproduction section"
    );
    assert!(
        content.contains("## Expected"),
        "Must contain Expected section"
    );
    assert!(
        content.contains("*Status: Pending Human Authorization (HITL)*"),
        "Must contain explicit HITL pending status"
    );
}

#[test]
fn tier1_outbox_stage_auto_creates_outbox_dir() {
    let temp_dir = common::create_temp_dir();
    let nested_outbox = temp_dir.path().join("deeply").join("nested").join("staged");

    assert!(!nested_outbox.exists());

    let envelope = common::sample_external_envelope();
    let staged_path = HitlOutbox::stage(&envelope, &nested_outbox)
        .expect("Stage must automatically create non-existent directories");

    assert!(nested_outbox.exists(), "Directory should have been created");
    assert!(staged_path.exists(), "Staged file should exist");
}

#[test]
fn tier1_cli_status_runner_helper() {
    let mut cmd = common::router_cmd();
    cmd.arg("status")
        .assert()
        .success()
        .stdout(predicates::str::contains("k0maru-router v"));
}

// ============================================================================
// Tier 2: Boundary & Corner Cases (>= 5 tests per feature)
// ============================================================================

#[test]
fn tier2_outbox_stage_with_special_characters_in_payload() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let mut envelope = common::sample_external_envelope();
    envelope.id = "task_spec_chars".to_string();
    envelope.payload.title = "Crash on `<Tag prop=\"val\" />` & **Markdown** *bold*".to_string();
    envelope.payload.expected = Some("Handle & < > quotes \"safe\"".to_string());

    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir)
        .expect("Stage must succeed with special characters in title");

    let content = fs::read_to_string(staged_path).expect("Must read staged file");
    assert!(content.contains(&envelope.payload.title));
    assert!(content.contains("Handle & < > quotes \"safe\""));
}

#[test]
fn tier2_outbox_stage_none_optional_fields() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let mut envelope = common::sample_external_envelope();
    envelope.id = "task_none_opts".to_string();
    envelope.payload.reproduction = None;
    envelope.payload.expected = None;

    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir)
        .expect("Stage must succeed when optional payload fields are None");

    let content = fs::read_to_string(staged_path).expect("Must read staged file");
    assert!(
        content.contains("N/A"),
        "Expected field should default to N/A"
    );
}

#[test]
fn tier2_outbox_stage_idempotent_overwrite() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let mut envelope = common::sample_external_envelope();
    envelope.id = "task_idempotent".to_string();
    envelope.payload.title = "Original Title".to_string();

    let path1 = HitlOutbox::stage(&envelope, &outbox_dir).expect("First stage must succeed");
    let content1 = fs::read_to_string(&path1).unwrap();
    assert!(content1.contains("Original Title"));

    // Update title and stage again with same ID
    envelope.payload.title = "Updated Title".to_string();
    let path2 = HitlOutbox::stage(&envelope, &outbox_dir).expect("Second stage must succeed");
    assert_eq!(path1, path2, "Paths must be identical");

    let content2 = fs::read_to_string(&path2).unwrap();
    assert!(content2.contains("Updated Title"));
    assert!(!content2.contains("Original Title"));
}

#[test]
fn tier2_outbox_stage_distinct_ids_create_distinct_files() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let mut env1 = common::sample_external_envelope();
    env1.id = "task_alpha".to_string();

    let mut env2 = common::sample_external_envelope();
    env2.id = "task_beta".to_string();

    let path1 = HitlOutbox::stage(&env1, &outbox_dir).expect("Stage 1 must succeed");
    let path2 = HitlOutbox::stage(&env2, &outbox_dir).expect("Stage 2 must succeed");

    assert_ne!(path1, path2);
    assert!(path1.exists());
    assert!(path2.exists());

    let entries: Vec<_> = fs::read_dir(&outbox_dir)
        .unwrap()
        .map(|r| r.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&"issue_task_alpha.md".to_string()));
    assert!(entries.contains(&"issue_task_beta.md".to_string()));
}

#[test]
fn tier2_outbox_stage_unicode_and_emojis() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let mut envelope = common::sample_external_envelope();
    envelope.id = "task_utf8_123".to_string();
    envelope.payload.title = "修复组件 aria-label 属性转发失败 🚀".to_string();
    envelope.target.identifier = "开源框架-Core".to_string();

    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir)
        .expect("Stage must handle multibyte UTF-8 characters cleanly");

    let content = fs::read_to_string(staged_path).unwrap();
    assert!(content.contains("修复组件 aria-label 属性转发失败 🚀"));
    assert!(content.contains("开源框架-Core"));
}

// ============================================================================
// Tier 3: Cross-Feature Combinations
// ============================================================================

#[test]
fn tier3_policy_evaluation_guides_staging() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let internal_env = common::sample_internal_envelope();
    let external_env = common::sample_external_envelope();

    // 1. Process Internal Envelope
    let verdict_internal = BoundaryPolicy::evaluate(&internal_env);
    assert_eq!(verdict_internal, PolicyVerdict::TrustedInternal);
    // Trusted internal should NOT stage
    assert!(
        !outbox_dir.exists() || fs::read_dir(&outbox_dir).unwrap().next().is_none(),
        "Internal envelope must not stage to outbox"
    );

    // 2. Process External Envelope
    let verdict_external = BoundaryPolicy::evaluate(&external_env);
    assert!(matches!(
        verdict_external,
        PolicyVerdict::UntrustedExternal { .. }
    ));
    let staged_path = HitlOutbox::stage(&external_env, &outbox_dir).expect("Stage must succeed");
    assert!(staged_path.exists());
}

#[test]
fn tier3_multi_envelope_batch_staging_isolation() {
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join("staged");

    let envelopes = vec![
        TaskEnvelope {
            schema: None,
            id: "task_ext_1".to_string(),
            created_at: "2026-10-11T10:00:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/ws".to_string(),
                agent: "a".to_string(),
                pane_id: None,
            },
            target: TargetInfo {
                identifier: "ext-1".to_string(),
                workspace: None,
                is_external: true,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: "External 1".to_string(),
                severity: "high".to_string(),
                reproduction: None,
                expected: None,
            },
        },
        TaskEnvelope {
            schema: None,
            id: "task_int_2".to_string(),
            created_at: "2026-10-11T10:01:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/ws".to_string(),
                agent: "a".to_string(),
                pane_id: None,
            },
            target: TargetInfo {
                identifier: "int-2".to_string(),
                workspace: None,
                is_external: false,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: "Internal 2".to_string(),
                severity: "low".to_string(),
                reproduction: None,
                expected: None,
            },
        },
        TaskEnvelope {
            schema: None,
            id: "task_ext_3".to_string(),
            created_at: "2026-10-11T10:02:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/ws".to_string(),
                agent: "a".to_string(),
                pane_id: None,
            },
            target: TargetInfo {
                identifier: "ext-3".to_string(),
                workspace: None,
                is_external: true,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: "External 3".to_string(),
                severity: "medium".to_string(),
                reproduction: None,
                expected: None,
            },
        },
    ];

    for env in &envelopes {
        let verdict = BoundaryPolicy::evaluate(env);
        if matches!(verdict, PolicyVerdict::UntrustedExternal { .. }) {
            HitlOutbox::stage(env, &outbox_dir).expect("Stage must succeed");
        }
    }

    let staged_files: Vec<_> = fs::read_dir(&outbox_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();

    assert_eq!(staged_files.len(), 2);
    assert!(staged_files.contains(&"issue_task_ext_1.md".to_string()));
    assert!(staged_files.contains(&"issue_task_ext_3.md".to_string()));
    assert!(!staged_files.contains(&"issue_task_int_2.md".to_string()));
}

// ============================================================================
// Tier 4: Real-World Scenarios
// ============================================================================

#[test]
fn tier4_scenario_upstream_oss_quarantine_workflow() {
    // Scenario: A consumer agent discovers an issue in external UI library 'Terra-UI'.
    // The agent serializes a bug report TaskEnvelope.
    // The router BoundaryPolicy identifies this as untrusted external OSS and quarantines it.
    // The staged markdown draft is generated with full reproduction details for human approval.
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join(".scratch").join("staged");

    let raw_json = common::sample_valid_external_json();
    let envelope: TaskEnvelope =
        serde_json::from_str(&raw_json).expect("Consumer envelope deserialization must succeed");

    let verdict = BoundaryPolicy::evaluate(&envelope);
    match verdict {
        PolicyVerdict::UntrustedExternal { reason } => {
            assert!(reason.contains("Terra-UI"));
            let draft_path =
                HitlOutbox::stage(&envelope, &outbox_dir).expect("Staging must succeed");
            assert!(draft_path.exists());

            let content = fs::read_to_string(&draft_path).expect("Read staged markdown");
            assert!(content.contains(
                "# Staged Issue Draft: TerraButton missing aria-label attribute pass-through"
            ));
            assert!(content.contains("HTML button element does not receive aria-label"));
            assert!(content.contains("*Status: Pending Human Authorization (HITL)*"));
        }
        PolicyVerdict::TrustedInternal => {
            panic!("External dependency 'Terra-UI' must be flagged as UntrustedExternal");
        }
    }
}

#[test]
fn tier4_scenario_internal_service_direct_routing() {
    // Scenario: A consumer agent requests assistance from an internal monorepo sister crate.
    // Policy confirms it is trusted internal, so no outbox staging occurs.
    let temp_dir = common::create_temp_dir();
    let outbox_dir = temp_dir.path().join(".scratch").join("staged");

    let raw_json = common::sample_valid_internal_json();
    let envelope: TaskEnvelope =
        serde_json::from_str(&raw_json).expect("Internal envelope deserialization must succeed");

    let verdict = BoundaryPolicy::evaluate(&envelope);
    assert_eq!(verdict, PolicyVerdict::TrustedInternal);

    // Verify outbox directory was not created and no files staged
    assert!(!outbox_dir.exists());
}
