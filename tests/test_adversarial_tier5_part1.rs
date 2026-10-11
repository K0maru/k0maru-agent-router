//! Tier 5 Part 1: White-Box Adversarial Coverage Hardening Suite
//!
//! Covers:
//! 1. `RouterError` exhaustive taxonomy, display formatting, conversions, and PartialEq matrix.
//! 2. `ReproductionPayload` conversion matrix, value handling, and type mismatch stress.
//! 3. `TaskEnvelope` Serde permutations, schema validation, and exotic RFC-3339 timestamps.
//! 4. `BoundaryPolicy` trust boundary evaluation and verdict predicates.
//! 5. `HitlOutbox` path traversal, normalization variants, and directory creation boundaries.
//! 6. `HitlOutbox` state machine anti-tampering, corrupted headers/trailers, and markdown parsing fallbacks.
//! 7. Concurrent multi-threaded restaging and deterministic sorting tie-breaker stress.
//! 8. `format_agent_prompt` payload permutations and Markdown block generation.
//! 9. `RouterCoordinator` & `RouteOutcome` dry-run, accessors, and system metrics.

use chrono::DateTime;
use k0maru_router::addressing::SessionResolution;
use k0maru_router::coordinator::{
    format_agent_prompt, DriverStatusSummary, OutboxStatusSummary, RouteOutcome, RouterCoordinator,
    SystemStatusSummary,
};
use k0maru_router::driver::{LiveSession, MultiplexerDriver, SessionStatus};
use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload, DEFAULT_SCHEMA,
    VALID_SEVERITIES,
};
use k0maru_router::error::RouterError;
use k0maru_router::outbox::{HitlOutbox, StagedIssueStatus};
use k0maru_router::policy::{BoundaryPolicy, PolicyVerdict};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use tempfile::tempdir;

fn sample_base_envelope(id: &str, is_external: bool, ws: Option<&str>) -> TaskEnvelope {
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

#[derive(Debug, Default)]
struct MockDriver {
    pub sessions: Mutex<Vec<LiveSession>>,
    pub sent_prompts: Mutex<Vec<(String, String)>>,
    pub spawned_panes: Mutex<Vec<(PathBuf, Option<String>)>>,
    pub available: bool,
}

impl MockDriver {
    fn new_available() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            sent_prompts: Mutex::new(Vec::new()),
            spawned_panes: Mutex::new(Vec::new()),
            available: true,
        }
    }
}

impl MultiplexerDriver for MockDriver {
    fn name(&self) -> &'static str {
        "mock-mux"
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
        if !self.available {
            return Err(RouterError::DriverUnavailable {
                driver: "mock-mux",
                reason: "Driver is offline".to_string(),
            });
        }
        Ok(self.sessions.lock().unwrap().clone())
    }

    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
        self.sent_prompts
            .lock()
            .unwrap()
            .push((pane_id.to_string(), prompt.to_string()));
        Ok(())
    }

    fn spawn_pane(
        &self,
        workspace: &Path,
        command: Option<&str>,
    ) -> Result<LiveSession, RouterError> {
        self.spawned_panes
            .lock()
            .unwrap()
            .push((workspace.to_path_buf(), command.map(String::from)));
        let session = LiveSession::new(
            "%100",
            workspace.to_path_buf(),
            SessionStatus::Idle,
            Some("zsh".into()),
        );
        self.sessions.lock().unwrap().push(session.clone());
        Ok(session)
    }
}

// ============================================================================
// Section 1: RouterError Taxonomy, Formatting, Conversions & PartialEq Matrix
// ============================================================================

