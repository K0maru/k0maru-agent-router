//! Adversarial and Boundary Stress-Testing Suite for Milestone M2:
//! HITL Outbox Lifecycle, Tri-State Machine, and Deterministic Sorting.
//!
//! Specifically tests:
//! 1. Tri-state machine: `Pending -> Approved`, `Pending -> Rejected`.
//! 2. Rejection of invalid transitions:
//!    - Duplicate approval of already approved issue
//!    - Approving a rejected issue
//!    - Duplicate rejection of already rejected issue
//!    - Rejecting an approved issue
//! 3. Re-staging of existing issue ID resetting state to `Pending`.
//! 4. Deterministic sorting in `list_staged` (created_at descending, id ascending tie-breaker).
//! 5. Lifecycle operations with ID variants (prefix, extension, whitespace).
//! 6. Resilience against adversarial draft payloads and non-existent/traversal IDs.

use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use k0maru_router::error::RouterError;
use k0maru_router::outbox::{HitlOutbox, StagedIssueStatus};
use std::fs;
use tempfile::tempdir;

fn sample_external_envelope(id: &str, title: &str, created_at: &str) -> TaskEnvelope {
    TaskEnvelope {
        schema: None,
        id: id.to_string(),
        created_at: created_at.to_string(),
        caller: CallerInfo {
            workspace: "/Users/k0maru3/workspace/consumer-app".to_string(),
            agent: "agy".to_string(),
            pane_id: Some("%1".to_string()),
        },
        target: TargetInfo {
            identifier: "Terra-UI".to_string(),
            workspace: None,
            is_external: true,
        },
        intent: "bug_report".to_string(),
        payload: TaskPayload {
            title: title.to_string(),
            severity: "blocking".to_string(),
            reproduction: Some(serde_json::json!({
                "snippet": "<TerraButton aria-label='close' />",
                "error": "HTML button element does not receive aria-label"
            })),
            expected: Some("Attribute forwarded to rendered <button> element".to_string()),
        },
    }
}

// ============================================================================
// 1. Tri-State Machine: Pending -> Approved, Pending -> Rejected
// ============================================================================

#[test]
fn test_lifecycle_pending_to_approved_full_flow() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope("task_app_01", "Aria Label Bug", "2026-10-11T10:00:00Z");
    let staged_path = HitlOutbox::stage(&env, &outbox_dir).expect("Stage must succeed");

    // 1. Verify initial Pending status
    let initial_content = fs::read_to_string(&staged_path).expect("Read draft");
    assert!(initial_content.contains("- **Status**: Pending Human Authorization (HITL)"));
    assert!(initial_content.contains("*Status: Pending Human Authorization (HITL)*"));

    let list_initial = HitlOutbox::list_staged(&outbox_dir).expect("List staged");
    assert_eq!(list_initial.len(), 1);
    assert_eq!(list_initial[0].id, "task_app_01");
    assert_eq!(list_initial[0].status, StagedIssueStatus::Pending);
    assert!(list_initial[0].status.is_pending());
    assert!(!list_initial[0].status.is_approved());
    assert!(!list_initial[0].status.is_rejected());

    // 2. Perform transition: Pending -> Approved
    let approved_path =
        HitlOutbox::approve("task_app_01", &outbox_dir).expect("Approve must succeed");
    assert_eq!(staged_path, approved_path);
    assert!(approved_path.exists());

    // 3. Verify file content updated on disk
    let approved_content = fs::read_to_string(&approved_path).expect("Read approved draft");
    assert!(approved_content.contains("- **Status**: Approved by Human"));
    assert!(approved_content.contains("*Status: Approved by Human*"));
    assert!(!approved_content.contains("Pending Human Authorization (HITL)"));
    assert!(!approved_content.contains("Rejected by Human"));

    // 4. Verify inspect_staged reflects Approved
    let inspected = HitlOutbox::inspect_staged("task_app_01", &outbox_dir).expect("Inspect staged");
    assert_eq!(inspected, approved_content);

    // 5. Verify list_staged reflects Approved
    let list_approved = HitlOutbox::list_staged(&outbox_dir).expect("List staged");
    assert_eq!(list_approved.len(), 1);
    assert_eq!(list_approved[0].status, StagedIssueStatus::Approved);
    assert!(list_approved[0].status.is_approved());
    assert!(!list_approved[0].status.is_pending());
    assert!(!list_approved[0].status.is_rejected());
}

