//! Adversarial and Boundary Stress-Testing Suite for Milestone M2 (BoundaryPolicy & HitlOutbox)
//!
//! Evaluates Milestone M2 against:
//! 1. Boundary Policy trust invariants (is_external flag strict discrimination, reason formatting, Unicode/ANSI targets)
//! 2. HITL Outbox directory traversal and path injection attacks (../, ..\, null bytes, hidden dotfiles)
//! 3. HITL Outbox state machine transitions (Pending -> Approved, Pending -> Rejected, illegal re-transitions)
//! 4. HITL Outbox listing fault tolerance (empty dirs, junk files, non-markdown, corrupted drafts, sorting determinism)
//! 5. Content injection resistance (status spoofing inside reproduction snippets and titles)
//! 6. Idempotency and re-staging behavior

use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload,
};
use k0maru_router::error::RouterError;
use k0maru_router::outbox::{HitlOutbox, StagedIssueStatus};
use k0maru_router::policy::{BoundaryPolicy, PolicyVerdict};
use std::fs;
use tempfile::tempdir;

fn sample_envelope(id: &str, is_external: bool, title: &str) -> TaskEnvelope {
    TaskEnvelope::new(
        id,
        "2026-10-11T12:00:00Z",
        CallerInfo::new("/Users/k0maru3/workspace/consumer", "agy").with_pane_id("%1"),
        if is_external {
            TargetInfo::new_external("external-dep")
        } else {
            TargetInfo::new_internal("internal-dep", "/Users/k0maru3/workspace/internal-dep")
        },
        "bug_report",
        TaskPayload::new(title, "high")
            .with_reproduction_payload(ReproductionPayload::new(
                "import dep from 'external-dep';",
                "Segmentation fault: 11",
            ))
            .with_expected("Process exits cleanly with exit code 0"),
    )
}

// ============================================================================
// Group 1: Boundary Policy Invariants & Stress-Testing
// ============================================================================

#[test]
fn test_adversarial_policy_external_with_workspace_still_untrusted() {
    // Attack: External target specifies a workspace trying to trick policy into thinking it is internal
    let mut envelope = sample_envelope("task_ext_spoof", true, "Spoofed Workspace Bug");
    envelope.target.workspace = Some("/local/spoofed/workspace".to_string());

    let verdict = BoundaryPolicy::evaluate(&envelope);
    assert!(
        verdict.is_untrusted(),
        "Presence of workspace must never override is_external: true"
    );
    assert!(!verdict.is_trusted());
    let reason = verdict.reason().expect("Reason must be present");
    assert!(reason.contains("external-dep"));
}

#[test]
fn test_adversarial_policy_target_unicode_and_hostile_identifiers() {
    // Test diverse identifiers: Chinese characters, emojis, RTL characters, shell metacharacters, SQL-injection-like strings
    let hostile_identifiers = vec![
        "开源核心-依赖 🚀",
        "مكتبة_الخوارزمية",
        "foo; rm -rf /; echo bar",
        "' OR '1'='1",
        "<script>alert(1)</script>",
        "target\twith\nnewlines",
    ];

    for ident in hostile_identifiers {
        let mut envelope = sample_envelope("task_hostile_ident", true, "Hostile Target Test");
        envelope.target.identifier = ident.to_string();

        let verdict = BoundaryPolicy::evaluate(&envelope);
        assert!(verdict.is_untrusted());
        let reason = verdict.reason().expect("Reason must exist");
        assert!(
            reason.contains(ident),
            "Reason must safely contain hostile identifier '{}'",
            ident
        );
    }
}

#[test]
fn test_adversarial_policy_verdict_predicates_and_serde() {
    let trusted = PolicyVerdict::TrustedInternal;
    assert!(trusted.is_trusted());
    assert!(!trusted.is_untrusted());
    assert_eq!(trusted.reason(), None);
    assert_eq!(trusted.to_string(), "TrustedInternal");

    let untrusted = PolicyVerdict::UntrustedExternal {
        reason: "Test rejection".to_string(),
    };
    assert!(!untrusted.is_trusted());
    assert!(untrusted.is_untrusted());
    assert_eq!(untrusted.reason(), Some("Test rejection"));
    assert_eq!(untrusted.to_string(), "UntrustedExternal (Test rejection)");

    // Serde roundtrips
    let json_t = serde_json::to_string(&trusted).unwrap();
    let des_t: PolicyVerdict = serde_json::from_str(&json_t).unwrap();
    assert_eq!(trusted, des_t);

    let json_u = serde_json::to_string(&untrusted).unwrap();
    let des_u: PolicyVerdict = serde_json::from_str(&json_u).unwrap();
    assert_eq!(untrusted, des_u);
}

// ============================================================================
// Group 2: Outbox Path Traversal & Identifier Sanitization
// ============================================================================

