mod common;

use chrono::DateTime;
use k0maru_router::envelope::TaskEnvelope;

// ============================================================================
// Tier 1: Feature Coverage (>= 5 tests per feature)
// ============================================================================

#[test]
fn tier1_envelope_full_json_deserialization() {
    let json = common::sample_valid_external_json();
    let envelope: TaskEnvelope =
        serde_json::from_str(&json).expect("Full TaskEnvelope JSON should deserialize cleanly");

    assert_eq!(
        envelope.schema.as_deref(),
        Some("https://k0maru.dev/schemas/agent-task-v1.json")
    );
    assert_eq!(envelope.id, "task_9f82a1b4");
    assert_eq!(envelope.created_at, "2026-10-11T18:30:00Z");
    assert_eq!(
        envelope.caller.workspace,
        "/Users/k0maru3/workspace/consumer-app"
    );
    assert_eq!(envelope.caller.agent, "agy");
    assert_eq!(envelope.caller.pane_id.as_deref(), Some("%1"));
    assert_eq!(envelope.target.identifier, "Terra-UI");
    assert_eq!(
        envelope.target.workspace.as_deref(),
        Some("/Users/k0maru3/workspace/terra-ui")
    );
    assert!(envelope.target.is_external);
    assert_eq!(envelope.intent, "bug_report");
    assert_eq!(
        envelope.payload.title,
        "TerraButton missing aria-label attribute pass-through"
    );
    assert_eq!(envelope.payload.severity, "blocking");

    let repro = envelope
        .payload
        .reproduction
        .expect("Reproduction payload should exist");
    assert_eq!(
        repro["snippet"].as_str(),
        Some("<TerraButton aria-label='close' />")
    );
    assert_eq!(
        repro["error"].as_str(),
        Some("HTML button element does not receive aria-label")
    );
    assert_eq!(
        envelope.payload.expected.as_deref(),
        Some("Attribute forwarded to rendered <button> element")
    );
}

#[test]
fn tier1_envelope_minimal_optional_fields_omitted() {
    let json = common::sample_minimal_json();
    let envelope: TaskEnvelope =
        serde_json::from_str(&json).expect("Minimal TaskEnvelope JSON should deserialize cleanly");

    assert_eq!(envelope.id, "task_minimal_123");
    assert_eq!(envelope.created_at, "2026-10-11T12:00:00Z");
    assert_eq!(envelope.caller.workspace, "/tmp/minimal-caller");
    assert_eq!(envelope.caller.agent, "minimal-agent");
    assert_eq!(envelope.caller.pane_id, None);
    assert_eq!(envelope.target.identifier, "minimal-target");
    assert_eq!(envelope.target.workspace, None);
    assert!(!envelope.target.is_external);
    assert_eq!(envelope.intent, "bug_report");
    assert_eq!(envelope.payload.title, "Minimal Bug Report");
    assert_eq!(envelope.payload.severity, "low");
    assert_eq!(envelope.payload.reproduction, None);
    assert_eq!(envelope.payload.expected, None);
    assert_eq!(envelope.schema, None);
}

#[test]
fn tier1_envelope_roundtrip_serde_preservation() {
    let original = common::sample_internal_envelope();
    let serialized =
        serde_json::to_string_pretty(&original).expect("Serialization to JSON must succeed");
    let deserialized: TaskEnvelope =
        serde_json::from_str(&serialized).expect("Deserialization back from JSON must succeed");

    assert_eq!(
        original, deserialized,
        "Deserialized envelope must be identical to original"
    );
}

#[test]
fn tier1_envelope_rfc3339_timestamp_validation() {
    let valid_timestamps = [
        "2026-10-11T18:30:00Z",
        "2026-10-11T18:30:00.123456Z",
        "2026-10-11T14:30:00+04:00",
        "2026-10-11T10:30:00-08:00",
    ];

    for ts in valid_timestamps {
        let json = common::sample_json_with_timestamp(ts);
        let envelope: TaskEnvelope = serde_json::from_str(&json)
            .unwrap_or_else(|_| panic!("Failed to parse for ts: {}", ts));
        assert_eq!(envelope.created_at, ts);

        // Verify RFC-3339 parseability
        let parsed = DateTime::parse_from_rfc3339(&envelope.created_at);
        assert!(
            parsed.is_ok(),
            "Timestamp {} must conform to RFC-3339",
            envelope.created_at
        );
    }
}

