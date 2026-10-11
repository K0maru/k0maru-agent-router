//! Cascading Multi-Tier Decision Engine.
//!
//! Evaluates candidate decision engines in strict priority order:
//! Deterministic Rule ➔ Local Laya ➔ Cloud Jev ➔ HITL Outbox Quarantine Gate.

use async_trait::async_trait;

use super::{DecisionEngine, DecisionRoutingOutcome};
use crate::envelope::TaskEnvelope;
use crate::error::RouterError;

const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.85;

/// Priority-based cascading decision engine orchestrator.
#[derive(Debug)]
pub struct CascadingDecisionEngine {
    engines: Vec<Box<dyn DecisionEngine>>,
    confidence_threshold: f64,
}

impl Default for CascadingDecisionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CascadingDecisionEngine {
    /// Creates an empty cascading engine.
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }

    /// Appends a decision engine to the priority cascade chain.
    pub fn with_engine(mut self, engine: impl DecisionEngine + 'static) -> Self {
        self.engines.push(Box::new(engine));
        self
    }

    /// Configures the minimum confidence threshold required for autonomous action.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.confidence_threshold = threshold;
        self
    }

    /// Returns a slice of configured engines in the cascade.
    pub fn engines(&self) -> &[Box<dyn DecisionEngine>] {
        &self.engines
    }
}

#[async_trait]
impl DecisionEngine for CascadingDecisionEngine {
    fn name(&self) -> &'static str {
        "cascading"
    }

    async fn is_healthy(&self) -> bool {
        if self.engines.is_empty() {
            return false;
        }
        for engine in &self.engines {
            if engine.is_healthy().await {
                return true;
            }
        }
        false
    }

    async fn evaluate_task(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        let mut best_fallback: Option<DecisionRoutingOutcome> = None;

        for engine in &self.engines {
            // Attempt evaluation if engine is healthy
            if !engine.is_healthy().await {
                continue;
            }

            match engine.evaluate_task(envelope, candidates).await {
                Ok(outcome) => {
                    // Fast-path: high confidence and act directive matches immediately
                    if outcome.confidence >= self.confidence_threshold
                        && outcome.action_gate == "act"
                        && !outcome.should_stage_outbox
                    {
                        return Ok(outcome);
                    }

                    // Otherwise track highest confidence candidate for HITL staging
                    let is_better = match &best_fallback {
                        None => true,
                        Some(prev) => outcome.confidence > prev.confidence,
                    };
                    if is_better {
                        best_fallback = Some(outcome);
                    }
                }
                Err(_) => {
                    // Gracefully cascade to next tier upon network or engine failure
                    continue;
                }
            }
        }

        // If no engine decisively accepted, escalate to HITL Outbox
        if let Some(mut outcome) = best_fallback {
            outcome.should_stage_outbox = true;
            outcome.action_gate = "escalate".to_string();
            Ok(outcome)
        } else {
            let fallback_target = candidates.first().map(|(id, _)| *id).unwrap_or("");
            Ok(DecisionRoutingOutcome::escalate(
                fallback_target,
                0.0,
                "cascading",
            ))
        }
    }
}