#[test]
fn test_lifecycle_pending_to_rejected_full_flow() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope("task_rej_01", "Flaky Test Report", "2026-10-11T10:00:00Z");
    let staged_path = HitlOutbox::stage(&env, &outbox_dir).expect("Stage must succeed");

    // 1. Verify initial Pending status
    let initial_content = fs::read_to_string(&staged_path).expect("Read draft");
    assert!(initial_content.contains("- **Status**: Pending Human Authorization (HITL)"));

    let list_initial = HitlOutbox::list_staged(&outbox_dir).expect("List staged");
    assert_eq!(list_initial.len(), 1);
    assert_eq!(list_initial[0].status, StagedIssueStatus::Pending);

    // 2. Perform transition: Pending -> Rejected
    HitlOutbox::reject("task_rej_01", &outbox_dir).expect("Reject must succeed");

    // 3. Verify file still exists on disk (audit trail preserved)
    assert!(
        staged_path.exists(),
        "Rejected draft must be preserved on disk"
    );

    // 4. Verify file content updated on disk
    let rejected_content = fs::read_to_string(&staged_path).expect("Read rejected draft");
    assert!(rejected_content.contains("- **Status**: Rejected by Human"));
    assert!(rejected_content.contains("*Status: Rejected by Human*"));
    assert!(!rejected_content.contains("Pending Human Authorization (HITL)"));
    assert!(!rejected_content.contains("Approved by Human"));

    // 5. Verify inspect_staged reflects Rejected
    let inspected = HitlOutbox::inspect_staged("task_rej_01", &outbox_dir).expect("Inspect staged");
    assert_eq!(inspected, rejected_content);

    // 6. Verify list_staged reflects Rejected
    let list_rejected = HitlOutbox::list_staged(&outbox_dir).expect("List staged");
    assert_eq!(list_rejected.len(), 1);
    assert_eq!(list_rejected[0].status, StagedIssueStatus::Rejected);
    assert!(list_rejected[0].status.is_rejected());
    assert!(!list_rejected[0].status.is_pending());
    assert!(!list_rejected[0].status.is_approved());
}

// ============================================================================
// 2. Rejection of Invalid Transitions
// ============================================================================

#[test]
fn test_lifecycle_duplicate_approval_rejected() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope(
        "task_dup_app",
        "Double Approve Test",
        "2026-10-11T10:00:00Z",
    );
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // First approval succeeds
    HitlOutbox::approve("task_dup_app", &outbox_dir).expect("First approval must succeed");

    // Second approval must fail with typed RouterError::OutboxError
    let err1 = HitlOutbox::approve("task_dup_app", &outbox_dir).unwrap_err();
    match err1 {
        RouterError::OutboxError(msg) => {
            assert!(
                msg.contains("already approved"),
                "Expected 'already approved' error message, got: {msg}"
            );
        }
        other => panic!("Expected RouterError::OutboxError, got: {other:?}"),
    }

    // Third approval must also fail consistently
    let err2 = HitlOutbox::approve("task_dup_app", &outbox_dir).unwrap_err();
    assert!(matches!(err2, RouterError::OutboxError(_)));

    // Verify status on disk remains Approved without corruption
    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list[0].status, StagedIssueStatus::Approved);
}