#[test]
fn test_tier5_router_error_display_formatting_and_taxonomy() {
    let err_invalid_env = RouterError::InvalidEnvelope("missing title".to_string());
    assert_eq!(
        err_invalid_env.to_string(),
        "Invalid task envelope: missing title"
    );

    let err_issue_nf = RouterError::IssueNotFound("task_404".to_string());
    assert_eq!(
        err_issue_nf.to_string(),
        "Staged issue not found: 'task_404'"
    );

    let err_ws_nf = RouterError::WorkspaceNotFound(PathBuf::from("/nonexistent/ws"));
    assert_eq!(
        err_ws_nf.to_string(),
        "Target workspace not found: '/nonexistent/ws'"
    );

    let err_driver_unavail = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "binary not in PATH".to_string(),
    };
    assert_eq!(
        err_driver_unavail.to_string(),
        "Multiplexer driver 'tmux' unavailable: binary not in PATH"
    );

    let err_driver_exec = RouterError::DriverExecutionFailed("exit code 127".to_string());
    assert_eq!(
        err_driver_exec.to_string(),
        "Multiplexer driver execution failed: exit code 127"
    );

    let err_outbox = RouterError::OutboxError("permission denied".to_string());
    assert_eq!(
        err_outbox.to_string(),
        "HITL Outbox error: permission denied"
    );

    let err_session_nf = RouterError::SessionNotFound("%42".to_string());
    assert_eq!(err_session_nf.to_string(), "Session not found: %42");

    let io_raw = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err_io = RouterError::Io(io_raw);
    assert_eq!(err_io.to_string(), "Filesystem IO error: file not found");

    let json_raw = serde_json::from_str::<TaskEnvelope>("{").unwrap_err();
    let err_json = RouterError::Json(json_raw);
    assert!(err_json
        .to_string()
        .starts_with("JSON serialization/deserialization error:"));

    let dt_raw = DateTime::parse_from_rfc3339("invalid-date").unwrap_err();
    let err_dt = RouterError::DateTimeParse(dt_raw);
    assert!(err_dt.to_string().starts_with("Timestamp parsing error:"));
}

#[test]
fn test_tier5_router_error_from_conversions() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let router_io: RouterError = io_err.into();
    assert!(matches!(router_io, RouterError::Io(_)));

    let json_err = serde_json::from_str::<TaskEnvelope>("not json").unwrap_err();
    let router_json: RouterError = json_err.into();
    assert!(matches!(router_json, RouterError::Json(_)));

    let dt_err = DateTime::parse_from_rfc3339("2026-99-99T00:00:00Z").unwrap_err();
    let router_dt: RouterError = dt_err.into();
    assert!(matches!(router_dt, RouterError::DateTimeParse(_)));
}

#[test]
fn test_tier5_router_error_partial_eq_matrix() {
    let e1 = RouterError::InvalidEnvelope("errA".to_string());
    let e2 = RouterError::InvalidEnvelope("errA".to_string());
    let e3 = RouterError::InvalidEnvelope("errB".to_string());
    assert_eq!(e1, e2);
    assert_ne!(e1, e3);

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
        reason: "busy".to_string(),
    };
    let d4 = RouterError::DriverUnavailable {
        driver: "herdr",
        reason: "offline".to_string(),
    };
    assert_eq!(d1, d2);
    assert_ne!(d1, d3);
    assert_ne!(d1, d4);

    let io1 = RouterError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "a"));
    let io2 = RouterError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "b"));
    let io3 = RouterError::Io(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "a",
    ));
    assert_eq!(io1, io2);
    assert_ne!(io1, io3);

    let all_variants: Vec<RouterError> = vec![
        RouterError::InvalidEnvelope("x".into()),
        RouterError::IssueNotFound("x".into()),
        RouterError::WorkspaceNotFound(PathBuf::from("/x")),
        RouterError::DriverUnavailable {
            driver: "tmux",
            reason: "x".into(),
        },
        RouterError::DriverExecutionFailed("x".into()),
        RouterError::OutboxError("x".into()),
        RouterError::SessionNotFound("x".into()),
        RouterError::Io(std::io::Error::other("x")),
        RouterError::Json(serde_json::from_str::<TaskEnvelope>("x").unwrap_err()),
        RouterError::DateTimeParse(DateTime::parse_from_rfc3339("x").unwrap_err()),
    ];

    for i in 0..all_variants.len() {
        for j in 0..all_variants.len() {
            if i != j {
                assert_ne!(
                    all_variants[i], all_variants[j],
                    "Variant at index {i} must not equal variant at index {j}"
                );
            } else {
                assert_eq!(all_variants[i], all_variants[j]);
            }
        }
    }
}

// ============================================================================
// Section 2: ReproductionPayload Conversion Matrix & Type Mismatch Stress
// ============================================================================

