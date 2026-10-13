#![allow(dead_code)]

use assert_cmd::Command;
use k0maru_router::envelope::{CallerInfo, TargetInfo, TaskEnvelope, TaskPayload};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Creates an isolated temporary directory for test fixtures and outbox staging.
pub fn create_temp_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temporary directory for test")
}

/// Helper to write content to a file inside the specified directory.
pub fn write_temp_file(dir: &Path, file_name: &str, content: &str) -> PathBuf {
    let file_path = dir.join(file_name);
    fs::write(&file_path, content).expect("Failed to write temporary test file");
    file_path
}

/// Generates a valid internal (trusted) TaskEnvelope instance.
pub fn sample_internal_envelope() -> TaskEnvelope {
    TaskEnvelope {
        schema: Some("https://k0maru.dev/schemas/agent-task-v1.json".to_string()),
        id: "task_internal_001".to_string(),
        created_at: "2026-10-11T18:30:00Z".to_string(),
        caller: CallerInfo {
            workspace: "/Users/k0maru3/workspace/consumer-app".to_string(),
            agent: "agy".to_string(),
            pane_id: Some("%1".to_string()),
            node: None,
        },
        target: TargetInfo {
            identifier: "internal-core".to_string(),
            workspace: Some("/Users/k0maru3/workspace/core".to_string()),
            is_external: false,
            node: None,
        },
        intent: "bug_report".to_string(),
        payload: TaskPayload {
            title: "Internal service logic error".to_string(),
            severity: "normal".to_string(),
            reproduction: Some(serde_json::json!({
                "snippet": "let res = call_internal();",
                "error": "status 500"
            })),
            expected: Some("status 200 OK".to_string()),
        },
    }
}

/// Generates a valid external (untrusted OSS) TaskEnvelope instance.
pub fn sample_external_envelope() -> TaskEnvelope {
    TaskEnvelope {
        schema: Some("https://k0maru.dev/schemas/agent-task-v1.json".to_string()),
        id: "task_9f82a1b4".to_string(),
        created_at: "2026-10-11T18:30:00Z".to_string(),
        caller: CallerInfo {
            workspace: "/Users/k0maru3/workspace/consumer-app".to_string(),
            agent: "agy".to_string(),
            pane_id: Some("%1".to_string()),
            node: None,
        },
        target: TargetInfo {
            identifier: "Terra-UI".to_string(),
            workspace: Some("/Users/k0maru3/workspace/terra-ui".to_string()),
            is_external: true,
            node: None,
        },
        intent: "bug_report".to_string(),
        payload: TaskPayload {
            title: "TerraButton missing aria-label attribute pass-through".to_string(),
            severity: "blocking".to_string(),
            reproduction: Some(serde_json::json!({
                "snippet": "<TerraButton aria-label='close' />",
                "error": "HTML button element does not receive aria-label"
            })),
            expected: Some("Attribute forwarded to rendered <button> element".to_string()),
        },
    }
}

/// Returns standard valid internal TaskEnvelope JSON string.
pub fn sample_valid_internal_json() -> String {
    serde_json::to_string_pretty(&sample_internal_envelope())
        .expect("Failed to serialize sample internal envelope")
}

/// Returns standard valid external TaskEnvelope JSON string.
pub fn sample_valid_external_json() -> String {
    serde_json::to_string_pretty(&sample_external_envelope())
        .expect("Failed to serialize sample external envelope")
}

/// Returns a minimal valid JSON with optional fields omitted.
pub fn sample_minimal_json() -> String {
    r#"{
        "id": "task_minimal_123",
        "created_at": "2026-10-11T12:00:00Z",
        "caller": {
            "workspace": "/tmp/minimal-caller",
            "agent": "minimal-agent"
        },
        "target": {
            "identifier": "minimal-target",
            "is_external": false
        },
        "intent": "bug_report",
        "payload": {
            "title": "Minimal Bug Report",
            "severity": "low"
        }
    }"#
    .to_string()
}

