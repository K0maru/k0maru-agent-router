mod common;

use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload, DEFAULT_SCHEMA,
    VALID_SEVERITIES,
};
use k0maru_router::error::RouterError;
use std::path::PathBuf;

// ============================================================================
// Area 1: Serialization Roundtrips
// ============================================================================

#[test]
fn test_roundtrip_canonical_envelope() {
    let original = common::sample_internal_envelope();

    // 1. to_json_string roundtrip
    let json_compact = original
        .to_json_string()
        .expect("to_json_string should succeed");
    let recovered_compact =
        TaskEnvelope::from_json_str(&json_compact).expect("from_json_str should succeed");
    assert_eq!(original, recovered_compact);

    // 2. to_json_pretty roundtrip
    let json_pretty = original
        .to_json_pretty()
        .expect("to_json_pretty should succeed");
    let recovered_pretty =
        TaskEnvelope::from_json_str(&json_pretty).expect("from_json_str should succeed");
    assert_eq!(original, recovered_pretty);

    // 3. serde_json::Value roundtrip
    let value = serde_json::to_value(&original).expect("to_value should succeed");
    let recovered_from_val: TaskEnvelope =
        serde_json::from_value(value).expect("from_value should succeed");
    assert_eq!(original, recovered_from_val);

    // 4. Double roundtrip idempotency: E == E' == E''
    let json_second = recovered_compact
        .to_json_string()
        .expect("second serialization should succeed");
    assert_eq!(json_compact, json_second);
}

#[test]
fn test_roundtrip_minimal_envelope_with_optional_fields_omitted() {
    let minimal = TaskEnvelope {
        schema: None,
        id: "task_min_roundtrip".to_string(),
        created_at: "2026-10-11T12:00:00Z".to_string(),
        caller: CallerInfo::new("/ws/caller", "worker-a"),
        target: TargetInfo::new_internal("target-svc", "/ws/target"),
        intent: "feature_request".to_string(),
        payload: TaskPayload::new("Add endpoint", "low"),
    };

    let serialized = minimal
        .to_json_string()
        .expect("Serialization should succeed");
    assert!(!serialized.contains("$schema"));
    assert!(!serialized.contains("pane_id"));
    assert!(!serialized.contains("reproduction"));
    assert!(!serialized.contains("expected"));

    let val: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert!(val.get("$schema").is_none());
    assert!(val["caller"].get("pane_id").is_none());
    assert!(val["payload"].get("reproduction").is_none());
    assert!(val["payload"].get("expected").is_none());

    let recovered =
        TaskEnvelope::from_json_str(&serialized).expect("Deserialization should succeed");
    assert_eq!(minimal, recovered);
    assert_eq!(recovered.schema, None);
    assert_eq!(recovered.caller.pane_id, None);
    assert_eq!(recovered.payload.reproduction, None);
    assert_eq!(recovered.payload.expected, None);
}

#[test]
fn test_roundtrip_external_target_with_and_without_workspace() {
    // Without workspace
    let ext_no_ws = TaskEnvelope {
        schema: Some(DEFAULT_SCHEMA.to_string()),
        id: "task_ext_1".to_string(),
        created_at: "2026-10-11T14:00:00Z".to_string(),
        caller: CallerInfo::new("/ws/caller", "worker-a"),
        target: TargetInfo::new_external("react"),
        intent: "bug_report".to_string(),
        payload: TaskPayload::new("Hydration error", "critical"),
    };
    let json_no_ws = ext_no_ws.to_json_string().unwrap();
    let val_no_ws: serde_json::Value = serde_json::from_str(&json_no_ws).unwrap();
    assert!(
        val_no_ws["target"].get("workspace").is_none(),
        "target.workspace must be omitted when None"
    );
    let rec_no_ws = TaskEnvelope::from_json_str(&json_no_ws).unwrap();
    assert_eq!(ext_no_ws, rec_no_ws);
    assert!(rec_no_ws.target.workspace.is_none());

    // With workspace
    let ext_with_ws = TaskEnvelope {
        schema: Some(DEFAULT_SCHEMA.to_string()),
        id: "task_ext_2".to_string(),
        created_at: "2026-10-11T14:00:00Z".to_string(),
        caller: CallerInfo::new("/ws/caller", "worker-a"),
        target: TargetInfo::new_external("vitest").with_workspace("/tmp/vitest-clone"),
        intent: "bug_report".to_string(),
        payload: TaskPayload::new("Runner crash", "high"),
    };
    let json_with_ws = ext_with_ws.to_json_string().unwrap();
    let val_with_ws: serde_json::Value = serde_json::from_str(&json_with_ws).unwrap();
    assert_eq!(
        val_with_ws["target"]["workspace"].as_str(),
        Some("/tmp/vitest-clone")
    );
    let rec_with_ws = TaskEnvelope::from_json_str(&json_with_ws).unwrap();
    assert_eq!(ext_with_ws, rec_with_ws);
    assert_eq!(
        rec_with_ws.target.workspace.as_deref(),
        Some("/tmp/vitest-clone")
    );
}