#[test]
fn test_tier5_reproduction_payload_try_from_value_matrix() {
    let repro = ReproductionPayload::new("<div>test</div>", "Assertion error");
    assert!(repro.validate().is_ok());

    let val = repro.to_value().expect("to_value must succeed");
    let from_val = ReproductionPayload::try_from(val).expect("try_from(Value) must succeed");
    assert_eq!(repro, from_val);

    let ref_val = repro.to_value().unwrap();
    let from_ref = ReproductionPayload::try_from(&ref_val).expect("try_from(&Value) must succeed");
    assert_eq!(repro, from_ref);

    let bad_values = vec![
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!(12345),
        serde_json::json!(42.5),
        serde_json::json!("raw string snippet"),
        serde_json::json!(["array", "of", "items"]),
        serde_json::json!({ "snippet": "valid" }),
        serde_json::json!({ "error": "valid" }),
        serde_json::json!({ "snippet": 123, "error": "valid" }),
        serde_json::json!({ "snippet": "valid", "error": [1, 2] }),
    ];

    for bad in bad_values {
        assert!(
            ReproductionPayload::try_from(&bad).is_err(),
            "ReproductionPayload::try_from(&Value) must fail on: {:?}",
            bad
        );
        assert!(
            ReproductionPayload::try_from(bad.clone()).is_err(),
            "ReproductionPayload::try_from(Value) must fail on: {:?}",
            bad
        );
        assert!(
            ReproductionPayload::from_value(&bad).is_err(),
            "ReproductionPayload::from_value must fail on: {:?}",
            bad
        );
    }
}

#[test]
fn test_tier5_task_payload_reproduction_value_handling() {
    let payload_none = TaskPayload::new("Test", "medium");
    assert!(payload_none.reproduction_payload().is_none());

    let payload_typed = TaskPayload::new("Test", "medium")
        .with_reproduction_payload(ReproductionPayload::new("code", "err"));
    let extracted = payload_typed
        .reproduction_payload()
        .expect("must be Some")
        .expect("must parse cleanly");
    assert_eq!(extracted.snippet, "code");
    assert_eq!(extracted.error, "err");

    let payload_arbitrary =
        TaskPayload::new("Test", "medium").with_reproduction(serde_json::json!({
            "exit_code": 1,
            "stdout": "failed",
        }));
    assert!(
        payload_arbitrary.validate().is_ok(),
        "Arbitrary JSON reproduction must pass TaskPayload validation"
    );
    let repro_res = payload_arbitrary.reproduction_payload().unwrap();
    assert!(
        repro_res.is_err(),
        "Arbitrary object missing snippet/error fails typed parsing"
    );

    let payload_array =
        TaskPayload::new("Test", "medium").with_reproduction(serde_json::json!([1, 2, 3]));
    assert!(payload_array.validate().is_ok());
    assert!(payload_array.reproduction_payload().unwrap().is_err());

    let payload_null =
        TaskPayload::new("Test", "medium").with_reproduction(serde_json::Value::Null);
    assert!(payload_null.validate().is_ok());
    assert!(payload_null.reproduction_payload().unwrap().is_err());

    let payload_bad_snippet =
        TaskPayload::new("Test", "medium").with_reproduction(serde_json::json!({
            "snippet": "",
            "error": "some error"
        }));
    assert!(matches!(
        payload_bad_snippet.validate(),
        Err(RouterError::InvalidEnvelope(_))
    ));

    let payload_bad_error =
        TaskPayload::new("Test", "medium").with_reproduction(serde_json::json!({
            "snippet": "some snippet",
            "error": "   \t\n  "
        }));
    assert!(matches!(
        payload_bad_error.validate(),
        Err(RouterError::InvalidEnvelope(_))
    ));
}

// ============================================================================
// Section 3: TaskEnvelope Serde Permutations & RFC-3339 Timestamp Boundaries
// ============================================================================

#[test]
fn test_tier5_envelope_serde_unknown_fields_and_optional_omissions() {
    let json_with_unknowns = r#"{
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_unknown_fields_001",
        "created_at": "2026-10-11T12:00:00Z",
        "extra_top_level": 42,
        "caller": {
            "workspace": "/tmp/ws",
            "agent": "caller",
            "future_metadata": { "pid": 1234 }
        },
        "target": {
            "identifier": "ext",
            "is_external": true,
            "unrecognized_flag": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Bug",
            "severity": "low",
            "custom_tag": "ui-team"
        }
    }"#;

    let envelope = TaskEnvelope::from_json_str(json_with_unknowns)
        .expect("TaskEnvelope must tolerate unknown fields across all nested structs");
    assert_eq!(envelope.id, "task_unknown_fields_001");
    assert_eq!(envelope.schema.as_deref(), Some(DEFAULT_SCHEMA));
    assert!(envelope.target.is_external);

    let serialized = envelope.to_json_string().unwrap();
    assert!(!serialized.contains("extra_top_level"));
    assert!(!serialized.contains("future_metadata"));
    assert!(!serialized.contains("unrecognized_flag"));
    assert!(!serialized.contains("custom_tag"));

    let pretty = envelope.to_json_pretty().unwrap();
    let deserialized_pretty = TaskEnvelope::from_json_str(&pretty).unwrap();
    assert_eq!(envelope, deserialized_pretty);
}