#[test]
fn test_lifecycle_approve_rejected_issue_rejected() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope(
        "task_app_after_rej",
        "Reject then Approve Test",
        "2026-10-11T10:00:00Z",
    );
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Transition to Rejected
    HitlOutbox::reject("task_app_after_rej", &outbox_dir).expect("Reject must succeed");

    // Attempting to approve a Rejected issue must fail
    let err = HitlOutbox::approve("task_app_after_rej", &outbox_dir).unwrap_err();
    match err {
        RouterError::OutboxError(msg) => {
            assert!(
                msg.contains("status is Rejected"),
                "Expected 'status is Rejected' error message, got: {msg}"
            );
        }
        other => panic!("Expected RouterError::OutboxError, got: {other:?}"),
    }

    // Verify status on disk remains Rejected
    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list[0].status, StagedIssueStatus::Rejected);
}

#[test]
fn test_lifecycle_duplicate_rejection_rejected() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env =
        sample_external_envelope("task_dup_rej", "Double Reject Test", "2026-10-11T10:00:00Z");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // First rejection succeeds
    HitlOutbox::reject("task_dup_rej", &outbox_dir).expect("First rejection must succeed");

    // Second rejection must fail with typed RouterError::OutboxError
    let err1 = HitlOutbox::reject("task_dup_rej", &outbox_dir).unwrap_err();
    match err1 {
        RouterError::OutboxError(msg) => {
            assert!(
                msg.contains("already rejected"),
                "Expected 'already rejected' error message, got: {msg}"
            );
        }
        other => panic!("Expected RouterError::OutboxError, got: {other:?}"),
    }

    // Third rejection must also fail
    let err2 = HitlOutbox::reject("task_dup_rej", &outbox_dir).unwrap_err();
    assert!(matches!(err2, RouterError::OutboxError(_)));

    // Verify status on disk remains Rejected
    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list[0].status, StagedIssueStatus::Rejected);
}

#[test]
fn test_lifecycle_reject_approved_issue_rejected() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope(
        "task_rej_after_app",
        "Approve then Reject Test",
        "2026-10-11T10:00:00Z",
    );
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Transition to Approved
    HitlOutbox::approve("task_rej_after_app", &outbox_dir).expect("Approve must succeed");

    // Attempting to reject an Approved issue must fail
    let err = HitlOutbox::reject("task_rej_after_app", &outbox_dir).unwrap_err();
    match err {
        RouterError::OutboxError(msg) => {
            assert!(
                msg.contains("already Approved"),
                "Expected 'already Approved' error message, got: {msg}"
            );
        }
        other => panic!("Expected RouterError::OutboxError, got: {other:?}"),
    }

    // Verify status on disk remains Approved
    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list[0].status, StagedIssueStatus::Approved);
}

// ============================================================================
// 3. Re-staging Resets State to Pending
// ============================================================================

#[test]
fn test_lifecycle_restaging_approved_resets_to_pending() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let mut env =
        sample_external_envelope("task_reuse_app", "Original Version", "2026-10-11T10:00:00Z");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // 1. Approve it
    HitlOutbox::approve("task_reuse_app", &outbox_dir).unwrap();
    let status_before = HitlOutbox::list_staged(&outbox_dir).unwrap()[0].status;
    assert_eq!(status_before, StagedIssueStatus::Approved);

    // 2. Re-stage the same issue ID with updated title
    env.payload.title = "Updated Revision 2".to_string();
    let restaged_path = HitlOutbox::stage(&env, &outbox_dir).expect("Re-stage must succeed");

    // 3. Verify status was reset to Pending
    let content = fs::read_to_string(&restaged_path).unwrap();
    assert!(content.contains("Updated Revision 2"));
    assert!(content.contains("- **Status**: Pending Human Authorization (HITL)"));
    assert!(content.contains("*Status: Pending Human Authorization (HITL)*"));
    assert!(!content.contains("Approved by Human"));

    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "task_reuse_app");
    assert_eq!(list[0].title, "Updated Revision 2");
    assert_eq!(list[0].status, StagedIssueStatus::Pending);

    // 4. Verify the reset issue can be approved again
    HitlOutbox::approve("task_reuse_app", &outbox_dir).expect("New approval must succeed");
    let list_after = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list_after[0].status, StagedIssueStatus::Approved);
}