#[test]
fn tier1_envelope_structured_reproduction_payload() {
    let mut envelope = common::sample_internal_envelope();
    let custom_repro = serde_json::json!({
        "command": "cargo test --test foo",
        "exit_code": 101,
        "stack_trace": ["line 1", "line 2", "panic at src/lib.rs:42"],
        "nested": {
            "flag": true,
            "count": 3
        }
    });
    envelope.payload.reproduction = Some(custom_repro.clone());

    let serialized = serde_json::to_string(&envelope).expect("Serialize must succeed");
    let deserialized: TaskEnvelope =
        serde_json::from_str(&serialized).expect("Deserialize must succeed");

    assert_eq!(deserialized.payload.reproduction, Some(custom_repro));
}

#[test]
fn tier1_envelope_target_external_flag_distinction() {
    let internal_env = common::sample_internal_envelope();
    let external_env = common::sample_external_envelope();

    assert!(
        !internal_env.target.is_external,
        "Internal envelope must have is_external: false"
    );
    assert!(
        external_env.target.is_external,
        "External envelope must have is_external: true"
    );
}

// ============================================================================
// Tier 2: Boundary & Corner Cases (>= 5 tests per feature)
// ============================================================================

#[test]
fn tier2_envelope_empty_json_string_fails() {
    let result: Result<TaskEnvelope, _> = serde_json::from_str("");
    assert!(
        result.is_err(),
        "Empty string must return a deserialization error"
    );
}

#[test]
fn tier2_envelope_malformed_syntax_fails() {
    let malformed_samples = [
        "{",
        r#"{"id": "task_1""#,
        r#"{"id": "task_1", "caller": }"#,
        "not-even-json",
        "[1, 2, 3]",
    ];

    for sample in malformed_samples {
        let result: Result<TaskEnvelope, _> = serde_json::from_str(sample);
        assert!(
            result.is_err(),
            "Malformed JSON '{}' must fail deserialization",
            sample
        );
    }
}

#[test]
fn tier2_envelope_missing_required_fields_fails() {
    for (missing_desc, invalid_json) in common::invalid_missing_field_samples() {
        let result: Result<TaskEnvelope, _> = serde_json::from_str(&invalid_json);
        assert!(
            result.is_err(),
            "Payload with {} must fail deserialization",
            missing_desc
        );
    }
}

#[test]
fn tier2_envelope_invalid_field_types_fails() {
    for (type_desc, invalid_json) in common::invalid_type_samples() {
        let result: Result<TaskEnvelope, _> = serde_json::from_str(&invalid_json);
        assert!(
            result.is_err(),
            "Payload with {} must fail deserialization",
            type_desc
        );
    }
}

#[test]
fn tier2_envelope_adversarial_escaping_and_unicode() {
    let adv_json = common::sample_adversarial_json();
    let envelope: TaskEnvelope = serde_json::from_str(&adv_json)
        .expect("Adversarial payload with quotes, emojis, and tags must parse successfully");

    assert!(envelope.id.contains("!@#$%^&*()"));
    assert!(envelope.caller.agent.contains("🤖"));
    assert!(envelope
        .target
        .identifier
        .contains("<script>alert('xss')</script>"));
    assert!(envelope.payload.title.contains("🚀🔥"));
    assert!(envelope.payload.title.contains("**Markdown**"));

    let repro = envelope
        .payload
        .reproduction
        .as_ref()
        .expect("Reproduction must exist");
    assert!(repro["snippet"]
        .as_str()
        .unwrap()
        .contains("&& rm -rf /; echo $HOME;"));
    assert!(repro["error"].as_str().unwrap().contains("Uncaught"));

    // Round-trip test to ensure escaping integrity
    let serialized = serde_json::to_string_pretty(&envelope).unwrap();
    let deserialized: TaskEnvelope = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        envelope, deserialized,
        "Adversarial envelope must preserve identical content after roundtrip"
    );
}

#[test]
fn tier2_envelope_large_payload_handling() {
    // Test a 128KB payload (typical large stack trace or minified bundle reproduction)
    let large_json = common::sample_large_payload_json(128 * 1024);
    let envelope: TaskEnvelope = serde_json::from_str(&large_json)
        .expect("Large 128KB reproduction payload must deserialize cleanly");

    let repro = envelope.payload.reproduction.unwrap();
    let snippet_len = repro["snippet"].as_str().unwrap().len();
    assert_eq!(snippet_len, 128 * 1024);
}

#[test]
fn tier2_envelope_invalid_timestamp_detection() {
    let invalid_timestamps = [
        "not-a-timestamp",
        "2026-02-31T00:00:00Z", // Invalid day of month
        "2026/10/11 18:30:00",  // Non-RFC-3339 format
        "10-11-2026",
        "",
    ];

    for bad_ts in invalid_timestamps {
        let parsed = DateTime::parse_from_rfc3339(bad_ts);
        assert!(
            parsed.is_err(),
            "Invalid timestamp '{}' must fail RFC-3339 parsing",
            bad_ts
        );
    }
}
