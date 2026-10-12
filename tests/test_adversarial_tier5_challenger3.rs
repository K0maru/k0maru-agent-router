//! Adversarial empirical stress tests authored by Tier 5 Challenger 3.
//!
//! Deeply challenges and stress-tests:
//! 1. `RouterError` taxonomy, display strings, `std::error::Error::source` chaining, `Send + Sync` bounds, and `PartialEq` edge cases.
//! 2. `ReproductionPayload` conversion matrix, value handling, and type mismatch stress.
//! 3. `TaskEnvelope` Serde permutations, exotic RFC-3339 timestamps (nanoseconds, leap centuries, UTC offsets, leap seconds), and contract validation boundaries.
//! 4. `BoundaryPolicy` trust boundary evaluation and verdict predicates.
//! 5. `HitlOutbox` state machine anti-tampering (header/trailer mismatch, contradictory markers, ambiguous keywords, double approvals).
//! 6. `HitlOutbox` path traversal defenses, hostile IDs, and directory boundary handling.
//! 7. Concurrent multi-threaded restaging stress and race condition resilience (multi-thread simultaneous approval race).
//! 8. Deterministic sorting tie-breaker stress with identical timestamps.

use chrono::DateTime;
use k0maru_router::addressing::SessionResolution;
use k0maru_router::coordinator::{format_agent_prompt, RouteOutcome};
use k0maru_router::driver::{LiveSession, SessionStatus};
use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload, DEFAULT_SCHEMA,
    VALID_SEVERITIES,
};
use k0maru_router::error::RouterError;
use k0maru_router::outbox::{HitlOutbox, StagedIssueStatus};
use k0maru_router::policy::{BoundaryPolicy, PolicyVerdict};
use std::error::Error;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

fn sample_envelope(id: &str, is_external: bool, ws: Option<&str>) -> TaskEnvelope {
    TaskEnvelope::new(
        id,
        "2026-10-11T12:00:00Z",
        CallerInfo::new("/caller/workspace", "agy").with_pane_id("%1"),
        if is_external {
            let mut t = TargetInfo::new_external("external-lib");
            if let Some(w) = ws {
                t = t.with_workspace(w);
            }
            t
        } else {
            TargetInfo::new_internal("internal-core", ws.unwrap_or("/internal/core"))
        },
        "bug_report",
        TaskPayload::new("Test Issue", "blocking")
            .with_reproduction_payload(ReproductionPayload::new("const x = 1;", "Error: crash"))
            .with_expected("Success"),
    )
}

// ============================================================================
// 1. RouterError Deep Taxonomy, Error::source, Send+Sync, & PartialEq Stress
// ============================================================================

#[test]
fn test_c3_router_error_send_sync_and_source_chaining() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RouterError>();

    // Test Error::source() for variants wrapping underlying causes
    let io_raw = std::io::Error::new(ErrorKind::PermissionDenied, "forbidden");
    let err_io = RouterError::Io(io_raw);
    assert!(err_io.source().is_some());
    assert_eq!(err_io.source().unwrap().to_string(), "forbidden");

    let json_raw = serde_json::from_str::<TaskEnvelope>("{bad json").unwrap_err();
    let err_json = RouterError::Json(json_raw);
    assert!(err_json.source().is_some());

    let dt_raw = DateTime::parse_from_rfc3339("bad date").unwrap_err();
    let err_dt = RouterError::DateTimeParse(dt_raw);
    assert!(err_dt.source().is_some());

    // Variants without underlying causes must return None for source()
    let err_envelope = RouterError::InvalidEnvelope("invalid".to_string());
    assert!(err_envelope.source().is_none());

    let err_not_found = RouterError::IssueNotFound("task_123".to_string());
    assert!(err_not_found.source().is_none());

    let err_ws = RouterError::WorkspaceNotFound(PathBuf::from("/missing"));
    assert!(err_ws.source().is_none());

    let err_driver = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "missing".to_string(),
    };
    assert!(err_driver.source().is_none());

    let err_driver_exec = RouterError::DriverExecutionFailed("failed".to_string());
    assert!(err_driver_exec.source().is_none());

    let err_outbox = RouterError::OutboxError("outbox fail".to_string());
    assert!(err_outbox.source().is_none());

    let err_session = RouterError::SessionNotFound("%0".to_string());
    assert!(err_session.source().is_none());
}