#[test]
fn test_lifecycle_restaging_rejected_resets_to_pending() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let mut env =
        sample_external_envelope("task_reuse_rej", "Rejected Draft", "2026-10-11T10:00:00Z");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // 1. Reject it
    HitlOutbox::reject("task_reuse_rej", &outbox_dir).unwrap();
    let status_before = HitlOutbox::list_staged(&outbox_dir).unwrap()[0].status;
    assert_eq!(status_before, StagedIssueStatus::Rejected);

    // 2. Re-stage the same issue ID
    env.payload.title = "Amended Draft Revision".to_string();
    HitlOutbox::stage(&env, &outbox_dir).expect("Re-stage must succeed");

    // 3. Verify status reset to Pending
    let list = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list[0].status, StagedIssueStatus::Pending);

    // 4. Verify it can now be approved (which was previously forbidden on a Rejected issue)
    HitlOutbox::approve("task_reuse_rej", &outbox_dir)
        .expect("Approve after re-stage must succeed");
    let list_after = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(list_after[0].status, StagedIssueStatus::Approved);
}

#[test]
fn test_lifecycle_restaging_multi_cycle_state_machine() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let mut env = sample_external_envelope("task_cycle", "Cycle Test", "2026-10-11T10:00:00Z");

    for cycle in 0..5 {
        // Step A: Stage / Re-stage -> must always be Pending
        env.payload.title = format!("Cycle Title #{cycle}");
        HitlOutbox::stage(&env, &outbox_dir).unwrap();

        let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
        assert_eq!(summaries[0].status, StagedIssueStatus::Pending);
        assert_eq!(summaries[0].title, format!("Cycle Title #{cycle}"));

        if cycle % 2 == 0 {
            // Even cycles: transition to Approved
            HitlOutbox::approve("task_cycle", &outbox_dir).unwrap();
            let sum = HitlOutbox::list_staged(&outbox_dir).unwrap();
            assert_eq!(sum[0].status, StagedIssueStatus::Approved);
            // Verify duplicate approve fails
            assert!(HitlOutbox::approve("task_cycle", &outbox_dir).is_err());
        } else {
            // Odd cycles: transition to Rejected
            HitlOutbox::reject("task_cycle", &outbox_dir).unwrap();
            let sum = HitlOutbox::list_staged(&outbox_dir).unwrap();
            assert_eq!(sum[0].status, StagedIssueStatus::Rejected);
            // Verify duplicate reject fails
            assert!(HitlOutbox::reject("task_cycle", &outbox_dir).is_err());
        }
    }
}

// ============================================================================
// 4. Deterministic Sorting in `list_staged`
// ============================================================================

#[test]
fn test_deterministic_sorting_by_timestamp_descending() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let timestamps = [
        ("task_01", "2026-10-11T08:00:00Z"),
        ("task_02", "2026-10-11T12:00:00Z"),
        ("task_03", "2026-10-11T09:30:00Z"),
        ("task_04", "2026-10-11T18:45:00Z"),
        ("task_05", "2026-10-11T11:15:00Z"),
    ];

    for (id, ts) in &timestamps {
        let env = sample_external_envelope(id, &format!("Title {id}"), ts);
        HitlOutbox::stage(&env, &outbox_dir).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 5);

    // Expected order: strictly descending by created_at:
    // task_04 (18:45), task_02 (12:00), task_05 (11:15), task_03 (09:30), task_01 (08:00)
    let expected_id_order = ["task_04", "task_02", "task_05", "task_03", "task_01"];
    let actual_id_order: Vec<&str> = summaries.iter().map(|s| s.id.as_str()).collect();

    assert_eq!(actual_id_order, expected_id_order);
}