#[test]
fn test_tier5_envelope_serde_explicit_nulls_for_optionals() {
    let json_explicit_nulls = r#"{
        "$schema": null,
        "id": "task_explicit_nulls",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": {
            "workspace": "/tmp/ws",
            "agent": "worker",
            "pane_id": null
        },
        "target": {
            "identifier": "ext-pkg",
            "workspace": null,
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Explicit Nulls Test",
            "severity": "normal",
            "reproduction": null,
            "expected": null
        }
    }"#;

    let envelope = TaskEnvelope::from_json_str(json_explicit_nulls)
        .expect("Explicit nulls for optional fields must deserialize cleanly");
    assert_eq!(envelope.schema, None);
    assert_eq!(envelope.caller.pane_id, None);
    assert_eq!(envelope.target.workspace, None);
    assert_eq!(envelope.payload.reproduction, None);
    assert_eq!(envelope.payload.expected, None);
    assert!(envelope.validate().is_ok());
}

#[test]
fn test_tier5_envelope_rfc3339_timestamp_edge_cases() {
    let mut env = sample_base_envelope("task_ts_edge", true, None);

    let valid_timestamps = [
        "0001-01-01T00:00:00Z",
        "9999-12-31T23:59:59.999999999Z",
        "2000-02-29T12:00:00Z", // Year 2000 was leap century
        "2026-10-11T12:00:00.1Z",
        "2026-10-11T12:00:00.123456Z",
        "2026-10-11T12:00:00+14:00",
        "2026-10-11T12:00:00-12:00",
        "2026-10-11T12:00:00+00:00",
        "2026-10-11T12:00:00-00:00",
        "2026-10-11T12:00:00+05:30",
        "2026-10-11T12:00:00+05:45",
        "2026-10-11t12:00:00z", // Lowercase T and Z are valid RFC-3339
        "2026-10-11 12:00:00Z", // Space separator is permitted in RFC-3339 §5.6
    ];

    for ts in valid_timestamps {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_ok(),
            "Timestamp '{}' must be accepted as valid RFC-3339",
            ts
        );
    }

    let invalid_timestamps = [
        "1900-02-29T00:00:00Z",      // Year 1900 was not a leap year
        "2026-02-29T00:00:00Z",      // Year 2026 is not a leap year
        "2026-10-11T24:00:00Z",      // Hour 24 invalid
        "2026-10-11T12:60:00Z",      // Minute 60 invalid
        "2026-10-11T12:00:61Z",      // Second 61 invalid
        "2026-10-11T12:00:00+25:00", // Offset hour 25 invalid
        "2026-10-11T12:00:00-25:00",
        "2026-10-11T12:00:00+00:60",     // Offset minute 60 invalid
        "2026-10-11T12:00:00",           // Missing timezone offset
        "2026/10/11T12:00:00Z",          // Slashes in date
        "2026-1-1T12:00:00Z",            // Unpadded month/day
        "Sun, 11 Oct 2026 12:00:00 GMT", // RFC-2822 instead of RFC-3339
        "1697025600",                    // Unix epoch timestamp string
        "",
        "   ",
    ];

    for ts in invalid_timestamps {
        env.created_at = ts.to_string();
        assert!(
            env.validate().is_err(),
            "Timestamp '{}' must be rejected as invalid RFC-3339",
            ts
        );
    }
}