#[test]
fn test_roundtrip_complex_structured_and_arbitrary_reproduction() {
    let mut envelope = common::sample_internal_envelope();

    // 1. Structured ReproductionPayload roundtrip
    let typed_repro = ReproductionPayload::new(
        "console.log('bug'); throw new Error('repro');",
        "Uncaught Error: repro at line 1",
    );
    envelope.payload = envelope
        .payload
        .clone()
        .with_reproduction_payload(typed_repro.clone());

    let json = envelope.to_json_string().unwrap();
    let recovered = TaskEnvelope::from_json_str(&json).unwrap();
    assert_eq!(envelope, recovered);

    let extracted_repro = recovered
        .payload
        .reproduction_payload()
        .expect("reproduction must be present")
        .expect("must parse cleanly");
    assert_eq!(typed_repro, extracted_repro);

    // 2. Arbitrary JSON reproduction with nested structures
    let arbitrary_repro = serde_json::json!({
        "steps": ["step1", "step2"],
        "metadata": {
            "exit_code": 137,
            "oom_killed": true,
            "metrics": [0.1, 0.99]
        }
    });
    envelope.payload.reproduction = Some(arbitrary_repro.clone());
    let json2 = envelope.to_json_string().unwrap();
    let recovered2 = TaskEnvelope::from_json_str(&json2).unwrap();
    assert_eq!(recovered2.payload.reproduction, Some(arbitrary_repro));
}

// ============================================================================
// Area 2: Unknown Field Handling
// ============================================================================

#[test]
fn test_unknown_fields_in_top_level_and_sub_structs() {
    let json_with_unknown = r#"{
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_unknown_01",
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/Users/k0maru3/workspace/consumer-app",
            "agent": "agy",
            "pane_id": "%1",
            "unknown_caller_field": "extra_data",
            "nested_caller_extra": { "foo": 123 }
        },
        "target": {
            "identifier": "Terra-UI",
            "workspace": "/Users/k0maru3/workspace/terra-ui",
            "is_external": false,
            "target_tag": "frontend",
            "priority_override": 5
        },
        "intent": "bug_report",
        "payload": {
            "title": "TerraButton issue",
            "severity": "normal",
            "extra_payload_field": true,
            "labels": ["ui", "bug"]
        },
        "extra_top_level_1": "ignored_value",
        "extra_top_level_2": { "deep": [1, 2, 3] }
    }"#;

    // Deserialization succeeds: Serde ignores unknown fields by default
    let envelope = TaskEnvelope::from_json_str(json_with_unknown).expect(
        "Deserialization with unknown fields should succeed (Serde ignores unrecognized keys)",
    );

    assert_eq!(envelope.id, "task_unknown_01");
    assert_eq!(envelope.caller.agent, "agy");
    assert_eq!(envelope.target.identifier, "Terra-UI");

    // Serialization drops unknown fields because they are not represented in Rust structs
    let serialized = envelope
        .to_json_string()
        .expect("Serialization should succeed");
    assert!(!serialized.contains("extra_top_level_1"));
    assert!(!serialized.contains("extra_top_level_2"));
    assert!(!serialized.contains("unknown_caller_field"));
    assert!(!serialized.contains("nested_caller_extra"));
    assert!(!serialized.contains("target_tag"));
    assert!(!serialized.contains("priority_override"));
    assert!(!serialized.contains("extra_payload_field"));
    assert!(!serialized.contains("labels"));
}