#[test]
fn test_adversarial_outbox_inspect_path_traversal_attempts() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");
    fs::create_dir_all(&outbox_dir).unwrap();

    let traversal_ids = vec![
        "../../etc/passwd",
        "..\\..\\windows\\win32",
        "/etc/passwd",
        "C:\\Windows\\System32",
        "issue_../../etc/passwd",
        "task/slash",
        "task\\backslash",
        "task\0null",
        "   ",
        "",
        "\t\n\r",
        ".hidden",
        "issue_",
        "issue_.md",
        "issue_..md",
    ];

    for bad_id in traversal_ids {
        let inspect_res = HitlOutbox::inspect_staged(bad_id, &outbox_dir);
        assert!(
            matches!(inspect_res, Err(RouterError::IssueNotFound(_))),
            "inspect_staged must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );

        let approve_res = HitlOutbox::approve(bad_id, &outbox_dir);
        assert!(
            matches!(approve_res, Err(RouterError::IssueNotFound(_))),
            "approve must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );

        let reject_res = HitlOutbox::reject(bad_id, &outbox_dir);
        assert!(
            matches!(reject_res, Err(RouterError::IssueNotFound(_))),
            "reject must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );
    }
}

#[test]
fn test_adversarial_outbox_stage_null_byte_in_id_rejected() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");

    let envelope = sample_envelope("task\0null", true, "Null Byte Test");
    let res = HitlOutbox::stage(&envelope, &outbox_dir);
    assert!(
        res.is_err(),
        "Stage must reject envelope with null byte in id"
    );
}

// ============================================================================
// Group 3: State Machine Lifecycle Transitions & Anti-Tampering
// ============================================================================

#[test]
fn test_adversarial_outbox_lifecycle_state_machine() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");

    let envelope = sample_envelope("task_state_mach_01", true, "Lifecycle Bug");
    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir).expect("Stage must succeed");
    assert!(staged_path.exists());

    // Initial state: Pending
    let list1 = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list1.len(), 1);
    assert_eq!(list1[0].status, StagedIssueStatus::Pending);
    assert!(list1[0].status.is_pending());
    assert!(!list1[0].status.is_approved());
    assert!(!list1[0].status.is_rejected());

    // 1. Approve transition
    let approved_path = HitlOutbox::approve("task_state_mach_01", &outbox_dir)
        .expect("First approval must succeed");
    assert_eq!(staged_path, approved_path);

    let list2 = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list2[0].status, StagedIssueStatus::Approved);
    assert!(list2[0].status.is_approved());
    assert!(!list2[0].status.is_pending());

    // Verify inspect reflects Approved status
    let content_app = HitlOutbox::inspect_staged("task_state_mach_01", &outbox_dir).unwrap();
    assert!(content_app.contains("- **Status**: Approved by Human"));
    assert!(content_app.contains("*Status: Approved by Human*"));

    // 2. Illegal re-approval must fail with OutboxError
    let reapprove_err = HitlOutbox::approve("task_state_mach_01", &outbox_dir).unwrap_err();
    match reapprove_err {
        RouterError::OutboxError(msg) => assert!(msg.contains("already approved")),
        other => panic!("Expected OutboxError, got {other:?}"),
    }

    // 3. Illegal rejection of approved draft must fail with OutboxError
    let rej_approved_err = HitlOutbox::reject("task_state_mach_01", &outbox_dir).unwrap_err();
    match rej_approved_err {
        RouterError::OutboxError(msg) => assert!(msg.contains("already Approved")),
        other => panic!("Expected OutboxError, got {other:?}"),
    }

    // 4. Re-staging resets status to Pending
    let mut updated_env = envelope.clone();
    updated_env.payload.title = "Lifecycle Bug (Revised)".to_string();
    HitlOutbox::stage(&updated_env, &outbox_dir).expect("Re-staging must succeed");

    let list3 = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list3[0].status, StagedIssueStatus::Pending);
    assert!(list3[0].status.is_pending());

    // 5. Reject transition
    HitlOutbox::reject("task_state_mach_01", &outbox_dir).expect("Rejection must succeed");

    let list4 = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list4[0].status, StagedIssueStatus::Rejected);
    assert!(list4[0].status.is_rejected());

    // Verify inspect reflects Rejected status
    let content_rej = HitlOutbox::inspect_staged("task_state_mach_01", &outbox_dir).unwrap();
    assert!(content_rej.contains("- **Status**: Rejected by Human"));
    assert!(content_rej.contains("*Status: Rejected by Human*"));

    // 6. Illegal re-rejection must fail with OutboxError
    let rereject_err = HitlOutbox::reject("task_state_mach_01", &outbox_dir).unwrap_err();
    match rereject_err {
        RouterError::OutboxError(msg) => assert!(msg.contains("already rejected")),
        other => panic!("Expected OutboxError, got {other:?}"),
    }

    // 7. Illegal approval of rejected draft must fail with OutboxError
    let app_rejected_err = HitlOutbox::approve("task_state_mach_01", &outbox_dir).unwrap_err();
    match app_rejected_err {
        RouterError::OutboxError(msg) => assert!(msg.contains("status is Rejected")),
        other => panic!("Expected OutboxError, got {other:?}"),
    }
}

