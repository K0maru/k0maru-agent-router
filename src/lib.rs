//! k0maru-agent-router core library
//!
//! Autonomous, decoupled cross-repository agent routing microkernel.

pub mod addressing;
pub mod coordinator;
pub mod decision;
pub mod driver;
pub mod envelope;
pub mod error;
pub mod mcp;
pub mod memory;
pub mod orchestrator;
pub mod outbox;
pub mod policy;
pub mod receipt;
pub mod signal;
pub mod workspace;

pub use addressing::{
    is_same_workspace, normalize_workspace_path, resolve_session, SessionResolution,
};
pub use coordinator::{
    format_agent_prompt, DriverStatusSummary, OutboxStatusSummary, RouteOutcome, RouterCoordinator,
    SystemStatusSummary,
};
pub use decision::{
    CascadingDecisionEngine, ChoiceCandidate, DecisionEngine, DecisionRoutingOutcome,
    DeterministicRuleEngine, LayaDecisionClient, MatchPattern, NoopDecisionEngine, Rule,
    SystemOneRequest, SystemOneResponse,
};
pub use driver::{
    classify_command_status, parse_pane_line, parse_tmux_output, LiveSession, MultiplexerDriver,
    SessionStatus, TmuxDriver,
};
pub use envelope::{
    CallerInfo, ReproductionPayload, TargetInfo, TaskEnvelope, TaskPayload, DEFAULT_SCHEMA,
    VALID_SEVERITIES,
};
pub use error::RouterError;
pub use mcp::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpServer, McpToolCallResult, McpToolContent,
};
pub use memory::{MemoryHook, NoopMemoryHook, OptionalMemoryHook};
pub use orchestrator::{
    collect_orchestrator_status, discover_default_workspaces, format_ascii_table, probe_git_status,
    probe_workspace_status, OrchestratorReport, WorkspaceGitStatus, WorkspaceReceiptStatus,
    WorkspaceSignalStatus, WorkspaceStatus,
};
pub use outbox::{HitlOutbox, StagedIssueStatus, StagedIssueSummary, StagedStatus};
pub use policy::{BoundaryPolicy, PolicyVerdict};
pub use receipt::{
    resolve_receipts_dir, CompilationCheck, ExecutionChecks, ExecutionReceipt, LinterCheck,
    ReceiptArtifacts, ReceiptStore, ReceiptSummary, ReceiptVerdict, ScopeGuardCheck, TestCheck,
    DEFAULT_K0MARU_DIR, DEFAULT_RECEIPTS_DIR, DEFAULT_RECEIPT_SCHEMA, ENV_K0MARU_RECEIPT_DIR,
};
pub use signal::{
    resolve_agent_pane, resolve_signals_dir, SignalStatus, SignalStore, TaskSignal,
    DEFAULT_SIGNALS_DIR, DEFAULT_SIGNAL_SCHEMA, ENV_K0MARU_SIGNAL_DIR,
};
pub use workspace::{
    CliWorkspaceProvider, InPlaceWorkspaceProvider, OptionalWorkspaceProvider, WorkspaceProvider,
};