#[test]
fn test_unknown_field_typos_in_optional_keys() {
    // If a user sends "paneId" (camelCase) instead of "pane_id", Serde ignores it and pane_id is None
    let json_camel = r#"{
        "id": "task_typo_01",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": {
            "workspace": "/ws",
            "agent": "agy",
            "paneId": "%9"
        },
        "target": {
            "identifier": "t",
            "workspace": "/ws",
            "is_external": false
        },
        "intent": "bug_report",
        "payload": {
            "title": "Typo test",
            "severity": "normal"
        }
    }"#;

    let envelope = TaskEnvelope::from_json_str(json_camel).unwrap();
    assert_eq!(
        envelope.caller.pane_id, None,
        "Unrecognized field 'paneId' must be ignored, pane_id remains None"
    );
}

// ============================================================================
// Area 3: Empty Optional Fields vs Omitted Fields
// ============================================================================

#[test]
fn test_optional_fields_omitted_vs_explicit_null() {
    // 1. Omitted fields: all deserialize to None
    let json_omitted = r#"{
        "id": "task_opt_omitted",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": {
            "workspace": "/ws",
            "agent": "worker-1"
        },
        "target": {
            "identifier": "ext-pkg",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Bug",
            "severity": "low"
        }
    }"#;
    let env_omitted = TaskEnvelope::from_json_str(json_omitted).unwrap();
    assert!(env_omitted.schema.is_none());
    assert!(env_omitted.caller.pane_id.is_none());
    assert!(env_omitted.target.workspace.is_none());
    assert!(env_omitted.payload.reproduction.is_none());
    assert!(env_omitted.payload.expected.is_none());

    // 2. Explicit null fields: all deserialize to None
    let json_null = r#"{
        "$schema": null,
        "id": "task_opt_null",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": {
            "workspace": "/ws",
            "agent": "worker-1",
            "pane_id": null
        },
        "target": {
            "identifier": "ext-pkg",
            "workspace": null,
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Bug",
            "severity": "low",
            "reproduction": null,
            "expected": null
        }
    }"#;
    let env_null = TaskEnvelope::from_json_str(json_null).unwrap();
    assert!(env_null.schema.is_none());
    assert!(env_null.caller.pane_id.is_none());
    assert!(env_null.target.workspace.is_none());
    assert!(env_null.payload.reproduction.is_none());
    assert!(env_null.payload.expected.is_none());

    // They produce identical semantic states
    assert_eq!(env_omitted.schema, env_null.schema);
    assert_eq!(env_omitted.caller.pane_id, env_null.caller.pane_id);
    assert_eq!(env_omitted.target.workspace, env_null.target.workspace);
    assert_eq!(
        env_omitted.payload.reproduction,
        env_null.payload.reproduction
    );
    assert_eq!(env_omitted.payload.expected, env_null.payload.expected);
}

#[test]
fn test_internal_target_explicit_null_workspace_rejected() {
    let json = r#"{
        "id": "task_internal_null_ws",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "agy" },
        "target": { "identifier": "internal-t", "workspace": null, "is_external": false },
        "intent": "bug_report",
        "payload": { "title": "Bug", "severity": "low" }
    }"#;

    let res = TaskEnvelope::from_json_str(json);
    assert!(
        matches!(res, Err(RouterError::InvalidEnvelope(ref msg)) if msg.contains("Internal target ('is_external: false') requires a non-empty 'target.workspace' path")),
        "Internal target with null workspace must be rejected by validate()"
    );
}

#[test]
fn test_empty_string_optional_fields_rejected_by_validation() {
    // 1. Empty string $schema
    let json_empty_schema = r#"{
        "$schema": "",
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_schema),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("$schema")
    ));

    // Whitespace-only $schema
    let json_ws_schema = r#"{
        "$schema": "   ",
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_schema),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("$schema")
    ));

    // 2. Empty string caller.pane_id
    let json_empty_pane = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a", "pane_id": "" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_pane),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("pane_id")
    ));

    // Whitespace caller.pane_id
    let json_ws_pane = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a", "pane_id": " \t " },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_pane),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("pane_id")
    ));

    // 3. Empty string external target.workspace
    let json_empty_ext_ws = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "", "is_external": true },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_ext_ws),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Target workspace path cannot be empty if specified")
    ));

    // Whitespace external target.workspace
    let json_ws_ext_ws = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "   ", "is_external": true },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_ext_ws),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Target workspace path cannot be empty if specified")
    ));

    // 4. Empty string payload.expected
    let json_empty_expected = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low", "expected": "" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_expected),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("payload.expected")
    ));

    // Whitespace payload.expected
    let json_ws_expected = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "low", "expected": "  \n  " }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_expected),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("payload.expected")
    ));
}

