//! Typed error definitions for the k0maru-agent-router microkernel.

use std::path::PathBuf;
use thiserror::Error;

/// Authoritative typed errors for k0maru-agent-router conforming to SPEC.md §7.
#[derive(Debug, Error)]
pub enum RouterError {
    /// Validation failure or malformed Task Envelope syntax.
    #[error("Invalid task envelope: {0}")]
    InvalidEnvelope(String),

    /// Staged issue draft not found in HITL outbox.
    #[error("Staged issue not found: '{0}'")]
    IssueNotFound(String),

    /// Target workspace directory does not exist on disk.
    #[error("Target workspace not found: '{0}'")]
    WorkspaceNotFound(PathBuf),

    /// Terminal multiplexer driver is missing or unreachable.
    #[error("Multiplexer driver '{driver}' unavailable: {reason}")]
    DriverUnavailable {
        driver: &'static str,
        reason: String,
    },

    /// Multiplexer driver command execution failed.
    #[error("Multiplexer driver execution failed: {0}")]
    DriverExecutionFailed(String),

    /// HITL Outbox filesystem IO or staging failure.
    #[error("HITL Outbox error: {0}")]
    OutboxError(String),

    /// Multiplexer session or pane was not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// General filesystem IO error.
    #[error("Filesystem IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization error.
    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// RFC-3339 / ISO-8601 timestamp parsing error.
    #[error("Timestamp parsing error: {0}")]
    DateTimeParse(#[from] chrono::ParseError),
}

impl PartialEq for RouterError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidEnvelope(a), Self::InvalidEnvelope(b)) => a == b,
            (Self::IssueNotFound(a), Self::IssueNotFound(b)) => a == b,
            (Self::WorkspaceNotFound(a), Self::WorkspaceNotFound(b)) => a == b,
            (
                Self::DriverUnavailable {
                    driver: d1,
                    reason: r1,
                },
                Self::DriverUnavailable {
                    driver: d2,
                    reason: r2,
                },
            ) => d1 == d2 && r1 == r2,
            (Self::DriverExecutionFailed(a), Self::DriverExecutionFailed(b)) => a == b,
            (Self::OutboxError(a), Self::OutboxError(b)) => a == b,
            (Self::SessionNotFound(a), Self::SessionNotFound(b)) => a == b,
            (Self::Io(a), Self::Io(b)) => a.kind() == b.kind(),
            (Self::Json(a), Self::Json(b)) => a.to_string() == b.to_string(),
            (Self::DateTimeParse(a), Self::DateTimeParse(b)) => a == b,
            _ => false,
        }
    }
}
