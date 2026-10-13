//! Task Envelope protocol data models, Serde serialization, and contract validation.

use crate::error::RouterError;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Authoritative default schema URI for Task Envelopes (`agent-task-v1`).
pub const DEFAULT_SCHEMA: &str = "https://k0maru.dev/schemas/agent-task-v1.json";

/// Permitted severity levels for tasks (case-insensitive).
pub const VALID_SEVERITIES: &[&str] = &["blocking", "critical", "high", "medium", "low", "normal"];

/// Information about the calling workspace and agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerInfo {
    pub workspace: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

impl CallerInfo {
    /// Creates a new `CallerInfo` without a pane ID or node.
    pub fn new(workspace: impl Into<String>, agent: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            agent: agent.into(),
            pane_id: None,
            node: None,
        }
    }

    /// Attaches an optional multiplexer pane ID to this `CallerInfo`.
    pub fn with_pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    /// Attaches an optional network node identifier to this `CallerInfo`.
    pub fn with_node(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Validates all field invariants for `CallerInfo`.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.workspace.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'caller.workspace' cannot be empty".to_string(),
            ));
        }
        if self.agent.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'caller.agent' cannot be empty".to_string(),
            ));
        }
        if let Some(pane_id) = &self.pane_id {
            if pane_id.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Task envelope 'caller.pane_id' cannot be empty if specified".to_string(),
                ));
            }
        }
        if let Some(node) = &self.node {
            if node.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Task envelope 'caller.node' cannot be empty if specified".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Destination target repository and agent information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInfo {
    pub identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub is_external: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

impl TargetInfo {
    /// Creates a new internal (trusted) target requiring a workspace path.
    pub fn new_internal(identifier: impl Into<String>, workspace: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            workspace: Some(workspace.into()),
            is_external: false,
            node: None,
        }
    }

    /// Creates a new external (untrusted OSS) target without a local workspace path.
    pub fn new_external(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            workspace: None,
            is_external: true,
            node: None,
        }
    }

    /// Sets or replaces the target workspace path.
    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    /// Sets or replaces the target network node identifier (e.g. "company-linux", "user@100.88.22.33").
    pub fn with_node(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }

    /// Validates target field invariants, enforcing internal workspace requirements.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.identifier.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'target.identifier' cannot be empty".to_string(),
            ));
        }

        if !self.is_external {
            match &self.workspace {
                Some(ws) if !ws.trim().is_empty() => {}
                _ => {
                    return Err(RouterError::InvalidEnvelope(
                        "Internal target ('is_external: false') requires a non-empty 'target.workspace' path".to_string(),
                    ));
                }
            }
        } else if let Some(ws) = &self.workspace {
            if ws.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Target workspace path cannot be empty if specified".to_string(),
                ));
            }
        }

        if let Some(node) = &self.node {
            if node.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Task envelope 'target.node' cannot be empty if specified".to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// Structured reproduction evidence for bug reports conforming to `agent-task-v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReproductionPayload {
    pub snippet: String,
    pub error: String,
}

impl ReproductionPayload {
    /// Constructs a new `ReproductionPayload`.
    pub fn new(snippet: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            snippet: snippet.into(),
            error: error.into(),
        }
    }

    /// Validates that reproduction snippet and error are non-empty.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.snippet.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Reproduction 'snippet' cannot be empty".to_string(),
            ));
        }
        if self.error.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Reproduction 'error' cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Converts this reproduction payload into a `serde_json::Value`.
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    /// Parses a reproduction payload from a `serde_json::Value`.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value.clone())
    }
}

impl From<ReproductionPayload> for serde_json::Value {
    fn from(payload: ReproductionPayload) -> Self {
        serde_json::json!({
            "snippet": payload.snippet,
            "error": payload.error,
        })
    }
}

impl TryFrom<&serde_json::Value> for ReproductionPayload {
    type Error = serde_json::Error;

    fn try_from(val: &serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(val.clone())
    }
}

impl TryFrom<serde_json::Value> for ReproductionPayload {
    type Error = serde_json::Error;

    fn try_from(val: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(val)
    }
}

