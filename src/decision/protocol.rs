//! System 1 wire protocol serde data models for Laya and Jev decision engines.
//!
//! Conforms to `POST /v1/systemone` specification and the `choice` primitive.

use serde::{Deserialize, Deserializer, Serialize};
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

/// Question specification conforming to Laya / TypeSafe Jev `choice` schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    /// Primitive question type: strictly "choice".
    #[serde(rename = "type")]
    pub question_type: String,
    /// Natural language instructions guiding the model forward pass.
    pub instructions: String,
    /// Mapping of candidate identifiers to semantic criteria.
    pub criteria: HashMap<String, String>,
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
    /// State map required by Laya / Jev runtime: `{"request": context}`.
    pub state: HashMap<String, String>,
    /// Question specification required by Laya / Jev: `{"target": {"type": "choice", ...}}`.
    pub questions: HashMap<String, ChoiceQuestion>,
    /// Optional task envelope identifier for audit tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl SystemOneRequest {
    /// Creates a new `choice` primitive request.
    pub fn choice(context: impl Into<String>, candidates: Vec<ChoiceCandidate>) -> Self {
        let ctx = context.into();
        let mut criteria = HashMap::new();
        for c in &candidates {
            let desc = if c.description.trim().is_empty() {
                c.id.clone()
            } else {
                c.description.clone()
            };
            criteria.insert(c.id.clone(), desc);
        }

        let mut questions = HashMap::new();
        questions.insert(
            "target".to_string(),
            ChoiceQuestion {
                question_type: "choice".to_string(),
                instructions: "Which workspace or candidate should handle `request`?".to_string(),
                criteria,
            },
        );

        let mut state = HashMap::new();
        state.insert("request".to_string(), ctx.clone());

        Self {
            primitive: "choice".to_string(),
            context: ctx,
            candidates,
            state,
            questions,
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

        let cand_vec: Vec<ChoiceCandidate> = candidates
            .iter()
            .map(|(id, desc)| ChoiceCandidate::new(*id, *desc))
            .collect();

        let mut req = Self::choice(context, cand_vec);
        req.task_id = Some(envelope.id.clone());
        req
    }
}

/// Action head payload returned by Laya ("act" or object with probability).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayaAnswerAction {
    #[serde(default)]
    pub act_probability: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayaActionValue {
    String(String),
    Object(LayaAnswerAction),
}

/// Answer object returned by Laya or TypeSafe Jev under `answers[<question_id>]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayaAnswer {
    #[serde(rename = "type", default)]
    pub answer_type: String,
    #[serde(default)]
    pub choice: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub answer_confidence: Option<f64>,
    #[serde(default)]
    pub x_jev_confidence: Option<f64>,
    #[serde(default)]
    pub probabilities: Option<HashMap<String, f64>>,
    #[serde(default)]
    pub action: Option<LayaActionValue>,
}

/// Wire response payload returned by `POST /v1/systemone`.
///
/// Seamlessly deserializes both native Laya / TypeSafe Jev responses
/// (`answers.target.choice`, `answer_confidence`) and simplified direct responses.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemOneResponse {
    /// The winning candidate identifier chosen by the model.
    pub selected_id: String,
    /// Calibrated model probability / confidence score [0.0, 1.0].
    pub confidence: f64,
    /// Action directive ("act" | "escalate").
    pub action: String,
    /// Optional model reasoning or classification explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// Optional probability distribution across all candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_probabilities: Option<HashMap<String, f64>>,
    /// Underlying model identifier returned by Laya/Jev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Full answers map if returned by Laya / Jev.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub answers: HashMap<String, LayaAnswer>,
}

impl<'de> Deserialize<'de> for SystemOneResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawResponse {
            #[serde(default)]
            selected_id: Option<String>,
            #[serde(default)]
            confidence: Option<f64>,
            #[serde(default)]
            action: Option<LayaActionValue>,
            #[serde(default)]
            explanation: Option<String>,
            #[serde(default)]
            raw_probabilities: Option<HashMap<String, f64>>,
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            answers: HashMap<String, LayaAnswer>,
        }

        let raw = RawResponse::deserialize(deserializer)?;

        let mut selected_id = raw.selected_id.unwrap_or_default();
        let mut confidence = raw.confidence.unwrap_or(0.0);
        let mut raw_probabilities = raw.raw_probabilities;
        let mut action = match raw.action {
            Some(LayaActionValue::String(s)) => s,
            Some(LayaActionValue::Object(obj)) => {
                if let Some(p) = obj.act_probability {
                    if p >= 0.5 {
                        "act".to_string()
                    } else {
                        "escalate".to_string()
                    }
                } else {
                    default_action()
                }
            }
            None => default_action(),
        };

        // If selected_id is empty or missing, resolve from answers.get("target") or first answer
        if selected_id.is_empty() {
            let target_answer = raw
                .answers
                .get("target")
                .or_else(|| raw.answers.values().next());
            if let Some(ans) = target_answer {
                if let Some(ref c) = ans.choice {
                    selected_id = c.clone();
                }
                if confidence == 0.0 {
                    confidence = ans
                        .answer_confidence
                        .or(ans.x_jev_confidence)
                        .or(ans.confidence)
                        .unwrap_or(0.0);
                }
                if raw_probabilities.is_none() {
                    raw_probabilities = ans.probabilities.clone();
                }
                if let Some(ref act) = ans.action {
                    action = match act {
                        LayaActionValue::String(s) => s.clone(),
                        LayaActionValue::Object(obj) => {
                            if let Some(p) = obj.act_probability {
                                if p >= 0.5 {
                                    "act".to_string()
                                } else {
                                    "escalate".to_string()
                                }
                            } else {
                                default_action()
                            }
                        }
                    };
                }
            }
        }

        Ok(SystemOneResponse {
            selected_id,
            confidence,
            action,
            explanation: raw.explanation,
            raw_probabilities,
            model: raw.model,
            answers: raw.answers,
        })
    }
}
