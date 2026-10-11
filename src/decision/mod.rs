//! Decision Engine Service Provider Interface (SPI) for k0maru-agent-router.
//!
//! Provides pluggable resolution for ambiguous target tasks across deterministic rules,
//! non-autoregressive System 1 models (Laya/ModernBERT), cloud fallbacks (Jev), and
//! cascading fallback gates. Conforms to SPEC.md §4.

pub mod cascading;
pub mod laya;
pub mod protocol;
pub mod rule;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::envelope::TaskEnvelope;
use crate::error::RouterError;

pub use cascading::CascadingDecisionEngine;
pub use laya::LayaDecisionClient;
pub use protocol::{ChoiceCandidate, SystemOneRequest, SystemOneResponse};
pub use rule::{DeterministicRuleEngine, MatchPattern, NoopDecisionEngine, Rule};

/// Result outcome evaluated by a `DecisionEngine` for a candidate task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRoutingOutcome {
    /// Selected or suggested target repository/agent identifier.
    pub target_identifier: String,
    /// Mathematically calibrated confidence score in the range [0.0, 1.0].
    pub confidence: f64,
    /// Identifying source of the deciding engine (e.g. "rule", "laya", "noop").
    pub engine_source: &'static str,
    /// Gate action directive: "act" for auto-dispatch or "escalate" for quarantine.
    pub action_gate: String,
    /// Whether this task must be staged to the HITL Outbox rather than direct dispatched.
    pub should_stage_outbox: bool,
}

impl DecisionRoutingOutcome {
    /// Creates an accepted outcome that can act immediately without human quarantine.
    pub fn act(
        target_identifier: impl Into<String>,
        confidence: f64,
        engine_source: &'static str,
    ) -> Self {
        Self {
            target_identifier: target_identifier.into(),
            confidence,
            engine_source,
            action_gate: "act".to_string(),
            should_stage_outbox: false,
        }
    }

    /// Creates an escalated outcome requiring staging to the HITL Outbox.
    pub fn escalate(
        target_identifier: impl Into<String>,
        confidence: f64,
        engine_source: &'static str,
    ) -> Self {
        Self {
            target_identifier: target_identifier.into(),
            confidence,
            engine_source,
            action_gate: "escalate".to_string(),
            should_stage_outbox: true,
        }
    }

    /// Returns `true` if this outcome allows immediate autonomous action.
    pub fn is_act(&self) -> bool {
        self.action_gate == "act" && !self.should_stage_outbox
    }
}

/// Pluggable Decision Engine SPI for evaluating ambiguous tasks against candidates.
#[async_trait]
pub trait DecisionEngine: Send + Sync + std::fmt::Debug {
    /// Engine identifier name (e.g. "rule", "laya", "cascading", "noop").
    fn name(&self) -> &'static str;

    /// Availability and health probe.
    async fn is_healthy(&self) -> bool;

    /// Evaluates a `TaskEnvelope` against a list of candidate targets.
    ///
    /// Candidates are pairs of `(identifier, semantic_description)`.
    async fn evaluate_task(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError>;
}