/// Task content payload including title, severity, reproduction, and expected behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPayload {
    pub title: String,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reproduction: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}

impl TaskPayload {
    /// Creates a new `TaskPayload` with required title and severity.
    pub fn new(title: impl Into<String>, severity: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            severity: severity.into(),
            reproduction: None,
            expected: None,
        }
    }

    /// Sets raw reproduction data from any JSON-serializable value.
    pub fn with_reproduction(mut self, reproduction: impl Into<serde_json::Value>) -> Self {
        self.reproduction = Some(reproduction.into());
        self
    }

    /// Sets strongly-typed structured reproduction payload.
    pub fn with_reproduction_payload(mut self, repro: ReproductionPayload) -> Self {
        self.reproduction = Some(repro.into());
        self
    }

    /// Sets the optional expected behavior description.
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// Attempts to parse the reproduction field as a typed `ReproductionPayload`.
    pub fn reproduction_payload(&self) -> Option<Result<ReproductionPayload, serde_json::Error>> {
        self.reproduction
            .as_ref()
            .map(ReproductionPayload::try_from)
    }

    /// Validates payload invariants: non-empty strings, valid severity, and reproduction snippet/error.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.title.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'payload.title' cannot be empty".to_string(),
            ));
        }

        if self.severity.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'payload.severity' cannot be empty".to_string(),
            ));
        }

        let normalized_severity = self.severity.trim().to_lowercase();
        if !VALID_SEVERITIES.contains(&normalized_severity.as_str()) {
            return Err(RouterError::InvalidEnvelope(format!(
                "Invalid severity '{}': must be one of [{}]",
                self.severity,
                VALID_SEVERITIES.join(", ")
            )));
        }

        if let Some(expected) = &self.expected {
            if expected.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Task envelope 'payload.expected' cannot be empty if specified".to_string(),
                ));
            }
        }

        if let Some(repro) = &self.reproduction {
            if let Some(obj) = repro.as_object() {
                if let Some(snippet_val) = obj.get("snippet") {
                    if let Some(s) = snippet_val.as_str() {
                        if s.trim().is_empty() {
                            return Err(RouterError::InvalidEnvelope(
                                "Reproduction 'snippet' cannot be empty if specified".to_string(),
                            ));
                        }
                    }
                }
                if let Some(error_val) = obj.get("error") {
                    if let Some(e) = error_val.as_str() {
                        if e.trim().is_empty() {
                            return Err(RouterError::InvalidEnvelope(
                                "Reproduction 'error' cannot be empty if specified".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// The top-level immutable agent task envelope (`agent-task-v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEnvelope {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub id: String,
    pub created_at: String,
    pub caller: CallerInfo,
    pub target: TargetInfo,
    pub intent: String,
    pub payload: TaskPayload,
}

impl TaskEnvelope {
    /// Constructs a new `TaskEnvelope` with default schema URI.
    pub fn new(
        id: impl Into<String>,
        created_at: impl Into<String>,
        caller: CallerInfo,
        target: TargetInfo,
        intent: impl Into<String>,
        payload: TaskPayload,
    ) -> Self {
        Self {
            schema: Some(DEFAULT_SCHEMA.to_string()),
            id: id.into(),
            created_at: created_at.into(),
            caller,
            target,
            intent: intent.into(),
            payload,
        }
    }

    /// Ingests and validates a TaskEnvelope from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self, RouterError> {
        if json.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope JSON string cannot be empty".to_string(),
            ));
        }
        let envelope: Self = serde_json::from_str(json)?;
        envelope.validate()?;
        Ok(envelope)
    }

    /// Reads and validates a TaskEnvelope from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, RouterError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Self::from_json_str(&content)
    }

    /// Serializes envelope to a JSON string after validation.
    pub fn to_json_string(&self) -> Result<String, RouterError> {
        self.validate()?;
        Ok(serde_json::to_string(self)?)
    }

    /// Serializes envelope to pretty-printed JSON string after validation.
    pub fn to_json_pretty(&self) -> Result<String, RouterError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Validates all contract invariants according to `agent-task-v1`.
    pub fn validate(&self) -> Result<(), RouterError> {
        if self.id.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'id' cannot be empty".to_string(),
            ));
        }

        if self.id.contains("..") || self.id.contains('/') || self.id.contains('\\') {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'id' cannot contain path traversal or separator characters"
                    .to_string(),
            ));
        }

        if self.created_at.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'created_at' cannot be empty".to_string(),
            ));
        }

        DateTime::parse_from_rfc3339(&self.created_at).map_err(|e| {
            RouterError::InvalidEnvelope(format!(
                "Task envelope 'created_at' must be a valid RFC-3339 timestamp: {e}"
            ))
        })?;

        if self.intent.trim().is_empty() {
            return Err(RouterError::InvalidEnvelope(
                "Task envelope 'intent' cannot be empty".to_string(),
            ));
        }

        if let Some(schema) = &self.schema {
            if schema.trim().is_empty() {
                return Err(RouterError::InvalidEnvelope(
                    "Task envelope '$schema' cannot be empty if specified".to_string(),
                ));
            }
        }

        self.caller.validate()?;
        self.target.validate()?;
        self.payload.validate()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_spec_json() -> &'static str {
        r#"{
            "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
            "id": "task_9f82a1b4",
            "created_at": "2026-10-11T18:30:00Z",
            "caller": {
                "workspace": "/Users/k0maru3/workspace/consumer-app",
                "agent": "agy",
                "pane_id": "%1"
            },
            "target": {
                "identifier": "Terra-UI",
                "workspace": "/Users/k0maru3/workspace/terra-ui",
                "is_external": false
            },
            "intent": "bug_report",
            "payload": {
                "title": "TerraButton missing aria-label attribute pass-through",
                "severity": "blocking",
                "reproduction": {
                    "snippet": "<TerraButton aria-label='close' />",
                    "error": "HTML button element does not receive aria-label"
                },
                "expected": "Attribute forwarded to rendered <button> element"
            }
        }"#
    }

    #[test]
    fn test_valid_canonical_envelope_from_json_str() {
        let envelope = TaskEnvelope::from_json_str(sample_spec_json())
            .expect("Canonical envelope from SPEC.md §2.1 must parse and validate cleanly");

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
        assert!(!envelope.target.is_external);
        assert_eq!(envelope.intent, "bug_report");
        assert_eq!(
            envelope.payload.title,
            "TerraButton missing aria-label attribute pass-through"
        );
        assert_eq!(envelope.payload.severity, "blocking");

        let repro = envelope
            .payload
            .reproduction_payload()
            .expect("Reproduction must be present")
            .expect("Reproduction must parse as typed ReproductionPayload");
        assert_eq!(repro.snippet, "<TerraButton aria-label='close' />");
        assert_eq!(
            repro.error,
            "HTML button element does not receive aria-label"
        );
        assert_eq!(
            envelope.payload.expected.as_deref(),
            Some("Attribute forwarded to rendered <button> element")
        );
    }

    #[test]
    fn test_valid_minimal_envelope_omitting_optional_fields() {
        let json = r#"{
            "id": "task_min_001",
            "created_at": "2026-10-11T12:00:00Z",
            "caller": {
                "workspace": "/tmp/caller",
                "agent": "worker-1"
            },
            "target": {
                "identifier": "target-service",
                "workspace": "/tmp/target",
                "is_external": false
            },
            "intent": "feature_request",
            "payload": {
                "title": "Add telemetry exporter",
                "severity": "low"
            }
        }"#;

        let envelope = TaskEnvelope::from_json_str(json).expect("Minimal envelope must be valid");
        assert_eq!(envelope.schema, None);
        assert_eq!(envelope.caller.pane_id, None);
        assert_eq!(envelope.payload.reproduction, None);
        assert_eq!(envelope.payload.expected, None);
        assert!(!envelope.target.is_external);
    }

    #[test]
    fn test_valid_external_target_without_workspace() {
        let json = r#"{
            "id": "task_ext_oss",
            "created_at": "2026-10-11T14:00:00Z",
            "caller": {
                "workspace": "/home/user/app",
                "agent": "agy"
            },
            "target": {
                "identifier": "react",
                "is_external": true
            },
            "intent": "bug_report",
            "payload": {
                "title": "React 19 hydration mismatch",
                "severity": "critical"
            }
        }"#;

        let envelope =
            TaskEnvelope::from_json_str(json).expect("External target without workspace is valid");
        assert!(envelope.target.is_external);
        assert!(envelope.target.workspace.is_none());
    }

    #[test]
    fn test_valid_external_target_with_optional_workspace() {
        let json = r#"{
            "id": "task_ext_with_ws",
            "created_at": "2026-10-11T14:00:00Z",
            "caller": {
                "workspace": "/home/user/app",
                "agent": "agy"
            },
            "target": {
                "identifier": "vitest",
                "workspace": "/tmp/vitest-clone",
                "is_external": true
            },
            "intent": "bug_report",
            "payload": {
                "title": "Vitest runner hang",
                "severity": "high"
            }
        }"#;

        let envelope = TaskEnvelope::from_json_str(json)
            .expect("External target with optional workspace is valid");
        assert!(envelope.target.is_external);
        assert_eq!(
            envelope.target.workspace.as_deref(),
            Some("/tmp/vitest-clone")
        );
    }

    #[test]
    fn test_rfc3339_timestamp_variations_and_failures() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();

        // Valid timestamp formats
        for valid_ts in &[
            "2026-10-11T18:30:00Z",
            "2026-10-11T18:30:00.123456Z",
            "2026-10-11T18:30:00+08:00",
            "2026-10-11T18:30:00-05:00",
            "2026-10-11T18:30:00.000+00:00",
        ] {
            envelope.created_at = valid_ts.to_string();
            assert!(
                envelope.validate().is_ok(),
                "Timestamp '{}' must be valid RFC-3339",
                valid_ts
            );
        }

        // Invalid timestamp formats
        for invalid_ts in &[
            "",
            "   ",
            "not-a-timestamp",
            "2026-10-11",
            "2026-10-11 18:30:00",
            "2026-10-11T18:30:00",  // missing timezone
            "2026-02-31T00:00:00Z", // invalid calendar date
            "2026-10-11T25:00:00Z", // invalid hour
        ] {
            envelope.created_at = invalid_ts.to_string();
            assert!(
                matches!(envelope.validate(), Err(RouterError::InvalidEnvelope(_))),
                "Timestamp '{}' must be rejected as invalid RFC-3339",
                invalid_ts
            );
        }
    }

    #[test]
    fn test_non_empty_string_validation() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();

        // Empty id
        envelope.id = "".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.id = "   ".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Restore id
        envelope.id = "task_valid_1".to_string();

        // Empty caller.workspace
        envelope.caller.workspace = "".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.caller.workspace = "/valid/ws".to_string();

        // Empty caller.agent
        envelope.caller.agent = "  ".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.caller.agent = "valid_agent".to_string();

        // Empty caller.pane_id when Some
        envelope.caller.pane_id = Some("".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.caller.pane_id = None;

        // Empty target.identifier
        envelope.target.identifier = "".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.target.identifier = "valid_target".to_string();

        // Empty intent
        envelope.intent = "   ".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.intent = "bug_report".to_string();

        // Empty payload.title
        envelope.payload.title = "".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.payload.title = "Valid Title".to_string();

        // Empty payload.severity
        envelope.payload.severity = "".to_string();
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.payload.severity = "high".to_string();

        // Empty payload.expected when Some
        envelope.payload.expected = Some("  ".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.payload.expected = None;

        // Empty schema when Some
        envelope.schema = Some("".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));
        envelope.schema = None;

        assert!(envelope.validate().is_ok());
    }

    #[test]
    fn test_id_path_traversal_protection() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();

        for traversal_id in &["../hack", "task/slash", "task\\backslash", "foo/../bar"] {
            envelope.id = traversal_id.to_string();
            assert!(
                matches!(envelope.validate(), Err(RouterError::InvalidEnvelope(_))),
                "Path traversal id '{}' must be rejected",
                traversal_id
            );
        }
    }

    #[test]
    fn test_internal_target_workspace_invariant() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();
        envelope.target.is_external = false;

        // Internal target without workspace is rejected
        envelope.target.workspace = None;
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Internal target with empty workspace is rejected
        envelope.target.workspace = Some("".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        envelope.target.workspace = Some("   ".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Internal target with valid workspace succeeds
        envelope.target.workspace = Some("/valid/internal/workspace".to_string());
        assert!(envelope.validate().is_ok());

        // External target with empty workspace string is rejected
        envelope.target.is_external = true;
        envelope.target.workspace = Some("".to_string());
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // External target with None workspace succeeds
        envelope.target.workspace = None;
        assert!(envelope.validate().is_ok());
    }

    #[test]
    fn test_severity_level_validation() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();

        for valid_sev in &[
            "blocking", "critical", "high", "medium", "low", "normal", "BLOCKING", "Critical",
            "High", "NORMAL",
        ] {
            envelope.payload.severity = valid_sev.to_string();
            assert!(
                envelope.validate().is_ok(),
                "Severity '{}' should be accepted",
                valid_sev
            );
        }

        for invalid_sev in &["urgent", "fatal", "unknown", "catastrophic", "p0", ""] {
            envelope.payload.severity = invalid_sev.to_string();
            assert!(
                matches!(envelope.validate(), Err(RouterError::InvalidEnvelope(_))),
                "Severity '{}' should be rejected",
                invalid_sev
            );
        }
    }

    #[test]
    fn test_reproduction_payload_behavior() {
        let repro = ReproductionPayload::new("<div>test</div>", "Assertion error at line 42");
        assert!(repro.validate().is_ok());

        let invalid_snippet = ReproductionPayload::new("", "some error");
        assert!(matches!(
            invalid_snippet.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        let invalid_error = ReproductionPayload::new("some snippet", "");
        assert!(matches!(
            invalid_error.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Value conversion
        let val = repro.to_value().expect("to_value should succeed");
        let parsed =
            ReproductionPayload::from_value(&val).expect("from_value should roundtrip cleanly");
        assert_eq!(repro, parsed);

        // TaskPayload integration
        let mut payload = TaskPayload::new("Test bug", "medium");
        payload = payload.with_reproduction_payload(repro.clone());
        let extracted = payload
            .reproduction_payload()
            .expect("should be present")
            .expect("should deserialize");
        assert_eq!(repro, extracted);
    }

    #[test]
    fn test_reproduction_in_envelope_validation() {
        let mut envelope = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();

        // Empty snippet inside object reproduction is caught
        envelope.payload.reproduction = Some(serde_json::json!({
            "snippet": "",
            "error": "some error"
        }));
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Empty error inside object reproduction is caught
        envelope.payload.reproduction = Some(serde_json::json!({
            "snippet": "some snippet",
            "error": "   "
        }));
        assert!(matches!(
            envelope.validate(),
            Err(RouterError::InvalidEnvelope(_))
        ));

        // Arbitrary JSON reproduction is permitted
        envelope.payload.reproduction = Some(serde_json::json!({
            "command": "cargo test",
            "exit_code": 1
        }));
        assert!(envelope.validate().is_ok());
    }

    #[test]
    fn test_from_json_str_empty_and_malformed() {
        assert!(matches!(
            TaskEnvelope::from_json_str(""),
            Err(RouterError::InvalidEnvelope(_))
        ));
        assert!(matches!(
            TaskEnvelope::from_json_str("   \n\t  "),
            Err(RouterError::InvalidEnvelope(_))
        ));
        assert!(matches!(
            TaskEnvelope::from_json_str("{ not valid json "),
            Err(RouterError::Json(_))
        ));
    }

    #[test]
    fn test_from_file_success_and_failure() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        write!(temp_file, "{}", sample_spec_json()).expect("Failed to write temp file");

        let loaded = TaskEnvelope::from_file(temp_file.path())
            .expect("from_file should load and validate envelope");
        assert_eq!(loaded.id, "task_9f82a1b4");

        let bad_path = std::env::temp_dir().join("non_existent_envelope_file_9999.json");
        let err = TaskEnvelope::from_file(&bad_path);
        assert!(matches!(err, Err(RouterError::Io(_))));
    }

    #[test]
    fn test_serde_roundtrip_and_pretty() {
        let original = TaskEnvelope::from_json_str(sample_spec_json()).unwrap();
        let json_str = original
            .to_json_string()
            .expect("Serialization to string should succeed");
        let deserialized =
            TaskEnvelope::from_json_str(&json_str).expect("Deserialization should succeed");
        assert_eq!(original, deserialized);

        let pretty_str = original
            .to_json_pretty()
            .expect("Serialization to pretty string should succeed");
        let deserialized_pretty =
            TaskEnvelope::from_json_str(&pretty_str).expect("Deserialization should succeed");
        assert_eq!(original, deserialized_pretty);
    }

    #[test]
    fn test_optional_fields_skip_serializing() {
        let envelope = TaskEnvelope {
            schema: None,
            id: "task_clean".to_string(),
            created_at: "2026-10-11T12:00:00Z".to_string(),
            caller: CallerInfo::new("/ws/caller", "agent-x"),
            target: TargetInfo::new_external("ext-dep"),
            intent: "fix".to_string(),
            payload: TaskPayload::new("Fix something", "low"),
        };

        let json = envelope.to_json_string().unwrap();
        assert!(!json.contains("$schema"));
        assert!(!json.contains("pane_id"));
        assert!(!json.contains("workspace\":null"));
        assert!(!json.contains("reproduction"));
        assert!(!json.contains("expected"));

        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(val["target"].get("workspace").is_none());
        assert!(val["caller"].get("pane_id").is_none());
        assert!(val["payload"].get("reproduction").is_none());
        assert!(val["payload"].get("expected").is_none());

        let roundtrip = TaskEnvelope::from_json_str(&json).unwrap();
        assert_eq!(envelope, roundtrip);
    }

    #[test]
    fn test_builders_and_constructors() {
        let caller = CallerInfo::new("/path/c", "agent-1").with_pane_id("%2");
        assert_eq!(caller.pane_id.as_deref(), Some("%2"));

        let target_int = TargetInfo::new_internal("core", "/path/t");
        assert!(!target_int.is_external);
        assert_eq!(target_int.workspace.as_deref(), Some("/path/t"));

        let target_ext = TargetInfo::new_external("ext").with_workspace("/tmp/clone");
        assert!(target_ext.is_external);
        assert_eq!(target_ext.workspace.as_deref(), Some("/tmp/clone"));

        let payload = TaskPayload::new("Title", "high")
            .with_reproduction_payload(ReproductionPayload::new("snippet", "error"))
            .with_expected("Success");
        assert_eq!(payload.expected.as_deref(), Some("Success"));

        let envelope = TaskEnvelope::new(
            "task_builder_1",
            "2026-10-11T10:00:00Z",
            caller,
            target_int,
            "bug_report",
            payload,
        );
        assert_eq!(
            envelope.schema.as_deref(),
            Some("https://k0maru.dev/schemas/agent-task-v1.json")
        );
        assert!(envelope.validate().is_ok());
    }

    #[test]
    fn test_node_in_caller_and_target() {
        let caller = CallerInfo::new("/path/c", "agy").with_node("personal-mac");
        assert_eq!(caller.node.as_deref(), Some("personal-mac"));

        let target =
            TargetInfo::new_internal("backend", "/home/user/backend").with_node("company-linux");
        assert_eq!(target.node.as_deref(), Some("company-linux"));

        let envelope = TaskEnvelope::new(
            "task_remote_1",
            "2026-10-13T10:00:00Z",
            caller,
            target,
            "feature",
            TaskPayload::new("Remote task", "normal"),
        );
        assert!(envelope.validate().is_ok());

        let json = envelope.to_json_string().unwrap();
        assert!(json.contains("\"node\":\"personal-mac\""));
        assert!(json.contains("\"node\":\"company-linux\""));

        let roundtrip = TaskEnvelope::from_json_str(&json).unwrap();
        assert_eq!(roundtrip.caller.node.as_deref(), Some("personal-mac"));
        assert_eq!(roundtrip.target.node.as_deref(), Some("company-linux"));
    }
}
