//! Router Coordinator Engine for k0maru-agent-router.
//!
//! Orchestrates the full task lifecycle: contract validation, boundary policy
//! evaluation, HITL Outbox quarantine, deterministic physical addressing,
//! prompt formatting, and multiplexer driver dispatch.

use crate::addressing::{normalize_workspace_path, resolve_session, SessionResolution};
use crate::decision::{DecisionEngine, DecisionRoutingOutcome, NoopDecisionEngine};
use crate::driver::{LiveSession, MultiplexerDriver};
use crate::envelope::TaskEnvelope;
use crate::error::RouterError;
use crate::memory::{MemoryHook, NoopMemoryHook};
use crate::outbox::{HitlOutbox, StagedIssueSummary};
use crate::policy::{BoundaryPolicy, PolicyVerdict};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Terminal outcome of routing a `TaskEnvelope` through the coordinator pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOutcome {
    /// Internal task dispatched to an active or newly spawned multiplexer session.
    Dispatched {
        /// Target live session where the prompt was submitted.
        session: LiveSession,
        /// Verbatim prompt string delivered to the pane.
        prompt: String,
    },
    /// Untrusted external task quarantined in the HITL Outbox.
    Staged {
        /// File path to the generated MRE markdown draft (`issue_<id>.md`).
        issue_path: PathBuf,
        /// Explanatory justification for quarantine.
        reason: String,
    },
    /// Non-mutating simulation evaluating boundary policy and physical addressing.
    DryRun {
        /// Policy boundary verdict evaluated for this task.
        verdict: PolicyVerdict,
        /// Physical session resolution if trusted internal target, or `None` if external.
        resolution: Option<SessionResolution>,
    },
}

impl RouteOutcome {
    /// Returns `true` if the task was dispatched to a live session.
    pub fn is_dispatched(&self) -> bool {
        matches!(self, Self::Dispatched { .. })
    }

    /// Returns `true` if the task was quarantined in the HITL Outbox.
    pub fn is_staged(&self) -> bool {
        matches!(self, Self::Staged { .. })
    }

    /// Returns `true` if this outcome was generated under dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        matches!(self, Self::DryRun { .. })
    }

    /// Returns the target `LiveSession` if dispatched.
    pub fn session(&self) -> Option<&LiveSession> {
        match self {
            Self::Dispatched { session, .. } => Some(session),
            Self::DryRun {
                resolution: Some(SessionResolution::HotSession(session)),
                ..
            } => Some(session),
            _ => None,
        }
    }

    /// Returns the delivered prompt string if dispatched.
    pub fn prompt(&self) -> Option<&str> {
        match self {
            Self::Dispatched { prompt, .. } => Some(prompt.as_str()),
            _ => None,
        }
    }

    /// Returns the staged draft path if quarantined.
    pub fn issue_path(&self) -> Option<&Path> {
        match self {
            Self::Staged { issue_path, .. } => Some(issue_path.as_path()),
            _ => None,
        }
    }

    /// Returns the quarantine reason string if staged.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Staged { reason, .. } => Some(reason.as_str()),
            _ => None,
        }
    }
}

/// Operational summary of multiplexer driver status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverStatusSummary {
    /// Driver identifier name (e.g. "tmux").
    pub name: String,
    /// Whether the driver executable is available on the system.
    pub available: bool,
    /// Number of active sessions or panes currently detected.
    pub sessions_count: usize,
}

/// Operational summary of HITL Outbox quarantine storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxStatusSummary {
    /// Directory path configured for staged drafts.
    pub directory: PathBuf,
    /// Total count of staged issue drafts found.
    pub total: usize,
    /// Count of drafts currently pending human authorization.
    pub pending: usize,
    /// Count of drafts approved by human engineer.
    pub approved: usize,
    /// Count of drafts rejected by human engineer.
    pub rejected: usize,
}

/// Overall system health and operational status summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemStatusSummary {
    /// Router package version.
    pub version: String,
    /// Status of the active multiplexer driver.
    pub driver: DriverStatusSummary,
    /// Status of the HITL Outbox holding area.
    pub outbox: OutboxStatusSummary,
}

impl SystemStatusSummary {
    /// Returns the configured driver identifier name.
    pub fn driver_name(&self) -> &str {
        &self.driver.name
    }

    /// Returns `true` if the driver is available.
    pub fn driver_available(&self) -> bool {
        self.driver.available
    }

