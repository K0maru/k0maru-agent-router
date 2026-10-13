//! Linda Tuple definition and pattern matching for K0maru Tuple Space.
//!
//! Provides associative shared-memory primitives conforming to SPEC-0006.

use serde::{Deserialize, Serialize};

/// Linda Tuple representing an associative unit of shared state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tuple {
    /// Unique tuple identifier (e.g., `tup_9f82a1b4`).
    pub id: String,
    /// Tuple classification or event kind (e.g. "TASK_READY", "TASK_CLAIMED", "TASK_COMPLETED", "TASK_FAILED").
    pub kind: String,
    /// Target repository or domain identifier (e.g. "AUTH-SRV", "SecondBrain").
    pub domain: String,
    /// Associated task identifier.
    pub task_id: String,
    /// Arbitrary payload, such as a K-DSL wire string or JSON object.
    pub payload: String,
    /// Creation timestamp in RFC-3339 format.
    pub created_at: String,
    /// Optional time-to-live in seconds before tuple becomes stale/expired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
}

impl Tuple {
    /// Creates a new Tuple with an auto-generated unique ID and current UTC timestamp.
    pub fn new(
        kind: impl Into<String>,
        domain: impl Into<String>,
        task_id: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("tup_{}", uuid::Uuid::new_v4().simple()),
            kind: kind.into(),
            domain: domain.into(),
            task_id: task_id.into(),
            payload: payload.into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            ttl_secs: None,
        }
    }

    /// Sets explicit tuple identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Sets optional time-to-live in seconds.
    pub fn with_ttl(mut self, ttl_secs: u64) -> Self {
        self.ttl_secs = Some(ttl_secs);
        self
    }

    /// Sets explicit creation timestamp (RFC-3339).
    pub fn with_created_at(mut self, created_at: impl Into<String>) -> Self {
        self.created_at = created_at.into();
        self
    }

    /// Checks if the tuple is expired according to its `ttl_secs`.
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl_secs {
            if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&self.created_at) {
                let now = chrono::Utc::now();
                let expires_at = created + chrono::Duration::seconds(ttl as i64);
                return now > expires_at;
            }
        }
        false
    }
}

/// Associative query pattern for matching Tuples in the space.
///
/// Any `None` field acts as an associative wildcard matching any value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TuplePattern {
    /// Optional exact match on `kind`.
    pub kind: Option<String>,
    /// Optional exact match on `domain`.
    pub domain: Option<String>,
    /// Optional exact match on `task_id`.
    pub task_id: Option<String>,
}

impl TuplePattern {
    /// Creates a new wildcard pattern matching all tuples.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new wildcard pattern matching all tuples.
    pub fn any() -> Self {
        Self::default()
    }

    /// Match a specific kind.
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Match a specific domain.
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    /// Match a specific task_id.
    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Returns `true` if the tuple satisfies the query pattern.
    pub fn matches(&self, tuple: &Tuple) -> bool {
        if let Some(ref kind) = self.kind {
            if &tuple.kind != kind {
                return false;
            }
        }
        if let Some(ref domain) = self.domain {
            if &tuple.domain != domain {
                return false;
            }
        }
        if let Some(ref task_id) = self.task_id {
            if &tuple.task_id != task_id {
                return false;
            }
        }
        true
    }
}
