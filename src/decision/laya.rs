//! Laya & Jev Non-Autoregressive Decision Client.
//!
//! Consumes the `POST /v1/systemone` choice primitive to resolve ambiguous task targets
//! via single forward-pass ModernBERT-large inference with mathematically calibrated probabilities.

use async_trait::async_trait;
use std::time::Duration;

use super::protocol::{SystemOneRequest, SystemOneResponse};
use super::{DecisionEngine, DecisionRoutingOutcome};
use crate::envelope::TaskEnvelope;
use crate::error::RouterError;

const DEFAULT_LAYA_URL: &str = "http://127.0.0.1:8000";
const DEFAULT_TIMEOUT_MS: u64 = 2000;
const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.85;

/// HTTP client interacting with local Laya or cloud Jev System 1 models.
#[derive(Debug, Clone)]
pub struct LayaDecisionClient {
    base_url: String,
    timeout: Duration,
    confidence_threshold: f64,
    engine_source: &'static str,
    client: reqwest::Client,
}

impl Default for LayaDecisionClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LayaDecisionClient {
    /// Creates a new `LayaDecisionClient` with default configuration (`http://127.0.0.1:8000`).
    pub fn new() -> Self {
        let timeout = Duration::from_millis(DEFAULT_TIMEOUT_MS);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();

        Self {
            base_url: DEFAULT_LAYA_URL.to_string(),
            timeout,
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
            engine_source: "laya",
            client,
        }
    }

    /// Creates a client configured for cloud Jev fallback.
    pub fn new_jev(base_url: impl Into<String>) -> Self {
        Self::new()
            .with_base_url(base_url)
            .with_engine_source("jev")
    }

    /// Configures the server base URL.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Configures request timeout duration.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        if let Ok(c) = reqwest::Client::builder().timeout(timeout).build() {
            self.client = c;
        }
        self
    }

    /// Configures the minimum confidence threshold required for autonomous action (default 0.85).
    pub fn with_confidence_threshold(mut self, threshold: f64) -> Self {
        self.confidence_threshold = threshold;
        self
    }

    /// Configures the engine source attribution name.
    pub fn with_engine_source(mut self, source: &'static str) -> Self {
        self.engine_source = source;
        self
    }

    /// Returns the configured base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the configured confidence threshold.
    pub fn confidence_threshold(&self) -> f64 {
        self.confidence_threshold
    }
}

#[async_trait]
impl DecisionEngine for LayaDecisionClient {
    fn name(&self) -> &'static str {
        self.engine_source
    }

    async fn is_healthy(&self) -> bool {
        let probe_url = format!("{}/health", self.base_url);
        match self
            .client
            .get(&probe_url)
            .timeout(Duration::from_millis(500))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => {
                // Secondary probe fallback to /v1/health
                let alt_url = format!("{}/v1/health", self.base_url);
                match self
                    .client
                    .get(&alt_url)
                    .timeout(Duration::from_millis(500))
                    .send()
                    .await
                {
                    Ok(alt_resp) => alt_resp.status().is_success(),
                    Err(_) => false,
                }
            }
        }
    }

    async fn evaluate_task(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        if candidates.is_empty() {
            return Ok(DecisionRoutingOutcome::escalate(
                "",
                0.0,
                self.engine_source,
            ));
        }

        let request_payload = SystemOneRequest::from_envelope(envelope, candidates);
        let endpoint = format!("{}/v1/systemone", self.base_url);

        let response = self
            .client
            .post(&endpoint)
            .json(&request_payload)
            .send()
            .await
            .map_err(|e| {
                RouterError::DriverExecutionFailed(format!(
                    "{} decision client request failed: {e}",
                    self.engine_source
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RouterError::DriverExecutionFailed(format!(
                "{} server returned error status {status}: {body}",
                self.engine_source
            )));
        }

        let sys_one_resp: SystemOneResponse = response.json().await.map_err(|e| {
            RouterError::DriverExecutionFailed(format!(
                "Failed to deserialize {} response: {e}",
                self.engine_source
            ))
        })?;

        let should_stage_outbox = sys_one_resp.confidence < self.confidence_threshold
            || sys_one_resp.action == "escalate";

        let action_gate = if should_stage_outbox {
            "escalate".to_string()
        } else {
            sys_one_resp.action
        };

        Ok(DecisionRoutingOutcome {
            target_identifier: sys_one_resp.selected_id,
            confidence: sys_one_resp.confidence,
            engine_source: self.engine_source,
            action_gate,
            should_stage_outbox,
        })
    }
}