    /// Returns the number of live sessions currently detected.
    pub fn sessions_count(&self) -> usize {
        self.driver.sessions_count
    }

    /// Returns the total number of staged issue drafts.
    pub fn outbox_total(&self) -> usize {
        self.outbox.total
    }

    /// Returns the count of pending staged issue drafts.
    pub fn outbox_pending(&self) -> usize {
        self.outbox.pending
    }

    /// Returns the count of approved staged issue drafts.
    pub fn outbox_approved(&self) -> usize {
        self.outbox.approved
    }

    /// Returns the count of rejected staged issue drafts.
    pub fn outbox_rejected(&self) -> usize {
        self.outbox.rejected
    }
}

/// Formats an actionable, structured Markdown agent prompt from a `TaskEnvelope`.
///
/// Includes task identity, intent, severity, title, caller provenance,
/// expected behavior, and reproduction instructions in fenced Markdown blocks.
pub fn format_agent_prompt(envelope: &TaskEnvelope) -> String {
    format_agent_prompt_with_context(envelope, None)
}

/// Formats an actionable Markdown agent prompt enriched with optional architectural context.
pub fn format_agent_prompt_with_context(
    envelope: &TaskEnvelope,
    architectural_context: Option<&str>,
) -> String {
    let mut prompt = format!(
        "[K0maru Task Dispatch] {} (Severity: {}, Intent: {})\nTitle: {}\n",
        envelope.id, envelope.payload.severity, envelope.intent, envelope.payload.title
    );

    if let Some(ref pane_id) = envelope.caller.pane_id {
        prompt.push_str(&format!(
            "Caller: {} (pane: {}) in {}\n",
            envelope.caller.agent, pane_id, envelope.caller.workspace
        ));
    } else {
        prompt.push_str(&format!(
            "Caller: {} in {}\n",
            envelope.caller.agent, envelope.caller.workspace
        ));
    }

    if let Some(ctx) = architectural_context {
        if !ctx.trim().is_empty() {
            prompt.push_str(&format!(
                "\nArchitectural Context & Blueprint:\n{}\n",
                ctx.trim()
            ));
        }
    }

    if let Some(ref exp) = envelope.payload.expected {
        prompt.push_str(&format!("\nExpected Behavior:\n{}\n", exp));
    }

    if let Some(ref repro) = envelope.payload.reproduction {
        prompt.push_str("\nReproduction Details:\n");
        if let Some(obj) = repro.as_object() {
            let mut formatted_structured = false;
            if let Some(snippet) = obj.get("snippet").and_then(|s| s.as_str()) {
                prompt.push_str(&format!("Snippet:\n```\n{}\n```\n", snippet));
                formatted_structured = true;
            }
            if let Some(error) = obj.get("error").and_then(|e| e.as_str()) {
                prompt.push_str(&format!("Error:\n```\n{}\n```\n", error));
                formatted_structured = true;
            }
            if !formatted_structured {
                if let Ok(pretty) = serde_json::to_string_pretty(repro) {
                    prompt.push_str(&format!("```json\n{}\n```\n", pretty));
                }
            }
        } else if let Ok(pretty) = serde_json::to_string_pretty(repro) {
            prompt.push_str(&format!("```json\n{}\n```\n", pretty));
        }
    }

    prompt.push_str("\nPlease investigate and resolve this issue in the current workspace.");
    prompt
}

/// High-level Router Coordinator orchestrating protocol contracts, boundary policies,
/// HITL Outbox quarantine, physical addressing, decision engines, and multiplexer driver prompt delivery.
#[derive(Debug, Clone)]
pub struct RouterCoordinator<D: MultiplexerDriver> {
    driver: D,
    outbox_dir: PathBuf,
    decision_engine: Arc<dyn DecisionEngine>,
    memory_hook: Arc<dyn MemoryHook>,
}