#[test]
fn test_deterministic_sorting_tie_breaker_by_id_ascending() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let identical_ts = "2026-10-11T12:00:00Z";
    let unsorted_ids = [
        "task_echo",
        "task_bravo",
        "task_alpha",
        "task_delta",
        "task_charlie",
    ];

    for id in &unsorted_ids {
        let env = sample_external_envelope(id, &format!("Title {id}"), identical_ts);
        HitlOutbox::stage(&env, &outbox_dir).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 5);

    // All created_at are identical -> must tie-break ascending by ID:
    let expected_order = [
        "task_alpha",
        "task_bravo",
        "task_charlie",
        "task_delta",
        "task_echo",
    ];
    let actual_order: Vec<&str> = summaries.iter().map(|s| s.id.as_str()).collect();

    assert_eq!(actual_order, expected_order);
}

#[test]
fn test_deterministic_sorting_mixed_timestamps_and_tie_breakers() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    // Two at 14:00 (z, a), two at 12:00 (y, b), two at 10:00 (x, c)
    let mixed = [
        ("task_z", "2026-10-11T14:00:00Z"),
        ("task_a", "2026-10-11T14:00:00Z"),
        ("task_y", "2026-10-11T12:00:00Z"),
        ("task_b", "2026-10-11T12:00:00Z"),
        ("task_x", "2026-10-11T10:00:00Z"),
        ("task_c", "2026-10-11T10:00:00Z"),
    ];

    for (id, ts) in &mixed {
        let env = sample_external_envelope(id, &format!("Title {id}"), ts);
        HitlOutbox::stage(&env, &outbox_dir).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 6);

    // Expected order:
    // 14:00 -> task_a, task_z
    // 12:00 -> task_b, task_y
    // 10:00 -> task_c, task_x
    let expected = ["task_a", "task_z", "task_b", "task_y", "task_c", "task_x"];
    let actual: Vec<&str> = summaries.iter().map(|s| s.id.as_str()).collect();

    assert_eq!(actual, expected);

    // Multiple invocations must yield identical deterministic results
    let summaries_second = HitlOutbox::list_staged(&outbox_dir).unwrap();
    let actual_second: Vec<&str> = summaries_second.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(actual, actual_second);
}

// ============================================================================
// 5. Lifecycle Operations with ID Variants & Whitespace
// ============================================================================

#[test]
fn test_lifecycle_operations_with_id_variants() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let env = sample_external_envelope("task_variant", "Variant Test", "2026-10-11T10:00:00Z");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Query inspect with various accepted forms
    let c1 = HitlOutbox::inspect_staged("task_variant", &outbox_dir).unwrap();
    let c2 = HitlOutbox::inspect_staged("issue_task_variant", &outbox_dir).unwrap();
    let c3 = HitlOutbox::inspect_staged("task_variant.md", &outbox_dir).unwrap();
    let c4 = HitlOutbox::inspect_staged("issue_task_variant.md", &outbox_dir).unwrap();
    let c5 = HitlOutbox::inspect_staged("   issue_task_variant.md   ", &outbox_dir).unwrap();

    assert_eq!(c1, c2);
    assert_eq!(c1, c3);
    assert_eq!(c1, c4);
    assert_eq!(c1, c5);

    // Approve using prefixed filename variant with whitespace
    HitlOutbox::approve("   issue_task_variant.md   ", &outbox_dir).unwrap();

    // Verify inspect via bare ID now sees Approved
    let content_after = HitlOutbox::inspect_staged("task_variant", &outbox_dir).unwrap();
    assert!(content_after.contains("Approved by Human"));

    // Verify duplicate approve using bare ID fails
    assert!(HitlOutbox::approve("task_variant", &outbox_dir).is_err());
}