#[test]
fn test_tier5_envelope_all_valid_severities_exhaustively() {
    let mut env = sample_base_envelope("task_sev_test", true, None);

    for sev in VALID_SEVERITIES {
        env.payload.severity = sev.to_string();
        assert!(env.validate().is_ok());

        env.payload.severity = sev.to_uppercase();
        assert!(env.validate().is_ok());

        env.payload.severity = format!("  {sev}  ");
        assert!(env.validate().is_ok());
    }

    let bad_severities = ["urgent", "catastrophic", "p0", "p1", "fatal", "blocker", ""];
    for bad in bad_severities {
        env.payload.severity = bad.to_string();
        assert!(matches!(
            env.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
    }
}

// ============================================================================
// Section 4: BoundaryPolicy Trust Boundaries & Verdict Predicates
// ============================================================================

#[test]
fn test_tier5_boundary_policy_target_evaluation_and_verdict_predicates() {
    let internal_target = TargetInfo::new_internal("core", "/workspace/core");
    let verdict_internal = BoundaryPolicy::evaluate_target(&internal_target);
    assert!(verdict_internal.is_trusted());
    assert!(!verdict_internal.is_untrusted());
    assert_eq!(verdict_internal.reason(), None);
    assert_eq!(verdict_internal.to_string(), "TrustedInternal");

    let external_target = TargetInfo::new_external("react");
    let verdict_external = BoundaryPolicy::evaluate_target(&external_target);
    assert!(verdict_external.is_untrusted());
    assert!(!verdict_external.is_trusted());
    let reason = verdict_external.reason().unwrap();
    assert!(reason.contains("react"));
    assert!(reason.contains("external 3rd-party OSS"));
    assert_eq!(
        verdict_external.to_string(),
        format!("UntrustedExternal ({reason})")
    );

    let external_with_ws = TargetInfo::new_external("vue").with_workspace("/tmp/cloned-vue-repo");
    let verdict_with_ws = BoundaryPolicy::evaluate_target(&external_with_ws);
    assert!(
        verdict_with_ws.is_untrusted(),
        "Target flagged external with local workspace MUST remain UntrustedExternal"
    );

    let env_internal = sample_base_envelope("env_int", false, Some("/workspace/core"));
    assert!(BoundaryPolicy::evaluate(&env_internal).is_trusted());

    let env_external = sample_base_envelope("env_ext", true, None);
    assert!(BoundaryPolicy::evaluate(&env_external).is_untrusted());
}

// ============================================================================
// Section 5: HitlOutbox Path Traversal, Normalization & Directory Boundaries
// ============================================================================

#[test]
fn test_tier5_outbox_path_traversal_hostile_id_resolution() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    let hostile_query_ids = [
        "../../etc/passwd",
        "../../etc/shadow",
        "..\\..\\windows\\system32",
        "/etc/passwd",
        "C:\\boot.ini",
        ".",
        "..",
        ".hidden",
        "issue_",
        "issue_.md",
        ".md",
        "issue_..md",
        "task/slash",
        "task\\backslash",
        "task\0null",
        "",
        "   ",
        "\t\r\n",
    ];

    for bad_id in hostile_query_ids {
        assert!(
            matches!(
                HitlOutbox::inspect_staged(bad_id, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "inspect_staged must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );
        assert!(
            matches!(
                HitlOutbox::approve(bad_id, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "approve must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );
        assert!(
            matches!(
                HitlOutbox::reject(bad_id, &outbox),
                Err(RouterError::IssueNotFound(_))
            ),
            "reject must reject hostile ID '{}' with IssueNotFound",
            bad_id
        );
    }
}

#[test]
fn test_tier5_outbox_stage_directory_boundary_errors() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("not_a_directory.txt");
    fs::write(&file_path, "I am a file, not a directory").unwrap();

    let env = sample_base_envelope("task_stage_file_err", true, None);
    let stage_err = HitlOutbox::stage(&env, &file_path).unwrap_err();
    assert!(
        matches!(stage_err, RouterError::OutboxError(_)),
        "HitlOutbox::stage when outbox_dir is a file must return OutboxError, got: {:?}",
        stage_err
    );

    let deep_nested = dir
        .path()
        .join("level1")
        .join("level2")
        .join("level3")
        .join("staged");
    let staged_path =
        HitlOutbox::stage(&env, &deep_nested).expect("Must auto-create deep nested directory");
    assert!(staged_path.exists());
    assert!(staged_path.is_file());
}

#[test]
fn test_tier5_outbox_stage_newline_sanitization_in_headers() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");

    let mut env = sample_base_envelope("task_newlines_test", true, None);
    env.payload.title = "Multiline\r\nTitle\nAttack".to_string();
    env.target.identifier = "Multi\nLine\rTarget".to_string();
    env.payload.severity = "\r\nblocking\n".to_string();

    let path = HitlOutbox::stage(&env, &outbox).expect("Stage must succeed");
    let content = fs::read_to_string(path).unwrap();

    let title_line = content
        .lines()
        .find(|l| l.starts_with("# Staged Issue Draft:"));
    assert!(title_line.is_some());
    assert_eq!(
        title_line.unwrap(),
        "# Staged Issue Draft: Multiline  Title Attack"
    );

    let target_line = content.lines().find(|l| l.starts_with("- **Target**:"));
    assert_eq!(target_line.unwrap(), "- **Target**: Multi Line Target");

    let severity_line = content.lines().find(|l| l.starts_with("- **Severity**:"));
    assert_eq!(severity_line.unwrap(), "- **Severity**:   blocking ");
}

// ============================================================================
// Section 6: HitlOutbox State Machine Anti-Tampering & Markdown Fallbacks
// ============================================================================

#[test]
fn test_tier5_outbox_tampered_header_trailer_mismatch_fails_authorization() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    let draft_mismatch = r#"# Staged Issue Draft: Tampered Issue

- **ID**: task_tamper_1
- **Target**: terra-ui
- **Severity**: high
- **Created At**: 2026-10-11T12:00:00Z
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

    fs::write(outbox.join("issue_task_tamper_1.md"), draft_mismatch).unwrap();

    let approve_err = HitlOutbox::approve("task_tamper_1", &outbox).unwrap_err();
    assert!(
        matches!(approve_err, RouterError::OutboxError(_)),
        "Approve on tampered draft must fail with OutboxError"
    );

    let reject_err = HitlOutbox::reject("task_tamper_1", &outbox).unwrap_err();
    assert!(
        matches!(reject_err, RouterError::OutboxError(_)),
        "Reject on tampered draft must fail with OutboxError"
    );

    let draft_missing_status = r#"# Staged Issue Draft: Missing Status

- **ID**: task_tamper_2
- **Target**: terra-ui
- **Severity**: high
- **Created At**: 2026-10-11T12:00:00Z

## Reproduction
```json
{}
```

## Expected
N/A
"#;

    fs::write(outbox.join("issue_task_tamper_2.md"), draft_missing_status).unwrap();

    assert!(matches!(
        HitlOutbox::approve("task_tamper_2", &outbox),
        Err(RouterError::OutboxError(_))
    ));
    assert!(matches!(
        HitlOutbox::reject("task_tamper_2", &outbox),
        Err(RouterError::OutboxError(_))
    ));
}

#[test]
fn test_tier5_outbox_markdown_summary_parsing_fallbacks() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");
    fs::create_dir_all(&outbox).unwrap();

    let draft_minimal_header = r#"# Staged Issue Draft: Partial Header Issue

- **ID**: task_partial_1
- **Status**: Pending Human Authorization (HITL)
"#;
    fs::write(outbox.join("issue_task_partial_1.md"), draft_minimal_header).unwrap();

    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), 1);
    let s = &summaries[0];
    assert_eq!(s.id, "task_partial_1");
    assert_eq!(s.title, "Partial Header Issue");
    assert_eq!(s.target, "unknown");
    assert_eq!(s.severity, "normal");
    assert_eq!(s.created_at, "");
    assert_eq!(s.status, StagedIssueStatus::Pending);

    let draft_no_id_line = r#"# Staged Issue Draft: Title Only Draft

- **Status**: Pending Human Authorization (HITL)
"#;
    fs::write(outbox.join("issue_fallback_id.md"), draft_no_id_line).unwrap();

    let summaries2 = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries2.len(), 2);
    let s2 = summaries2.iter().find(|x| x.id == "fallback_id").unwrap();
    assert_eq!(s2.title, "Title Only Draft");

    let draft_no_title = r#"- **ID**: task_no_title

- **Status**: Pending Human Authorization (HITL)
"#;
    fs::write(outbox.join("issue_task_no_title.md"), draft_no_title).unwrap();

    let summaries3 = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries3.len(), 3);
    let s3 = summaries3.iter().find(|x| x.id == "task_no_title").unwrap();
    assert_eq!(s3.title, "Untitled Staged Issue");

    let draft_no_title_no_id = r#"Random junk text without markdown headers"#;
    fs::write(outbox.join("issue_junk.md"), draft_no_title_no_id).unwrap();

    let summaries4 = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries4.len(), 3, "Junk file must be skipped");
}