impl<D: MultiplexerDriver> RouterCoordinator<D> {
    /// Creates a new `RouterCoordinator` with the specified multiplexer driver.
    /// Defaults `outbox_dir` to `.scratch/staged`, decision engine to `NoopDecisionEngine`,
    /// and memory hook to `NoopMemoryHook`.
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            outbox_dir: PathBuf::from(".scratch/staged"),
            decision_engine: Arc::new(NoopDecisionEngine),
            memory_hook: Arc::new(NoopMemoryHook),
        }
    }

    /// Configures a custom HITL outbox holding area directory.
    pub fn with_outbox_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.outbox_dir = path.into();
        self
    }

    /// Configures a custom decision engine for ambiguous target resolution.
    pub fn with_decision_engine(mut self, engine: impl DecisionEngine + 'static) -> Self {
        self.decision_engine = Arc::new(engine);
        self
    }

    /// Configures a shared decision engine via Arc.
    pub fn with_decision_engine_arc(mut self, engine: Arc<dyn DecisionEngine>) -> Self {
        self.decision_engine = engine;
        self
    }

    /// Configures an optional cognitive memory hook.
    pub fn with_memory_hook(mut self, hook: impl MemoryHook + 'static) -> Self {
        self.memory_hook = Arc::new(hook);
        self
    }

    /// Configures a shared memory hook via Arc.
    pub fn with_memory_hook_arc(mut self, hook: Arc<dyn MemoryHook>) -> Self {
        self.memory_hook = hook;
        self
    }

    /// Returns a reference to the configured outbox directory.
    pub fn outbox_dir(&self) -> &Path {
        &self.outbox_dir
    }

    /// Returns a reference to the underlying multiplexer driver.
    pub fn driver(&self) -> &D {
        &self.driver
    }

    /// Returns a reference to the active decision engine.
    pub fn decision_engine(&self) -> &dyn DecisionEngine {
        self.decision_engine.as_ref()
    }

    /// Returns a reference to the active memory hook.
    pub fn memory_hook(&self) -> &dyn MemoryHook {
        self.memory_hook.as_ref()
    }

    /// Synchronously evaluates a `TaskEnvelope` against candidate targets using the configured decision engine.
    pub fn evaluate_candidates(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
    ) -> Result<DecisionRoutingOutcome, RouterError> {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| {
                handle.block_on(self.decision_engine.evaluate_task(envelope, candidates))
            })
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    RouterError::DriverExecutionFailed(format!(
                        "Failed to initialize runtime for decision engine: {e}"
                    ))
                })?;
            rt.block_on(self.decision_engine.evaluate_task(envelope, candidates))
        }
    }

    /// Ingests, validates, and routes a `TaskEnvelope` with optional candidate targets for ambiguous tasks.
    pub fn route_envelope_with_candidates(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)],
        dry_run: bool,
    ) -> Result<RouteOutcome, RouterError> {
        let is_ambiguous = envelope.target.identifier == "auto"
            || envelope.target.identifier == "unspecified"
            || !candidates.is_empty();

        let target_envelope =
            if is_ambiguous && (!candidates.is_empty() || self.decision_engine.name() != "noop") {
                let outcome = self.evaluate_candidates(envelope, candidates)?;
                if outcome.should_stage_outbox || outcome.action_gate == "escalate" {
                    if dry_run {
                        return Ok(RouteOutcome::DryRun {
                            verdict: PolicyVerdict::UntrustedExternal {
                                reason: format!(
                                    "Gated by decision engine ({}, confidence {:.2})",
                                    outcome.engine_source, outcome.confidence
                                ),
                            },
                            resolution: None,
                        });
                    } else {
                        let issue_path = HitlOutbox::stage(envelope, &self.outbox_dir)?;
                        return Ok(RouteOutcome::Staged {
                            issue_path,
                            reason: format!(
                                "Escalated by decision engine ({}, confidence {:.2})",
                                outcome.engine_source, outcome.confidence
                            ),
                        });
                    }
                }

                let mut resolved = envelope.clone();
                resolved.target.identifier = outcome.target_identifier;
                resolved
            } else {
                envelope.clone()
            };

        // Step 1: Validate contract invariants
        target_envelope.validate()?;

        // Step 2: Evaluate boundary policy
        let verdict = BoundaryPolicy::evaluate(&target_envelope);

        // Step 3: Handle untrusted external target
        if let PolicyVerdict::UntrustedExternal { reason } = verdict {
            if dry_run {
                return Ok(RouteOutcome::DryRun {
                    verdict: PolicyVerdict::UntrustedExternal { reason },
                    resolution: None,
                });
            } else {
                let issue_path = HitlOutbox::stage(&target_envelope, &self.outbox_dir)?;
                return Ok(RouteOutcome::Staged { issue_path, reason });
            }
        }

        // Step 4: Handle trusted internal target
        let raw_ws = match &target_envelope.target.workspace {
            Some(ws) if !ws.trim().is_empty() => PathBuf::from(ws),
            _ => {
                return Err(RouterError::InvalidEnvelope(
                    "Internal target requires a non-empty 'target.workspace' path".to_string(),
                ));
            }
        };

        // Fail fast if workspace directory does not exist or is invalid
        let canonical_ws = normalize_workspace_path(&raw_ws)?;

        // Query live multiplexer sessions and resolve physical address
        let live_sessions = self.driver.list_sessions()?;
        let resolution = resolve_session(&canonical_ws, &live_sessions)?;

        if dry_run {
            return Ok(RouteOutcome::DryRun {
                verdict: PolicyVerdict::TrustedInternal,
                resolution: Some(resolution),
            });
        }

        let memory_context = self
            .memory_hook
            .preload_context(&canonical_ws, &target_envelope.target.identifier)
            .unwrap_or(None);
        let prompt = format_agent_prompt_with_context(&target_envelope, memory_context.as_deref());

        match resolution {
            SessionResolution::HotSession(session) => {
                self.driver.send_prompt(&session.pane_id, &prompt)?;
                Ok(RouteOutcome::Dispatched { session, prompt })
            }
            SessionResolution::ColdSpawn { canonical_path } => {
                let session = self.driver.spawn_pane(&canonical_path, None)?;
                self.driver.send_prompt(&session.pane_id, &prompt)?;
                Ok(RouteOutcome::Dispatched { session, prompt })
            }
        }
    }

    /// Ingests, validates, and routes a `TaskEnvelope`.
    ///
    /// # Pipeline Execution
    /// 1. Validates envelope contract invariants (`envelope.validate()?`).
    /// 2. Evaluates trust boundary policy (`BoundaryPolicy::evaluate(envelope)`).
    /// 3. If `PolicyVerdict::UntrustedExternal`:
    ///    - If `dry_run`: returns `Ok(RouteOutcome::DryRun { verdict, resolution: None })`.
    ///    - Else: stages issue draft via `HitlOutbox::stage` and returns `Ok(RouteOutcome::Staged)`.
    /// 4. If `PolicyVerdict::TrustedInternal`:
    ///    - Extracts target workspace path from `envelope.target.workspace` (or fallback).
    ///    - Fails fast with `RouterError::WorkspaceNotFound` if the directory does not exist.
    ///    - Queries live sessions and resolves session via `resolve_session`.
    ///    - If `dry_run`: returns `Ok(RouteOutcome::DryRun { verdict, resolution: Some(resolution) })`.
    ///    - Preloads architectural blueprint context via memory hook.
    ///    - Formats actionable prompt via `format_agent_prompt_with_context`.
    ///    - If `HotSession(session)`: dispatches prompt to `session.pane_id`.
    ///    - If `ColdSpawn { canonical_path }`: spawns pane in workspace, then dispatches prompt.
    ///    - Returns `Ok(RouteOutcome::Dispatched)`.
    pub fn route_envelope(
        &self,
        envelope: &TaskEnvelope,
        dry_run: bool,
    ) -> Result<RouteOutcome, RouterError> {
        self.route_envelope_with_candidates(envelope, &[], dry_run)
    }

    /// Probes active multiplexer sessions matching the specified workspace directory.
    ///
    /// Fails fast with `RouterError::WorkspaceNotFound` if the directory does not exist on disk.
    pub fn probe_workspace(&self, workspace: &Path) -> Result<SessionResolution, RouterError> {
        let canonical = normalize_workspace_path(workspace)?;
        let live_sessions = self.driver.list_sessions()?;
        resolve_session(&canonical, &live_sessions)
    }

    /// Inspects global router health, driver availability, live sessions count, and outbox metrics.
    ///
    /// Resilient: gracefully handles missing directories or offline multiplexer servers.
    pub fn system_status(&self) -> Result<SystemStatusSummary, RouterError> {
        let driver_name = self.driver.name().to_string();
        let driver_available = self.driver.is_available();

        let live_sessions_count = if driver_available {
            match self.driver.list_sessions() {
                Ok(sessions) => sessions.len(),
                Err(_) => 0,
            }
        } else {
            0
        };

        let staged_issues = HitlOutbox::list_staged(&self.outbox_dir).unwrap_or_default();
        let pending = staged_issues
            .iter()
            .filter(|s| s.status.is_pending())
            .count();
        let approved = staged_issues
            .iter()
            .filter(|s| s.status.is_approved())
            .count();
        let rejected = staged_issues
            .iter()
            .filter(|s| s.status.is_rejected())
            .count();

        Ok(SystemStatusSummary {
            version: env!("CARGO_PKG_VERSION").to_string(),
            driver: DriverStatusSummary {
                name: driver_name,
                available: driver_available,
                sessions_count: live_sessions_count,
            },
            outbox: OutboxStatusSummary {
                directory: self.outbox_dir.clone(),
                total: staged_issues.len(),
                pending,
                approved,
                rejected,
            },
        })
    }

    /// Lists summary metadata for all staged issue drafts in the configured outbox directory.
    pub fn list_staged(&self) -> Result<Vec<StagedIssueSummary>, RouterError> {
        HitlOutbox::list_staged(&self.outbox_dir)
    }

    /// Reads and returns the complete markdown content of a staged issue draft.
    pub fn inspect_staged(&self, id: &str) -> Result<String, RouterError> {
        HitlOutbox::inspect_staged(id, &self.outbox_dir)
    }

    /// Authorizes a staged issue draft for outbound transmission.
    pub fn approve_staged(&self, id: &str) -> Result<PathBuf, RouterError> {
        HitlOutbox::approve(id, &self.outbox_dir)
    }

    /// Explicitly rejects a staged issue draft.
    pub fn reject_staged(&self, id: &str) -> Result<(), RouterError> {
        HitlOutbox::reject(id, &self.outbox_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::SessionStatus;
    use crate::envelope::{CallerInfo, TargetInfo, TaskPayload};
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// Mock multiplexer driver for deterministic unit testing without spawning OS processes.
    #[derive(Debug, Default)]
    struct MockDriver {
        pub sessions: Mutex<Vec<LiveSession>>,
        pub sent_prompts: Mutex<Vec<(String, String)>>,
        pub spawned_panes: Mutex<Vec<(PathBuf, Option<String>)>>,
        pub available: bool,
    }

    impl MockDriver {
        fn new_available() -> Self {
            Self {
                sessions: Mutex::new(Vec::new()),
                sent_prompts: Mutex::new(Vec::new()),
                spawned_panes: Mutex::new(Vec::new()),
                available: true,
            }
        }

        fn with_session(self, session: LiveSession) -> Self {
            self.sessions.lock().unwrap().push(session);
            self
        }
    }

    impl MultiplexerDriver for MockDriver {
        fn name(&self) -> &'static str {
            "mock-mux"
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError> {
            if !self.available {
                return Err(RouterError::DriverUnavailable {
                    driver: "mock-mux",
                    reason: "mock unavailable".to_string(),
                });
            }
            Ok(self.sessions.lock().unwrap().clone())
        }

        fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError> {
            self.sent_prompts
                .lock()
                .unwrap()
                .push((pane_id.to_string(), prompt.to_string()));
            Ok(())
        }

        fn spawn_pane(
            &self,
            workspace: &Path,
            command: Option<&str>,
        ) -> Result<LiveSession, RouterError> {
            self.spawned_panes
                .lock()
                .unwrap()
                .push((workspace.to_path_buf(), command.map(String::from)));
            let new_session = LiveSession::new(
                "%99",
                workspace.to_path_buf(),
                SessionStatus::Idle,
                Some("bash".into()),
            );
            self.sessions.lock().unwrap().push(new_session.clone());
            Ok(new_session)
        }
    }

    fn sample_envelope(is_external: bool, workspace: Option<&str>) -> TaskEnvelope {
        TaskEnvelope {
            schema: None,
            id: "task_coord_1".to_string(),
            created_at: "2026-10-11T12:00:00Z".to_string(),
            caller: CallerInfo {
                workspace: "/caller/ws".to_string(),
                agent: "caller_agent".to_string(),
                pane_id: Some("%1".to_string()),
            },
            target: TargetInfo {
                identifier: if is_external {
                    "react-dom".to_string()
                } else {
                    "internal-core".to_string()
                },
                workspace: workspace.map(String::from),
                is_external,
            },
            intent: "bug_report".to_string(),
            payload: TaskPayload {
                title: "Hydration Mismatch".to_string(),
                severity: "blocking".to_string(),
                reproduction: Some(serde_json::json!({
                    "snippet": "<App />",
                    "error": "Hydration failed"
                })),
                expected: Some("Clean hydration".to_string()),
            },
        }
    }

    #[test]
    fn test_coordinator_defaults_and_builder() {
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver);
        assert_eq!(coordinator.outbox_dir(), Path::new(".scratch/staged"));
        assert_eq!(coordinator.driver().name(), "mock-mux");

        let coordinator = coordinator.with_outbox_dir("/tmp/custom_staged");
        assert_eq!(coordinator.outbox_dir(), Path::new("/tmp/custom_staged"));
    }

    #[test]
    fn test_route_untrusted_external_stages_issue() {
        let temp = tempdir().unwrap();
        let outbox = temp.path().join("outbox");
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&outbox);

        let env = sample_envelope(true, None);
        let outcome = coordinator.route_envelope(&env, false).unwrap();

        assert!(outcome.is_staged());
        assert!(!outcome.is_dispatched());
        assert!(!outcome.is_dry_run());

        let issue_path = outcome.issue_path().expect("Must have issue path");
        assert!(issue_path.exists());
        assert!(outcome.reason().unwrap().contains("react-dom"));

        // Invariant: mock driver was never contacted
        assert!(coordinator.driver.sent_prompts.lock().unwrap().is_empty());
        assert!(coordinator.driver.spawned_panes.lock().unwrap().is_empty());
    }

    #[test]
    fn test_route_untrusted_external_dry_run_zero_filesystem_mutation() {
        let temp = tempdir().unwrap();
        let outbox = temp.path().join("outbox");
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&outbox);

        let env = sample_envelope(true, None);
        let outcome = coordinator.route_envelope(&env, true).unwrap();

        assert!(outcome.is_dry_run());
        assert!(!outcome.is_staged());
        assert!(!outbox.exists()); // Zero filesystem mutation

        match outcome {
            RouteOutcome::DryRun {
                verdict,
                resolution,
            } => {
                assert!(verdict.is_untrusted());
                assert!(resolution.is_none());
            }
            _ => panic!("Expected DryRun outcome"),
        }
    }

    #[test]
    fn test_route_trusted_internal_hot_session_dispatches_prompt() {
        let temp = tempdir().unwrap();
        let ws_canon = std::fs::canonicalize(temp.path()).unwrap();
        let live = LiveSession::new(
            "%5",
            ws_canon.clone(),
            SessionStatus::Idle,
            Some("zsh".into()),
        );

        let driver = MockDriver::new_available().with_session(live);
        let coordinator = RouterCoordinator::new(driver);

        let env = sample_envelope(false, Some(temp.path().to_str().unwrap()));
        let outcome = coordinator.route_envelope(&env, false).unwrap();

        assert!(outcome.is_dispatched());
        assert_eq!(outcome.session().unwrap().pane_id, "%5");

        let prompts = coordinator.driver.sent_prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].0, "%5");
        assert!(prompts[0].1.contains("Hydration Mismatch"));
        assert!(prompts[0].1.contains("Caller: caller_agent (pane: %1)"));

        // No new panes were spawned
        assert!(coordinator.driver.spawned_panes.lock().unwrap().is_empty());
    }

    #[test]
    fn test_route_trusted_internal_cold_spawn_dispatches_prompt() {
        let temp = tempdir().unwrap();
        let driver = MockDriver::new_available(); // No live sessions initially
        let coordinator = RouterCoordinator::new(driver);

        let env = sample_envelope(false, Some(temp.path().to_str().unwrap()));
        let outcome = coordinator.route_envelope(&env, false).unwrap();

        assert!(outcome.is_dispatched());
        assert_eq!(outcome.session().unwrap().pane_id, "%99");

        let spawned = coordinator.driver.spawned_panes.lock().unwrap();
        assert_eq!(spawned.len(), 1);

        let prompts = coordinator.driver.sent_prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].0, "%99");
    }

    #[test]
    fn test_route_trusted_internal_dry_run_zero_mutation() {
        let temp = tempdir().unwrap();
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver);

        let env = sample_envelope(false, Some(temp.path().to_str().unwrap()));
        let outcome = coordinator.route_envelope(&env, true).unwrap();

        assert!(outcome.is_dry_run());
        match outcome {
            RouteOutcome::DryRun {
                verdict,
                resolution,
            } => {
                assert!(verdict.is_trusted());
                assert!(resolution.is_some());
                assert!(resolution.unwrap().is_cold());
            }
            _ => panic!("Expected DryRun outcome"),
        }

        // Invariant: zero driver mutation
        assert!(coordinator.driver.sent_prompts.lock().unwrap().is_empty());
        assert!(coordinator.driver.spawned_panes.lock().unwrap().is_empty());
    }

    #[test]
    fn test_route_trusted_internal_missing_workspace_fails_fast() {
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver);

        let env = sample_envelope(false, Some("/nonexistent/directory/xyz_987"));
        let err = coordinator.route_envelope(&env, false).unwrap_err();

        assert!(matches!(err, RouterError::WorkspaceNotFound(_)));
    }

    #[test]
    fn test_probe_workspace_resolution() {
        let temp = tempdir().unwrap();
        let ws_canon = std::fs::canonicalize(temp.path()).unwrap();
        let live = LiveSession::new(
            "%3",
            ws_canon.clone(),
            SessionStatus::Idle,
            Some("bash".into()),
        );

        let driver = MockDriver::new_available().with_session(live);
        let coordinator = RouterCoordinator::new(driver);

        let res = coordinator.probe_workspace(temp.path()).unwrap();
        assert!(res.is_hot());
        assert_eq!(res.session().unwrap().pane_id, "%3");

        let missing = Path::new("/definitely/missing/dir_123");
        let err = coordinator.probe_workspace(missing).unwrap_err();
        assert!(matches!(err, RouterError::WorkspaceNotFound(_)));
    }

    #[test]
    fn test_system_status_reporting() {
        let temp = tempdir().unwrap();
        let outbox = temp.path().join("outbox");
        let driver = MockDriver::new_available();
        let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&outbox);

        // Initially empty
        let status = coordinator.system_status().unwrap();
        assert_eq!(status.driver_name(), "mock-mux");
        assert!(status.driver_available());
        assert_eq!(status.sessions_count(), 0);
        assert_eq!(status.outbox_total(), 0);
        assert_eq!(status.outbox_pending(), 0);

        // Stage an issue
        let env = sample_envelope(true, None);
        coordinator.route_envelope(&env, false).unwrap();

        let status2 = coordinator.system_status().unwrap();
        assert_eq!(status2.outbox_total(), 1);
        assert_eq!(status2.outbox_pending(), 1);
        assert_eq!(status2.outbox_approved(), 0);
        assert_eq!(status2.outbox_rejected(), 0);

        // Approve it
        coordinator.approve_staged("task_coord_1").unwrap();
        let status3 = coordinator.system_status().unwrap();
        assert_eq!(status3.outbox_total(), 1);
        assert_eq!(status3.outbox_pending(), 0);
        assert_eq!(status3.outbox_approved(), 1);
    }

    #[test]
    fn test_format_agent_prompt_structure() {
        let env = sample_envelope(false, Some("/ws"));
        let prompt = format_agent_prompt(&env);

        assert!(prompt.contains("[K0maru Task Dispatch] task_coord_1"));
        assert!(prompt.contains("Title: Hydration Mismatch"));
        assert!(prompt.contains("Severity: blocking"));
        assert!(prompt.contains("Caller: caller_agent (pane: %1) in /caller/ws"));
        assert!(prompt.contains("Expected Behavior:\nClean hydration"));
        assert!(prompt.contains("Snippet:\n```\n<App />\n```"));
        assert!(prompt.contains("Error:\n```\nHydration failed\n```"));
        assert!(
            prompt.contains("Please investigate and resolve this issue in the current workspace.")
        );
    }

    #[test]
    fn test_route_outcome_serde_roundtrip() {
        let staged = RouteOutcome::Staged {
            issue_path: PathBuf::from(".scratch/staged/issue_1.md"),
            reason: "Untrusted".to_string(),
        };
        let json = serde_json::to_string(&staged).unwrap();
        let deserialized: RouteOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(staged, deserialized);

        let dry_run = RouteOutcome::DryRun {
            verdict: PolicyVerdict::TrustedInternal,
            resolution: Some(SessionResolution::ColdSpawn {
                canonical_path: PathBuf::from("/test"),
            }),
        };
        let json2 = serde_json::to_string(&dry_run).unwrap();
        let deserialized2: RouteOutcome = serde_json::from_str(&json2).unwrap();
        assert_eq!(dry_run, deserialized2);
    }
}