// ============================================================================
// 6. Security Boundaries & Traversal Rejections
// ============================================================================

#[test]
fn test_lifecycle_operations_on_nonexistent_ids() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let bad_id = "ghost_task_404";
    assert!(matches!(
        HitlOutbox::approve(bad_id, &outbox_dir).unwrap_err(),
        RouterError::IssueNotFound(id) if id == bad_id
    ));
    assert!(matches!(
        HitlOutbox::reject(bad_id, &outbox_dir).unwrap_err(),
        RouterError::IssueNotFound(id) if id == bad_id
    ));
    assert!(matches!(
        HitlOutbox::inspect_staged(bad_id, &outbox_dir).unwrap_err(),
        RouterError::IssueNotFound(id) if id == bad_id
    ));
}

#[test]
fn test_lifecycle_operations_reject_path_traversal() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let attack_vectors = [
        "../etc/passwd",
        "../../shadow",
        "..",
        "/etc/passwd",
        "\\windows\\system32",
        "sub/../evil",
        "task\0evil",
        "   ",
        "",
    ];

    for vec in attack_vectors {
        assert!(matches!(
            HitlOutbox::approve(vec, &outbox_dir).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
        assert!(matches!(
            HitlOutbox::reject(vec, &outbox_dir).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
        assert!(matches!(
            HitlOutbox::inspect_staged(vec, &outbox_dir).unwrap_err(),
            RouterError::IssueNotFound(_)
        ));
    }
}

// ============================================================================
// 7. Integrity: Preservation of Unrelated Draft Content
// ============================================================================

#[test]
fn test_lifecycle_status_transitions_preserve_body_content() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let mut env = sample_external_envelope(
        "task_integrity",
        "Complex Bug Report",
        "2026-10-11T10:00:00Z",
    );
    env.payload.expected =
        Some("Multiline\nExpected\nBehavior with *formatting* & <code>".to_string());
    env.payload.reproduction = Some(serde_json::json!({
        "steps": [1, 2, 3],
        "details": {
            "nested_key": "special characters: & < > \" '",
            "fake_status": "Status: Pending Fake Line"
        }
    }));

    let path = HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Transition to Approved
    HitlOutbox::approve("task_integrity", &outbox_dir).unwrap();

    let content = fs::read_to_string(&path).unwrap();
    // Verify core body remains untouched
    assert!(content.contains("# Staged Issue Draft: Complex Bug Report"));
    assert!(content.contains("- **ID**: task_integrity"));
    assert!(content.contains("- **Target**: Terra-UI"));
    assert!(content.contains("- **Severity**: blocking"));
    assert!(content.contains("- **Created At**: 2026-10-11T10:00:00Z"));
    assert!(content.contains("Multiline\nExpected\nBehavior with *formatting* & <code>"));
    assert!(content.contains("special characters: & < > \\\" '"));
    assert!(content.contains("- **Status**: Approved by Human"));
    assert!(content.contains("*Status: Approved by Human*"));
}

// ============================================================================
// 8. Independent Isolation Across Multiple Staged Drafts
// ============================================================================

#[test]
fn test_concurrent_independent_draft_lifecycle_transitions() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    // Stage 10 distinct drafts
    for i in 0..10 {
        let id = format!("task_{i:02}");
        let env = sample_external_envelope(
            &id,
            &format!("Bug {i}"),
            &format!("2026-10-11T10:{i:02}:00Z"),
        );
        HitlOutbox::stage(&env, &outbox_dir).unwrap();
    }

    // Approve even indices, reject odd indices
    for i in 0..10 {
        let id = format!("task_{i:02}");
        if i % 2 == 0 {
            HitlOutbox::approve(&id, &outbox_dir).unwrap();
        } else {
            HitlOutbox::reject(&id, &outbox_dir).unwrap();
        }
    }

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 10);

    for s in summaries {
        let num: usize = s.id[5..].parse().unwrap();
        if num.is_multiple_of(2) {
            assert_eq!(
                s.status,
                StagedIssueStatus::Approved,
                "Draft {} should be Approved",
                s.id
            );
        } else {
            assert_eq!(
                s.status,
                StagedIssueStatus::Rejected,
                "Draft {} should be Rejected",
                s.id
            );
        }
    }
}