// ============================================================================
// Section 7: Concurrent Multi-Threaded Restaging & Deterministic Tie-Breaker Stress
// ============================================================================

#[test]
fn test_tier5_concurrent_multithreaded_staging_and_lifecycle() {
    let dir = tempdir().unwrap();
    let outbox = Arc::new(dir.path().join("staged"));
    fs::create_dir_all(&*outbox).unwrap();

    let num_threads = 20;
    let mut handles = Vec::new();

    for i in 0..num_threads {
        let outbox_clone = Arc::clone(&outbox);
        let handle = thread::spawn(move || {
            let id = format!("task_thread_{:02}", i);
            let env = sample_base_envelope(&id, true, None);
            HitlOutbox::stage(&env, &outbox_clone).expect("Thread staging must succeed");
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Thread join must succeed");
    }

    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), num_threads);

    let mut action_handles = Vec::new();
    for i in 0..num_threads {
        let outbox_clone = Arc::clone(&outbox);
        let h = thread::spawn(move || {
            let id = format!("task_thread_{:02}", i);
            if i % 2 == 0 {
                HitlOutbox::approve(&id, &outbox_clone).expect("Approve thread must succeed");
            } else {
                HitlOutbox::reject(&id, &outbox_clone).expect("Reject thread must succeed");
            }
        });
        action_handles.push(h);
    }

    for h in action_handles {
        h.join().expect("Action thread join must succeed");
    }

    let updated_summaries = HitlOutbox::list_staged(&outbox).unwrap();
    let approved_count = updated_summaries
        .iter()
        .filter(|s| s.status.is_approved())
        .count();
    let rejected_count = updated_summaries
        .iter()
        .filter(|s| s.status.is_rejected())
        .count();
    let pending_count = updated_summaries
        .iter()
        .filter(|s| s.status.is_pending())
        .count();

    assert_eq!(approved_count, 10);
    assert_eq!(rejected_count, 10);
    assert_eq!(pending_count, 0);
}