#[test]
fn test_reproduction_empty_fields_in_object() {
    // 1. Empty snippet inside reproduction object
    let json_empty_snippet = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": {
            "title": "t",
            "severity": "low",
            "reproduction": { "snippet": "", "error": "some error" }
        }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_snippet),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Reproduction 'snippet' cannot be empty")
    ));

    // 2. Whitespace snippet inside reproduction object
    let json_ws_snippet = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": {
            "title": "t",
            "severity": "low",
            "reproduction": { "snippet": "   ", "error": "some error" }
        }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_snippet),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Reproduction 'snippet' cannot be empty")
    ));

    // 3. Empty error inside reproduction object
    let json_empty_err = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": {
            "title": "t",
            "severity": "low",
            "reproduction": { "snippet": "some snippet", "error": "" }
        }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_empty_err),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Reproduction 'error' cannot be empty")
    ));

    // 4. Whitespace error inside reproduction object
    let json_ws_err = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": {
            "title": "t",
            "severity": "low",
            "reproduction": { "snippet": "some snippet", "error": "   " }
        }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_ws_err),
        Err(RouterError::InvalidEnvelope(ref m)) if m.contains("Reproduction 'error' cannot be empty")
    ));
}

// ============================================================================
// Area 4: Invalid Severities
// ============================================================================

#[test]
fn test_all_valid_severities_accepted_case_insensitively() {
    let valid_cases = [
        ("blocking", "blocking"),
        ("BLOCKING", "BLOCKING"),
        ("Blocking", "Blocking"),
        ("critical", "critical"),
        ("CRITICAL", "CRITICAL"),
        ("Critical", "Critical"),
        ("high", "high"),
        ("HIGH", "HIGH"),
        ("High", "High"),
        ("medium", "medium"),
        ("MEDIUM", "MEDIUM"),
        ("Medium", "Medium"),
        ("low", "low"),
        ("LOW", "LOW"),
        ("Low", "Low"),
        ("normal", "normal"),
        ("NORMAL", "NORMAL"),
        ("Normal", "Normal"),
    ];

    for (input_sev, _) in valid_cases {
        let mut envelope = common::sample_internal_envelope();
        envelope.payload.severity = input_sev.to_string();
        assert!(
            envelope.validate().is_ok(),
            "Severity '{}' must pass validation",
            input_sev
        );
    }
}

#[test]
fn test_invalid_severities_rejected_with_exact_contract_error() {
    let invalid_severities = [
        "urgent",
        "fatal",
        "p0",
        "p1",
        "catastrophic",
        "warn",
        "info",
        "trivial",
        "blocker",
        "minor",
        "major",
        "1",
        "true",
    ];

    let expected_valid_list = VALID_SEVERITIES.join(", ");

    for bad_sev in invalid_severities {
        let mut envelope = common::sample_internal_envelope();
        envelope.payload.severity = bad_sev.to_string();
        let err = envelope
            .validate()
            .expect_err(&format!("Severity '{}' must be rejected", bad_sev));

        let expected_msg = format!(
            "Invalid severity '{}': must be one of [{}]",
            bad_sev, expected_valid_list
        );

        assert_eq!(
            err,
            RouterError::InvalidEnvelope(expected_msg),
            "Exact error message mismatch for invalid severity '{}'",
            bad_sev
        );
    }
}

#[test]
fn test_empty_and_whitespace_severity_rejection() {
    let mut envelope = common::sample_internal_envelope();

    envelope.payload.severity = "".to_string();
    assert_eq!(
        envelope.validate().unwrap_err(),
        RouterError::InvalidEnvelope(
            "Task envelope 'payload.severity' cannot be empty".to_string()
        )
    );

    envelope.payload.severity = "   ".to_string();
    assert_eq!(
        envelope.validate().unwrap_err(),
        RouterError::InvalidEnvelope(
            "Task envelope 'payload.severity' cannot be empty".to_string()
        )
    );
}