/// Returns an envelope JSON string with a custom timestamp.
pub fn sample_json_with_timestamp(timestamp: &str) -> String {
    format!(
        r#"{{
            "id": "task_ts_test",
            "created_at": "{}",
            "caller": {{
                "workspace": "/tmp/ws",
                "agent": "agent1"
            }},
            "target": {{
                "identifier": "target1",
                "is_external": false
            }},
            "intent": "bug_report",
            "payload": {{
                "title": "Timestamp Test",
                "severity": "normal"
            }}
        }}"#,
        timestamp
    )
}

/// Returns an adversarial JSON string containing Markdown formatting characters,
/// XML/HTML tags, shell metacharacters, and Unicode/emojis.
pub fn sample_adversarial_json() -> String {
    r#"{
        "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
        "id": "task_adv_!@#$%^&*()",
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/Users/k0maru3/workspace/special_!@#$_path",
            "agent": "agent_🤖_test",
            "pane_id": "%99"
        },
        "target": {
            "identifier": "3rd-party/pkg <script>alert('xss')</script>",
            "workspace": "/Users/k0maru3/workspace/target_path",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Adversarial `title` with **Markdown** and [links](http://example.com) & emoji 🚀🔥",
            "severity": "critical",
            "reproduction": {
                "snippet": "<Component prop=\"value\" && rm -rf /; echo $HOME; />",
                "error": "Error: Uncaught Exception in `eval()`: null pointer \n at /dev/null:0"
            },
            "expected": "Expected safe escaping of characters: < > & \" ' \n \t"
        }
    }"#.to_string()
}

/// Returns an envelope JSON string with a large reproduction payload.
pub fn sample_large_payload_json(size_bytes: usize) -> String {
    let large_snippet = "x".repeat(size_bytes);
    let payload = serde_json::json!({
        "id": "task_large_payload",
        "created_at": "2026-10-11T18:30:00Z",
        "caller": {
            "workspace": "/tmp/ws",
            "agent": "agy"
        },
        "target": {
            "identifier": "large-target",
            "is_external": true
        },
        "intent": "bug_report",
        "payload": {
            "title": "Large Payload Test",
            "severity": "high",
            "reproduction": {
                "snippet": large_snippet,
                "error": "Large stack trace"
            },
            "expected": "Handles large strings smoothly"
        }
    });
    serde_json::to_string_pretty(&payload).expect("Failed to serialize large payload")
}

/// Returns a collection of invalid JSON samples where required fields are missing.
pub fn invalid_missing_field_samples() -> Vec<(&'static str, String)> {
    vec![
        (
            "missing_id",
            r#"{
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_created_at",
            r#"{
                "id": "task_1",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_caller",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_target",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_intent",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_payload",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report"
            }"#
            .to_string(),
        ),
        (
            "missing_caller_workspace",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_target_is_external",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t" },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "missing_payload_title",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "severity": "low" }
            }"#
            .to_string(),
        ),
    ]
}

/// Returns samples of JSON with incorrect types.
pub fn invalid_type_samples() -> Vec<(&'static str, String)> {
    vec![
        (
            "numeric_id",
            r#"{
                "id": 12345,
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "boolean_is_external_as_string",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": "true" },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "caller_as_array",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": ["/tmp", "a"],
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": { "title": "t", "severity": "low" }
            }"#
            .to_string(),
        ),
        (
            "payload_as_string",
            r#"{
                "id": "task_1",
                "created_at": "2026-10-11T12:00:00Z",
                "caller": { "workspace": "/tmp", "agent": "a" },
                "target": { "identifier": "t", "is_external": false },
                "intent": "bug_report",
                "payload": "simple string"
            }"#
            .to_string(),
        ),
    ]
}

/// Spawns a CLI command runner for the `k0maru-router` binary.
pub fn router_cmd() -> Command {
    Command::cargo_bin("k0maru-router")
        .expect("The k0maru-router binary must be buildable via cargo_bin")
}