#[test]
fn test_tier5_deterministic_sorting_identical_timestamp_tie_breaker() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");

    let ids = ["task_charlie", "task_alpha", "task_bravo", "task_delta"];
    for id in ids {
        let mut env = sample_base_envelope(id, true, None);
        env.created_at = "2026-10-11T12:00:00Z".to_string(); // Identical timestamp
        HitlOutbox::stage(&env, &outbox).unwrap();
    }

    let summaries = HitlOutbox::list_staged(&outbox).unwrap();
    assert_eq!(summaries.len(), 4);
    // When created_at matches, tie breaker is id ascending: alpha, bravo, charlie, delta
    assert_eq!(summaries[0].id, "task_alpha");
    assert_eq!(summaries[1].id, "task_bravo");
    assert_eq!(summaries[2].id, "task_charlie");
    assert_eq!(summaries[3].id, "task_delta");
}

// ============================================================================
// Section 8: format_agent_prompt Structured Payload Permutations
// ============================================================================

#[test]
fn test_tier5_format_agent_prompt_permutations() {
    let mut env = sample_base_envelope("task_fmt_01", false, Some("/ws/core"));
    env.caller.pane_id = None;
    let prompt_no_pane = format_agent_prompt(&env);
    assert!(prompt_no_pane.contains("Caller: agy in /caller/workspace"));
    assert!(!prompt_no_pane.contains("pane:"));

    env.caller.pane_id = Some("%3".to_string());
    let prompt_with_pane = format_agent_prompt(&env);
    assert!(prompt_with_pane.contains("Caller: agy (pane: %3) in /caller/workspace"));

    env.payload.expected = None;
    let prompt_no_expected = format_agent_prompt(&env);
    assert!(!prompt_no_expected.contains("Expected Behavior:"));

    env.payload.expected = Some("Custom expected outcome".to_string());
    let prompt_with_expected = format_agent_prompt(&env);
    assert!(prompt_with_expected.contains("Expected Behavior:\nCustom expected outcome"));

    env.payload.reproduction = Some(serde_json::json!({
        "error": "Only error message present"
    }));
    let prompt_error_only = format_agent_prompt(&env);
    assert!(prompt_error_only.contains("Error:\n```\nOnly error message present\n```"));
    assert!(!prompt_error_only.contains("Snippet:"));

    env.payload.reproduction = Some(serde_json::json!({
        "snippet": "const btn = click();"
    }));
    let prompt_snippet_only = format_agent_prompt(&env);
    assert!(prompt_snippet_only.contains("Snippet:\n```\nconst btn = click();\n```"));
    assert!(!prompt_snippet_only.contains("Error:"));

    env.payload.reproduction = Some(serde_json::json!({
        "cmd": "cargo test",
        "code": 101
    }));
    let prompt_arbitrary_json = format_agent_prompt(&env);
    assert!(prompt_arbitrary_json.contains("```json"));
    assert!(prompt_arbitrary_json.contains("\"cmd\": \"cargo test\""));

    env.payload.reproduction = None;
    let prompt_no_repro = format_agent_prompt(&env);
    assert!(!prompt_no_repro.contains("Reproduction Details:"));
}

// ============================================================================
// Section 9: RouterCoordinator & RouteOutcome State & Metrics
// ============================================================================