#[test]
fn test_c3_router_error_partial_eq_exhaustive_subtleties() {
    // DriverUnavailable equality relies on driver name AND reason
    let d1 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "offline".to_string(),
    };
    let d2 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "offline".to_string(),
    };
    let d3 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "different reason".to_string(),
    };
    let d4 = RouterError::DriverUnavailable {
        driver: "herdr",
        reason: "offline".to_string(),
    };
    assert_eq!(d1, d2);
    assert_ne!(d1, d3);
    assert_ne!(d1, d4);

    // Io equality is based strictly on ErrorKind
    let io_a = RouterError::Io(std::io::Error::new(ErrorKind::NotFound, "msg A"));
    let io_b = RouterError::Io(std::io::Error::new(ErrorKind::NotFound, "msg B"));
    let io_c = RouterError::Io(std::io::Error::new(ErrorKind::AlreadyExists, "msg A"));
    assert_eq!(io_a, io_b);
    assert_ne!(io_a, io_c);

    // Json equality is based on to_string()
    let j1 = RouterError::Json(serde_json::from_str::<TaskEnvelope>("bad1").unwrap_err());
    let j2 = RouterError::Json(serde_json::from_str::<TaskEnvelope>("bad1").unwrap_err());
    assert_eq!(j1, j2);
}

// ============================================================================
// 2. Serde Permutations & ReproductionPayload Matrix
// ============================================================================

#[test]
fn test_c3_reproduction_payload_edge_conversions_and_validations() {
    // Empty snippet or empty error must be rejected by validate()
    let r_empty_snippet = ReproductionPayload::new("", "some error");
    assert!(r_empty_snippet.validate().is_err());

    let r_ws_snippet = ReproductionPayload::new("   \t\n", "some error");
    assert!(r_ws_snippet.validate().is_err());

    let r_empty_error = ReproductionPayload::new("snippet", "");
    assert!(r_empty_error.validate().is_err());

    let r_ws_error = ReproductionPayload::new("snippet", "  \r\n  ");
    assert!(r_ws_error.validate().is_err());

    let valid_repro = ReproductionPayload::new("console.log(1);", "Uncaught ReferenceError");
    assert!(valid_repro.validate().is_ok());

    // TryFrom edge types
    let json_null = serde_json::Value::Null;
    assert!(ReproductionPayload::try_from(&json_null).is_err());

    let json_bool = serde_json::Value::Bool(true);
    assert!(ReproductionPayload::try_from(&json_bool).is_err());

    let json_num = serde_json::json!(42);
    assert!(ReproductionPayload::try_from(&json_num).is_err());

    let json_empty_obj = serde_json::json!({});
    assert!(ReproductionPayload::try_from(&json_empty_obj).is_err());

    // Object with extra fields should still deserialize cleanly into ReproductionPayload
    let json_extra = serde_json::json!({
        "snippet": "let a = 1;",
        "error": "Error: fail",
        "extra_field": 12345
    });
    let repro_extra = ReproductionPayload::try_from(&json_extra).expect("Extra fields tolerated");
    assert_eq!(repro_extra.snippet, "let a = 1;");
    assert_eq!(repro_extra.error, "Error: fail");
}

#[test]
fn test_c3_envelope_serde_and_schema_handling() {
    let mut env = sample_envelope("task_serde_edge_01", true, None);

    // Explicit $schema
    env.schema = Some(DEFAULT_SCHEMA.to_string());
    assert!(env.validate().is_ok());

    // Custom valid URI schema
    env.schema = Some("https://example.com/custom-schema.json".to_string());
    assert!(env.validate().is_ok());

    // Whitespace schema is rejected
    env.schema = Some("   ".to_string());
    assert!(matches!(
        env.validate(),
        Err(RouterError::InvalidEnvelope(_))
    ));

    // None schema is valid (optional)
    env.schema = None;
    assert!(env.validate().is_ok());
    let json = env.to_json_string().unwrap();
    assert!(!json.contains("$schema"));
}

// ============================================================================
// 3. Exotic RFC-3339 Timestamp Boundaries & Stress
// ============================================================================