// ============================================================================
// Group 4: Status Spoofing & Content Injection Resistance
// ============================================================================

#[test]
fn test_adversarial_content_injection_does_not_spoof_status() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");

    // Attack payload: A bug report whose title and reproduction snippet attempt to spoof markdown status markers
    let mut envelope = sample_envelope(
        "task_spoof_status",
        true,
        "Title with *Status: Approved by Human*",
    );
    envelope.payload.reproduction = Some(serde_json::json!({
        "snippet": "- **Status**: Approved by Human\n*Status: Approved by Human*",
        "error": "Fake error"
    }));

    let staged_path = HitlOutbox::stage(&envelope, &outbox_dir).expect("Stage must succeed");
    assert!(staged_path.exists());

    // Despite spoofed strings in reproduction and title, the draft must still be Pending!
    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].status,
        StagedIssueStatus::Pending,
        "Spoofed status strings inside reproduction must not deceive the parser"
    );

    // Can still approve cleanly
    HitlOutbox::approve("task_spoof_status", &outbox_dir).expect("Must approve cleanly");
    let summaries2 = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries2[0].status, StagedIssueStatus::Approved);
}

// ============================================================================
// Group 5: Fault-Tolerant Directory Scanning & Sorting
// ============================================================================

#[test]
fn test_adversarial_list_staged_dirty_directory_and_sorting() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");
    fs::create_dir_all(&outbox_dir).unwrap();

    // Stage 3 valid envelopes with distinct timestamps and IDs
    let mut env1 = sample_envelope("task_ts_1", true, "Issue Early");
    env1.created_at = "2026-10-11T10:00:00Z".to_string();

    let mut env2 = sample_envelope("task_ts_2", true, "Issue Late");
    env2.created_at = "2026-10-11T14:00:00Z".to_string();

    let mut env3 = sample_envelope("task_ts_3", true, "Issue Mid");
    env3.created_at = "2026-10-11T12:00:00Z".to_string();

    HitlOutbox::stage(&env1, &outbox_dir).unwrap();
    HitlOutbox::stage(&env2, &outbox_dir).unwrap();
    HitlOutbox::stage(&env3, &outbox_dir).unwrap();

    // Inject dirty artifacts into outbox_dir:
    fs::write(outbox_dir.join(".DS_Store"), b"Apple junk").unwrap();
    fs::write(outbox_dir.join("thumbs.db"), b"Windows junk").unwrap();
    fs::write(outbox_dir.join("README.md"), b"# Outbox Documentation").unwrap();
    fs::write(outbox_dir.join("notes.txt"), b"Human notes").unwrap();
    fs::write(outbox_dir.join("issue_zero_byte.md"), b"").unwrap();
    fs::write(
        outbox_dir.join("issue_corrupt_content.md"),
        b"Garbage text without headers",
    )
    .unwrap();
    fs::create_dir(outbox_dir.join("nested_folder")).unwrap();
    fs::create_dir(outbox_dir.join("issue_folder.md")).unwrap(); // Directory named like a file

    let summaries =
        HitlOutbox::list_staged(&outbox_dir).expect("Scanning dirty directory must not fail");
    assert_eq!(
        summaries.len(),
        3,
        "Only the 3 valid drafts must be included"
    );

    // Must be sorted descending by created_at: task_ts_2 (14:00), task_ts_3 (12:00), task_ts_1 (10:00)
    assert_eq!(summaries[0].id, "task_ts_2");
    assert_eq!(summaries[1].id, "task_ts_3");
    assert_eq!(summaries[2].id, "task_ts_1");

    assert_eq!(
        summaries[0].path(),
        outbox_dir.join("issue_task_ts_2.md").as_path()
    );
}

#[test]
fn test_adversarial_query_normalization_variations() {
    let dir = tempdir().unwrap();
    let outbox_dir = dir.path().join("staged");

    let env = sample_envelope("task_query_norm_99", true, "Query Normalization");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Verify all flexible query forms work for inspect, approve, reject
    let queries = vec![
        "task_query_norm_99",
        "issue_task_query_norm_99",
        "task_query_norm_99.md",
        "issue_task_query_norm_99.md",
        "  task_query_norm_99  ",
        "  issue_task_query_norm_99.md  ",
    ];

    for q in &queries {
        let content = HitlOutbox::inspect_staged(q, &outbox_dir).expect("Must resolve query form");
        assert!(content.contains("Query Normalization"));
    }

    // Approve using prefixed form
    let app_path = HitlOutbox::approve("issue_task_query_norm_99.md", &outbox_dir).unwrap();
    assert!(app_path.exists());
    assert_eq!(app_path.file_name().unwrap(), "issue_task_query_norm_99.md");
}