#[test]
fn test_tier5_router_coordinator_lifecycle_and_system_status() {
    let dir = tempdir().unwrap();
    let outbox = dir.path().join("staged");

    let driver = MockDriver::new_available();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&outbox);
    assert_eq!(coordinator.outbox_dir(), outbox.as_path());
    assert_eq!(coordinator.driver().name(), "mock-mux");

    let status1 = coordinator.system_status().unwrap();
    assert_eq!(status1.driver_name(), "mock-mux");
    assert!(status1.driver_available());
    assert_eq!(status1.sessions_count(), 0);
    assert_eq!(status1.outbox_total(), 0);
    assert_eq!(status1.outbox_pending(), 0);
    assert_eq!(status1.outbox_approved(), 0);
    assert_eq!(status1.outbox_rejected(), 0);

    let env_ext = sample_base_envelope("task_coord_ext", true, None);
    let outcome = coordinator.route_envelope(&env_ext, false).unwrap();
    assert!(outcome.is_staged());
    assert!(!outcome.is_dispatched());
    assert!(!outcome.is_dry_run());
    assert!(outcome.issue_path().unwrap().exists());
    assert!(outcome.reason().unwrap().contains("external-lib"));

    let status2 = coordinator.system_status().unwrap();
    assert_eq!(status2.outbox_total(), 1);
    assert_eq!(status2.outbox_pending(), 1);

    coordinator.approve_staged("task_coord_ext").unwrap();
    let status3 = coordinator.system_status().unwrap();
    assert_eq!(status3.outbox_pending(), 0);
    assert_eq!(status3.outbox_approved(), 1);
    assert_eq!(status3.outbox_rejected(), 0);

    let env_int_bad_ws = sample_base_envelope("task_bad_ws", false, Some("/missing/directory/999"));
    let route_err = coordinator
        .route_envelope(&env_int_bad_ws, false)
        .unwrap_err();
    assert!(matches!(route_err, RouterError::WorkspaceNotFound(_)));

    let dry_run_ext = coordinator.route_envelope(&env_ext, true).unwrap();
    assert!(dry_run_ext.is_dry_run());
    match dry_run_ext {
        RouteOutcome::DryRun {
            verdict,
            resolution,
        } => {
            assert!(verdict.is_untrusted());
            assert!(resolution.is_none());
        }
        _ => panic!("Expected DryRun outcome"),
    }
}

#[test]
fn test_tier5_route_outcome_accessors_and_serde() {
    let session = LiveSession::new(
        "%5",
        PathBuf::from("/ws"),
        SessionStatus::Idle,
        Some("zsh".into()),
    );
    let dispatched = RouteOutcome::Dispatched {
        session: session.clone(),
        prompt: "Run prompt".to_string(),
    };
    assert!(dispatched.is_dispatched());
    assert!(!dispatched.is_staged());
    assert!(!dispatched.is_dry_run());
    assert_eq!(dispatched.session().unwrap().pane_id, "%5");
    assert_eq!(dispatched.prompt(), Some("Run prompt"));
    assert_eq!(dispatched.issue_path(), None);
    assert_eq!(dispatched.reason(), None);

    let json_disp = serde_json::to_string(&dispatched).unwrap();
    let des_disp: RouteOutcome = serde_json::from_str(&json_disp).unwrap();
    assert_eq!(dispatched, des_disp);

    let staged = RouteOutcome::Staged {
        issue_path: PathBuf::from("/staged/issue_1.md"),
        reason: "Untrusted external".to_string(),
    };
    assert!(staged.is_staged());
    assert!(!staged.is_dispatched());
    assert_eq!(staged.issue_path(), Some(Path::new("/staged/issue_1.md")));
    assert_eq!(staged.reason(), Some("Untrusted external"));

    let json_staged = serde_json::to_string(&staged).unwrap();
    let des_staged: RouteOutcome = serde_json::from_str(&json_staged).unwrap();
    assert_eq!(staged, des_staged);

    let dry_run = RouteOutcome::DryRun {
        verdict: PolicyVerdict::TrustedInternal,
        resolution: Some(SessionResolution::HotSession(session)),
    };
    assert!(dry_run.is_dry_run());
    assert_eq!(dry_run.session().unwrap().pane_id, "%5");

    let json_dry = serde_json::to_string(&dry_run).unwrap();
    let des_dry: RouteOutcome = serde_json::from_str(&json_dry).unwrap();
    assert_eq!(dry_run, des_dry);
}

#[test]
fn test_tier5_system_status_serde_roundtrip() {
    let summary = SystemStatusSummary {
        version: "0.1.0".to_string(),
        driver: DriverStatusSummary {
            name: "tmux".to_string(),
            available: true,
            sessions_count: 3,
        },
        outbox: OutboxStatusSummary {
            directory: PathBuf::from(".scratch/staged"),
            total: 5,
            pending: 2,
            approved: 2,
            rejected: 1,
        },
    };

    let json = serde_json::to_string(&summary).unwrap();
    let des: SystemStatusSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(summary, des);
}
