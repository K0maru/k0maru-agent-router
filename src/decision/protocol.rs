//! System 1 wire protocol serde data models for Laya and Jev decision engines.
//!
//! Conforms to `POST /v1/systemone` specification and the `choice` primitive.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::envelope::TaskEnvelope;

fn default_action() -> String {
    "act".to_string()
}

/// A candidate target option for the System 1 choice primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceCandidate {
    /// Unique identifier for the candidate target.
    pub id: String,
    /// Semantic description or role of the candidate target.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl ChoiceCandidate {
    /// Creates a new candidate with an identifier and semantic description.
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
        }
    }
}

/// Wire request payload for `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneRequest {
    /// Primitive name: strictly "choice" for candidate classification.
    pub primitive: String,
    /// Aggregated task context for single forward pass classification.
    pub context: String,
    /// Set of candidate choices evaluated by the model.
    pub candidates: Vec<ChoiceCandidate>,
    /// Optional task envelope identifier for audit tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl SystemOneRequest {
    /// Creates a new `choice` primitive request.
    pub fn choice(context: impl Into<String>, candidates: Vec<ChoiceCandidate>) -> Self {
        Self {
            primitive: "choice".to_string(),
            context: context.into(),
            candidates,
            task_id: None,
        }
    }

    /// Constructs a `SystemOneRequest` from a `TaskEnvelope` and candidate slice.
    pub fn from_envelope(envelope: &TaskEnvelope, candidates: &[(&str, &str)]) -> Self {
        let mut context = format!(
            "Task: {}\nIntent: {}\nSeverity: {}\nCaller Workspace: {}\n",
            envelope.payload.title,
            envelope.intent,
            envelope.payload.severity,
            envelope.caller.workspace
        );

        if let Some(ref repro) = envelope.payload.reproduction {
            if let Some(obj) = repro.as_object() {
                if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
                    context.push_str(&format!("Error: {}\n", err));
                }
                if let Some(snip) = obj.get("snippet").and_then(|v| v.as_str()) {
                    context.push_str(&format!("Snippet: {}\n", snip));
                }
            }
        }

        let cand_vec = candidates
            .iter()
            .map(|(id, desc)| ChoiceCandidate::new(*id, *desc))
            .collect();

        Self {
            primitive: "choice".to_string(),
            context,
            candidates: cand_vec,
            task_id: Some(envelope.id.clone()),
        }
    }
}

/// Wire response payload returned by `POST /v1/systemone`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    /// The winning candidate identifier chosen by the model.
    pub selected_id: String,
    /// Calibrated model probability / confidence score [0.0, 1.0].
    pub confidence: f64,
    /// Action directive ("act" | "escalate").
    #[serde(default = "default_action")]
    pub action: String,
    /// Optional model reasoning or classification explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// Optional probability distribution across all candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_probabilities: Option<HashMap<String, f64>>,
}