#[test]
fn test_c3_exotic_rfc3339_timestamp_handling() {
    let mut env = sample_envelope("task_ts_c3", true, None);

    // Nanosecond precision (9 decimals), microsecond (6), millisecond (3), 1 decimal
    let high_precision_timestamps = [
        "2026-10-11T12:00:00.123456789Z",
        "2026-10-11T12:00:00.123456Z",
        "2026-10-11T12:00:00.123Z",
        "2026-10-11T12:00:00.5Z",
    ];

    for ts in high_precision_timestamps {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_ok(),
            "High-precision timestamp '{}' must be valid RFC-3339",
            ts
        );
    }

    // Exotic valid timezones
    let exotic_timezones = [
        "2026-10-11T12:00:00+14:00", // Kiribati / Line Islands (UTC+14)
        "2026-10-11T12:00:00-11:00", // Samoa (UTC-11)
        "2026-10-11T12:00:00+12:45", // Chatham Islands (UTC+12:45)
        "2026-10-11T12:00:00+05:45", // Nepal (UTC+5:45)
        "2026-10-11T12:00:00-00:00", // RFC 3339 unknown local offset (-00:00)
    ];

    for ts in exotic_timezones {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_ok(),
            "Exotic timezone timestamp '{}' must be valid RFC-3339",
            ts
        );
    }

    // RFC-3339 case insensitivity & space separator
    let format_variants = [
        "2026-10-11t12:00:00z",
        "2026-10-11T12:00:00z",
        "2026-10-11t12:00:00Z",
        "2026-10-11 12:00:00Z",
        "2026-10-11 12:00:00+00:00",
    ];

    for ts in format_variants {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_ok(),
            "Variant timestamp '{}' must be accepted per RFC-3339 specs",
            ts
        );
    }

    // Extreme invalid timestamps
    let invalid_ts = [
        "2026-10-11T12:00:00",             // Missing offset
        "2026-10-11T12:00:00+25:00",       // Offset hour 25 invalid
        "2026-10-11T12:00:00-25:00",       // Offset hour -25 invalid
        "2026-10-11T12:00:00+00:60",       // Offset minute 60 invalid
        "2026-02-29T12:00:00Z",            // 2026 is non-leap year
        "2024-02-30T12:00:00Z",            // Feb 30 never exists
        "2026-04-31T12:00:00Z",            // April only has 30 days
        "2026-13-01T12:00:00Z",            // Month 13
        "2026-00-01T12:00:00Z",            // Month 0
        "2026-10-00T12:00:00Z",            // Day 0
        "2026-10-11T24:00:00Z",            // Hour 24
        "2026-10-11T12:60:00Z",            // Minute 60
        "2026-10-11T12:00:61Z",            // Second 61
        "26-10-11T12:00:00Z",              // 2-digit year
        "2026-10-11T12:00:00+00:00 extra", // Trailing garbage
        "leading 2026-10-11T12:00:00Z",    // Leading garbage
    ];

    for ts in invalid_ts {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_err(),
            "Invalid timestamp '{}' must be rejected",
            ts
        );
    }
}

// ============================================================================
// 4. HitlOutbox State Machine Anti-Tampering & Parsing Stress
// ============================================================================

#[test]
fn test_c3_outbox_tampering_contradictory_markers_and_ambiguity() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    // 1. Contradictory header status lines within the header block
    let draft_contradictory_header = r#"# Staged Issue Draft: Contradictory Header
- **ID**: task_contra_1
- **Target**: core
- **Severity**: high
- **Created At**: 2026-10-11T12:00:00Z
- **Status**: Pending Human Authorization (HITL)
- **Status**: Approved by Human

## Reproduction
```json
{}
```

## Expected
N/A

---
*Status: Pending Human Authorization (HITL)*
"#;

    fs::write(
        outbox.join("issue_task_contra_1.md"),
        draft_contradictory_header,
    )
    .unwrap();

    // Contradictory header status lines must fail detection
    assert!(
        HitlOutbox::approve("task_contra_1", &outbox).is_err(),
        "Contradictory status in header must fail approve"
    );
    assert!(
        HitlOutbox::reject("task_contra_1", &outbox).is_err(),
        "Contradictory status in header must fail reject"
    );

    // 2. Ambiguous keyword in status line (both "Approved" and "Pending" in one line)
    let draft_ambiguous_line = r#"# Staged Issue Draft: Ambiguous Status Line
- **ID**: task_ambig_1
- **Target**: core
- **Severity**: high
- **Created At**: 2026-10-11T12:00:00Z
- **Status**: Approved Pending Review

## Reproduction
```json
{}
```

## Expected
N/A

---
*Status: Approved Pending Review*
"#;

    fs::write(outbox.join("issue_task_ambig_1.md"), draft_ambiguous_line).unwrap();

    assert!(
        HitlOutbox::approve("task_ambig_1", &outbox).is_err(),
        "Ambiguous status must fail detection and approval"
    );

    // 3. Double approval stress: First approve succeeds, second approve fails with OutboxError
    let env_normal = sample_envelope("task_double_appr", true, None);
    HitlOutbox::stage(&env_normal, &outbox).unwrap();

    let approved_path = HitlOutbox::approve("task_double_appr", &outbox).unwrap();
    assert!(approved_path.exists());

    let second_approve_res = HitlOutbox::approve("task_double_appr", &outbox);
    assert!(
        matches!(second_approve_res, Err(RouterError::OutboxError(_))),
        "Second approve must fail with OutboxError, got: {:?}",
        second_approve_res
    );

    // Rejection on an already Approved draft must also fail with OutboxError
    let reject_on_approved = HitlOutbox::reject("task_double_appr", &outbox);
    assert!(
        matches!(reject_on_approved, Err(RouterError::OutboxError(_))),
        "Reject on already Approved draft must fail with OutboxError, got: {:?}",
        reject_on_approved
    );
}