// ============================================================================
// 9. Edge Cases: Corrupted Status Markers, Subsecond Sorting & Dirty Files
// ============================================================================

#[test]
fn test_corrupted_or_missing_status_markers_in_draft_file() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");
    fs::create_dir_all(&outbox_dir).unwrap();

    // 1. File with unrecognized status marker
    let corrupt_content =
        "# Staged Issue Draft: Corrupt\n- **ID**: task_bad\n- **Status**: UnknownState\n";
    fs::write(outbox_dir.join("issue_task_bad.md"), corrupt_content).unwrap();

    let app_err = HitlOutbox::approve("task_bad", &outbox_dir).unwrap_err();
    assert!(
        matches!(app_err, RouterError::OutboxError(msg) if msg.contains("Corrupted or missing status"))
    );

    let rej_err = HitlOutbox::reject("task_bad", &outbox_dir).unwrap_err();
    assert!(
        matches!(rej_err, RouterError::OutboxError(msg) if msg.contains("Corrupted or missing status"))
    );

    // 2. File with missing status marker entirely
    let missing_status =
        "# Staged Issue Draft: Missing\n- **ID**: task_no_stat\n- **Severity**: high\n";
    fs::write(outbox_dir.join("issue_task_no_stat.md"), missing_status).unwrap();

    let app_err2 = HitlOutbox::approve("task_no_stat", &outbox_dir).unwrap_err();
    assert!(matches!(app_err2, RouterError::OutboxError(_)));

    let rej_err2 = HitlOutbox::reject("task_no_stat", &outbox_dir).unwrap_err();
    assert!(matches!(rej_err2, RouterError::OutboxError(_)));
}

#[test]
fn test_deterministic_sorting_subsecond_timestamps() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");

    let subseconds = [
        ("task_sub_1", "2026-10-11T12:00:00.000000001Z"),
        ("task_sub_3", "2026-10-11T12:00:00.999999999Z"),
        ("task_sub_2", "2026-10-11T12:00:00.500000000Z"),
    ];

    for (id, ts) in &subseconds {
        let env = sample_external_envelope(id, id, ts);
        HitlOutbox::stage(&env, &outbox_dir).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 3);
    assert_eq!(summaries[0].id, "task_sub_3");
    assert_eq!(summaries[1].id, "task_sub_2");
    assert_eq!(summaries[2].id, "task_sub_1");
}

#[test]
fn test_list_staged_resilience_to_directory_with_subdirectories_and_zero_byte_files() {
    let temp_dir = tempdir().expect("tempdir");
    let outbox_dir = temp_dir.path().join("staged");
    fs::create_dir_all(&outbox_dir).unwrap();

    // Stage 1 valid draft
    let env = sample_external_envelope("task_good", "Good Issue", "2026-10-11T10:00:00Z");
    HitlOutbox::stage(&env, &outbox_dir).unwrap();

    // Dirty artifacts
    fs::create_dir_all(outbox_dir.join("issue_subfolder.md")).unwrap(); // directory disguised as .md
    fs::write(outbox_dir.join("issue_.md"), "empty id").unwrap(); // issue_.md with empty id
    fs::write(outbox_dir.join("issue_empty.md"), "").unwrap(); // 0 bytes
    fs::write(outbox_dir.join(".hidden_issue.md"), "secret").unwrap(); // dotfile
    fs::write(outbox_dir.join("notes.txt"), "hello").unwrap(); // non-md

    let summaries = HitlOutbox::list_staged(&outbox_dir).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "task_good");
}