#[test]
fn test_non_string_severity_json_type_failure() {
    // If severity is a number or boolean in JSON, it should fail during Serde deserialization
    let json_num = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": 1 }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_num),
        Err(RouterError::Json(_))
    ));

    let json_bool = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": true }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(json_bool),
        Err(RouterError::Json(_))
    ));
}

// ============================================================================
// Area 5: Error Message Types
// ============================================================================

#[test]
fn test_error_type_distinction_syntax_vs_validation_vs_io() {
    // 1. Syntax / Malformed JSON -> RouterError::Json
    let bad_syntax = "{ broken json ";
    assert!(matches!(
        TaskEnvelope::from_json_str(bad_syntax),
        Err(RouterError::Json(_))
    ));

    // 2. Missing required field -> RouterError::Json (Serde missing field error)
    let missing_field = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug"
    }"#; // missing payload
    let res = TaskEnvelope::from_json_str(missing_field);
    assert!(matches!(res, Err(RouterError::Json(_))));

    // 3. Empty string input -> RouterError::InvalidEnvelope
    let empty_str = "   \n\t  ";
    assert!(matches!(
        TaskEnvelope::from_json_str(empty_str),
        Err(RouterError::InvalidEnvelope(ref msg)) if msg.contains("cannot be empty")
    ));

    // 4. Semantic validation failure -> RouterError::InvalidEnvelope
    let empty_title_json = r#"{
        "id": "task_1",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "  ", "severity": "normal" }
    }"#;
    assert!(matches!(
        TaskEnvelope::from_json_str(empty_title_json),
        Err(RouterError::InvalidEnvelope(ref msg)) if msg.contains("payload.title")
    ));

    // 5. File IO failure -> RouterError::Io
    let non_existent = PathBuf::from("/non_existent_dir_99999/task.json");
    let file_res = TaskEnvelope::from_file(&non_existent);
    assert!(matches!(file_res, Err(RouterError::Io(_))));
}

#[test]
fn test_timestamp_parse_error_is_typed_invalid_envelope() {
    let bad_ts_json = r#"{
        "id": "task_1",
        "created_at": "invalid-timestamp",
        "caller": { "workspace": "/ws", "agent": "a" },
        "target": { "identifier": "t", "workspace": "/ws", "is_external": false },
        "intent": "bug",
        "payload": { "title": "t", "severity": "normal" }
    }"#;

    let res = TaskEnvelope::from_json_str(bad_ts_json);
    match res {
        Err(RouterError::InvalidEnvelope(msg)) => {
            assert!(
                msg.contains("valid RFC-3339 timestamp"),
                "Timestamp error must mention RFC-3339: {}",
                msg
            );
        }
        other => panic!("Expected RouterError::InvalidEnvelope, got {:?}", other),
    }
}

#[test]
fn test_router_error_display_formatting() {
    let err1 = RouterError::InvalidEnvelope("bad field".to_string());
    assert_eq!(err1.to_string(), "Invalid task envelope: bad field");

    let err2 = RouterError::IssueNotFound("issue_123".to_string());
    assert_eq!(err2.to_string(), "Staged issue not found: 'issue_123'");

    let err3 = RouterError::WorkspaceNotFound(PathBuf::from("/nonexistent"));
    assert_eq!(
        err3.to_string(),
        "Target workspace not found: '/nonexistent'"
    );

    let err4 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "tmux binary not in PATH".to_string(),
    };
    assert_eq!(
        err4.to_string(),
        "Multiplexer driver 'tmux' unavailable: tmux binary not in PATH"
    );

    let err5 = RouterError::OutboxError("permission denied".to_string());
    assert_eq!(err5.to_string(), "HITL Outbox error: permission denied");
}

#[test]
fn test_router_error_partial_eq() {
    let e1 = RouterError::InvalidEnvelope("msg".to_string());
    let e2 = RouterError::InvalidEnvelope("msg".to_string());
    let e3 = RouterError::InvalidEnvelope("different".to_string());
    assert_eq!(e1, e2);
    assert_ne!(e1, e3);

    let d1 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "none".to_string(),
    };
    let d2 = RouterError::DriverUnavailable {
        driver: "tmux",
        reason: "none".to_string(),
    };
    let d3 = RouterError::DriverUnavailable {
        driver: "herdr",
        reason: "none".to_string(),
    };
    assert_eq!(d1, d2);
    assert_ne!(d1, d3);
}
