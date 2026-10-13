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

    /// Execution receipt validation or parsing error.
    #[error("Invalid execution receipt: {0}")]
    InvalidReceipt(String),

    /// Receipt not found in store.
    #[error("Receipt not found for task: '{0}'")]
    ReceiptNotFound(String),

    /// Task signal validation or parsing error.
    #[error("Invalid task signal: {0}")]
    InvalidSignal(String),

    /// Signal not found in store.
    #[error("Signal not found for task: '{0}'")]
    SignalNotFound(String),

    /// Workspace provisioning or teardown error.
    #[error("Workspace error: {0}")]
    WorkspaceError(String),

    /// Pipeline coordination or execution error.
    #[error("Pipeline error: {0}")]
    PipelineError(String),

    /// K-DSL protocol parsing, encoding, or bridging error.
    #[error("K-DSL protocol error: {0}")]
    KdslError(String),

    /// Linda Tuple Space error or invariant failure.
    #[error("Tuple Space error: {0}")]
    TupleSpaceError(String),
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
            (Self::InvalidReceipt(a), Self::InvalidReceipt(b)) => a == b,
            (Self::ReceiptNotFound(a), Self::ReceiptNotFound(b)) => a == b,
            (Self::InvalidSignal(a), Self::InvalidSignal(b)) => a == b,
            (Self::SignalNotFound(a), Self::SignalNotFound(b)) => a == b,
            (Self::WorkspaceError(a), Self::WorkspaceError(b)) => a == b,
            (Self::PipelineError(a), Self::PipelineError(b)) => a == b,
            (Self::KdslError(a), Self::KdslError(b)) => a == b,
            (Self::TupleSpaceError(a), Self::TupleSpaceError(b)) => a == b,
            _ => false,
        }
    }
}
