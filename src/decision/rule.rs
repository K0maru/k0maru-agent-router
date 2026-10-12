//! Deterministic Rule-Based Decision Engine.
//!
//! Provides ultra-low-latency (<1ms), zero-model rule matching against task envelope
//! caller workspace, payload title, and target identifiers.

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::{DecisionEngine, DecisionRoutingOutcome};
use crate::envelope::TaskEnvelope;
use crate::error::RouterError;

fn default_confidence() -> f64 {
    1.0
}

fn default_action() -> String {
    "act".to_string()
}

/// Pattern matching strategy for string fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MatchPattern {
    /// Exact match against the text (case-insensitive fallback).
    Exact(String),
    /// Case-sensitive prefix match.
    Prefix(String),
    /// Regular expression pattern.
    Regex(String),
}

impl MatchPattern {
    /// Creates an exact match pattern.
    pub fn exact(pattern: impl Into<String>) -> Self {
        Self::Exact(pattern.into())
    }

    /// Creates a prefix match pattern.
    pub fn prefix(pattern: impl Into<String>) -> Self {
        Self::Prefix(pattern.into())
    }

    /// Creates a regular expression match pattern.
    pub fn regex(pattern: impl Into<String>) -> Self {
        Self::Regex(pattern.into())
    }

    /// Tests whether the given text satisfies this pattern.
    pub fn matches(&self, text: &str) -> bool {
        match self {
            Self::Exact(pat) => pat == text || pat.eq_ignore_ascii_case(text),
            Self::Prefix(pat) => text.starts_with(pat),
            Self::Regex(pat) => Regex::new(pat).map(|r| r.is_match(text)).unwrap_or(false),
        }
    }
}

/// A deterministic routing rule matching against envelope fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Target repository/service identifier to assign when matched.
    pub target: String,
    /// Optional match against caller workspace path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_workspace: Option<MatchPattern>,
    /// Optional match against task payload title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_title: Option<MatchPattern>,
    /// Optional match against incoming target identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_identifier: Option<MatchPattern>,
    /// Assigned confidence score (default 1.0).
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Gate action ("act" | "escalate", default "act").
    #[serde(default = "default_action")]
    pub action: String,
}

impl Rule {
    /// Creates a new rule targeting the given identifier.
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            match_workspace: None,
            match_title: None,
            match_identifier: None,
            confidence: 1.0,
            action: "act".to_string(),
        }
    }

    /// Adds a workspace match condition.
    pub fn with_match_workspace(mut self, pattern: MatchPattern) -> Self {
        self.match_workspace = Some(pattern);
        self
    }

    /// Adds a title match condition.
    pub fn with_match_title(mut self, pattern: MatchPattern) -> Self {
        self.match_title = Some(pattern);
        self
    }

    /// Adds a target identifier match condition.
    pub fn with_match_identifier(mut self, pattern: MatchPattern) -> Self {
        self.match_identifier = Some(pattern);
        self
    }

    /// Sets the rule confidence score.
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence;
        self
    }

    /// Sets the rule gate action ("act" | "escalate").
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = action.into();
        self
    }

    /// Evaluates whether this rule matches the given `TaskEnvelope`.
    pub fn matches(&self, envelope: &TaskEnvelope) -> bool {
        // Workspace pattern matching: checks caller.workspace or target.workspace
        if let Some(ref ws_pat) = self.match_workspace {
            let caller_matched = ws_pat.matches(&envelope.caller.workspace);
            let target_matched = envelope
                .target
                .workspace
                .as_deref()
                .map(|ws| ws_pat.matches(ws))
                .unwrap_or(false);
            if !caller_matched && !target_matched {
                return false;
            }
        }

        // Title pattern matching
        if let Some(ref title_pat) = self.match_title {
            if !title_pat.matches(&envelope.payload.title) {
                return false;
            }
        }

        // Identifier pattern matching
        if let Some(ref id_pat) = self.match_identifier {
            if !id_pat.matches(&envelope.target.identifier) {
                return false;
            }
        }

        true
    }
}

/// Deterministic Rule Engine evaluating rules sequentially.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeterministicRuleEngine {
    /// Ordered list of routing rules.
    pub rules: Vec<Rule>,
}

impl DeterministicRuleEngine {
    /// Creates an empty rule engine.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Creates a rule engine with an initial list of rules.
    pub fn with_rules(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Appends a rule to the engine.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Parses rules from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self, RouterError> {
        serde_json::from_str(json).map_err(RouterError::from)
    }

    /// Loads rules from a JSON file on disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, RouterError> {
        let content = fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }
}

#[async_trait]
impl DecisionEngine for DeterministicRuleEngine {
    fn name(&self) -> &'static str {
        "rule"
    }

    async fn is_healthy(&self) -> bool {
        true
    }

    async fn evaluate_task(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        for rule in &self.rules {
            if rule.matches(envelope) {
                let should_stage_outbox = rule.action == "escalate" || rule.confidence < 0.85;
                return Ok(DecisionRoutingOutcome {
                    target_identifier: rule.target.clone(),
                    confidence: rule.confidence,
                    engine_source: "rule",
                    action_gate: rule.action.clone(),
                    should_stage_outbox,
                });
            }
        }

        // Fallback when no rule matches: escalate to outbox
        let fallback_target = candidates.first().map(|(id, _)| *id).unwrap_or("");
        Ok(DecisionRoutingOutcome::escalate(
            fallback_target,
            0.0,
            "rule",
        ))
    }
}

/// Default pass-through decision engine with zero runtime overhead.
#[derive(Debug, Clone, Default)]
pub struct NoopDecisionEngine;

#[async_trait]
impl DecisionEngine for NoopDecisionEngine {
    fn name(&self) -> &'static str {
        "noop"
    }

    async fn is_healthy(&self) -> bool {
        true
    }

    async fn evaluate_task(
        &self,
        _envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        let fallback_target = candidates.first().map(|(id, _)| *id).unwrap_or("");
        Ok(DecisionRoutingOutcome::escalate(
            fallback_target,
            0.0,
            "noop",
        ))
    }
}