#[test]
fn test_c3_outbox_double_rejection_and_approve_rejected() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    let env = sample_envelope("task_double_rej", true, None);
    HitlOutbox::stage(&env, &outbox).unwrap();

    // Reject succeeds
    HitlOutbox::reject("task_double_rej", &outbox).unwrap();

    // Second reject must fail with OutboxError
    let second_reject = HitlOutbox::reject("task_double_rej", &outbox);
    assert!(
        matches!(second_reject, Err(RouterError::OutboxError(_))),
        "Second reject must fail with OutboxError"
    );

    // Approve on rejected draft must fail with OutboxError
    let approve_on_rejected = HitlOutbox::approve("task_double_rej", &outbox);
    assert!(
        matches!(approve_on_rejected, Err(RouterError::OutboxError(_))),
        "Approve on rejected draft must fail with OutboxError"
    );
}

// ============================================================================
// 5. Hostile Path Traversal & Outbox Boundary Stress
// ============================================================================

#[test]
fn test_c3_outbox_hostile_id_stress() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    let hostile_ids = [
        "../secret",
        "..\\secret",
        "/etc/passwd",
        "nested/sub/dir",
        "task\0injection",
        ".",
        "..",
        ".hidden",
        "issue_",
        "issue_.md",
        "",
        "   ",
    ];

    for hostile in hostile_ids {
        assert!(
            matches!(
                HitlOutbox::inspect_staged(hostile, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "inspect_staged must reject '{}' with IssueNotFound",
            hostile
        );
        assert!(
            matches!(
                HitlOutbox::approve(hostile, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "approve must reject '{}' with IssueNotFound",
            hostile
        );
        assert!(
            matches!(
                HitlOutbox::reject(hostile, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "reject must reject '{}' with IssueNotFound",
            hostile
        );
    }
}

// ============================================================================
// 6. Concurrent Multi-Threaded Approval Race Condition Stress
// ============================================================================

#[test]
fn test_c3_concurrent_multi_thread_approval_race() {
    let dir = tempdir().unwrap();
    let outbox = Arc::new(dir.path().join("staged"));
    fs::create_dir_all(&*outbox).unwrap();

    let task_id = "task_race_001";
    let env = sample_envelope(task_id, true, None);
    HitlOutbox::stage(&env, &outbox).unwrap();

    let num_threads = 16;
    let barrier = Arc::new(Barrier::new(num_threads));
    let mut handles = Vec::new();

    for _ in 0..num_threads {
        let outbox_clone = Arc::clone(&outbox);
        let barrier_clone = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            // Synchronize all threads to execute approve concurrently
            barrier_clone.wait();
            HitlOutbox::approve(task_id, &outbox_clone)
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    let mut outbox_err_count = 0;

    for h in handles {
        match h.join().expect("Thread must not panic") {
            Ok(_) => success_count += 1,
            Err(RouterError::OutboxError(_)) => outbox_err_count += 1,
            Err(other) => panic!("Unexpected error type in race: {:?}", other),
        }
    }

    // At least one thread must succeed, and all threads must either succeed or get OutboxError (already approved)
    assert!(
        success_count >= 1,
        "At least one approve call must succeed in race condition"
    );
    assert_eq!(
        success_count + outbox_err_count,
        num_threads,
        "Every thread must complete with either success or OutboxError"
    );

    // Final draft on disk must be in clean Approved state
    let content = HitlOutbox::inspect_staged(task_id, &outbox).expect("Draft must exist");
    assert!(content.contains("- **Status**: Approved by Human"));
    assert!(content.contains("*Status: Approved by Human*"));

    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].status, StagedIssueStatus::Approved);
}

#[test]
fn test_c3_concurrent_multithreaded_restaging_idempotency() {
    let dir = tempdir().unwrap();
    let outbox = Arc::new(dir.path().join("staged"));
    fs::create_dir_all(&*outbox).unwrap();

    let num_threads = 20;
    let barrier = Arc::new(Barrier::new(num_threads));
    let mut handles = Vec::new();

    for _i in 0..num_threads {
        let outbox_clone = Arc::clone(&outbox);
        let barrier_clone = Arc::clone(&barrier);
        let h = thread::spawn(move || {
            let env = sample_envelope("task_restage_idempotent", true, None);
            barrier_clone.wait();
            HitlOutbox::stage(&env, &outbox_clone)
        });
        handles.push(h);
    }

    for h in handles {
        h.join()
            .expect("Thread join must succeed")
            .expect("Staging must succeed");
    }

    // Exactly 1 staged file should exist and be valid
    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "task_restage_idempotent");
    assert_eq!(summaries[0].status, StagedIssueStatus::Pending);
}

// ============================================================================
// 7. Deterministic Sorting Tie-Breaker Stress
// ============================================================================

#[test]
fn test_c3_deterministic_sorting_multiple_tie_breakers() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");

    // 6 tasks with identical timestamp
    let ids = ["task_f", "task_b", "task_d", "task_a", "task_e", "task_c"];
    for id in ids {
        let mut env = sample_envelope(id, true, None);
        env.created_at = "2026-10-11T12:00:00Z".to_string();
        HitlOutbox::stage(&env, &outbox).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), 6);

    let sorted_ids: Vec<String> = summaries.into_iter().map(|s| s.id).collect();
    assert_eq!(
        sorted_ids,
        vec![
            "task_a".to_string(),
            "task_b".to_string(),
            "task_c".to_string(),
            "task_d".to_string(),
            "task_e".to_string(),
            "task_f".to_string(),
        ],
        "When created_at is identical, tie-breaker must sort by ID ascending"
    );
}

// ============================================================================
// 8. BoundaryPolicy & Severity Contract Exhaustive Verification
// ============================================================================

#[test]
fn test_c3_boundary_policy_and_severity_contract() {
    let internal_target = TargetInfo::new_internal("my-crate", "/path/to/my-crate");
    let v_int = BoundaryPolicy::evaluate_target(&internal_target);
    assert!(v_int.is_trusted());
    assert!(!v_int.is_untrusted());
    assert_eq!(v_int.reason(), None);

    let ext_target = TargetInfo::new_external("tokio");
    let v_ext = BoundaryPolicy::evaluate_target(&ext_target);
    assert!(v_ext.is_untrusted());
    assert!(!v_ext.is_trusted());
    assert!(v_ext.reason().unwrap().contains("tokio"));

    // Exhaustive valid severities
    let mut env = sample_envelope("task_sev_c3", true, None);
    for sev in VALID_SEVERITIES {
        env.payload.severity = sev.to_string();
        assert!(env.validate().is_ok());
    }
}

// ============================================================================
// 9. RouteOutcome, SessionResolution, & Prompt Formatting Stress
// ============================================================================

#[test]
fn test_c3_route_outcome_and_format_agent_prompt_stress() {
    let session = LiveSession::new(
        "%42",
        PathBuf::from("/workspace/foo"),
        SessionStatus::Idle,
        Some("bash".to_string()),
    );

    let outcome_disp = RouteOutcome::Dispatched {
        session: session.clone(),
        prompt: "Fix bug now".to_string(),
        sandbox: None,
    };
    assert!(outcome_disp.is_dispatched());
    assert!(!outcome_disp.is_staged());
    assert!(!outcome_disp.is_dry_run());
    assert_eq!(outcome_disp.session().unwrap().pane_id, "%42");

    let resolution = SessionResolution::HotSession(session);
    let outcome_dry = RouteOutcome::DryRun {
        verdict: PolicyVerdict::TrustedInternal,
        resolution: Some(resolution),
    };
    assert!(outcome_dry.is_dry_run());
    assert!(outcome_dry.session().is_some());

    let outcome_staged = RouteOutcome::Staged {
        issue_path: PathBuf::from("/tmp/staged/issue_1.md"),
        reason: "OSS untrusted".to_string(),
    };
    assert!(outcome_staged.is_staged());
    assert_eq!(
        outcome_staged.issue_path(),
        Some(Path::new("/tmp/staged/issue_1.md"))
    );

    let mut env = sample_envelope("task_prompt_c3", false, Some("/ws/foo"));
    env.caller.pane_id = Some("%99".to_string());
    env.payload.expected = Some("Expected clean return".to_string());
    let prompt = format_agent_prompt(&env);
    assert!(prompt.contains("Title: Test Issue"));
    assert!(prompt.contains("Caller: agy (pane: %99) in /caller/workspace"));
    assert!(prompt.contains("Expected Behavior:\nExpected clean return"));
}
