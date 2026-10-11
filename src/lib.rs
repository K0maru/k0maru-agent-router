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
pub mod outbox;
pub mod policy;

pub use addressing::{normalize_workspace_path, resolve_session, SessionResolution};
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
pub use outbox::{HitlOutbox, StagedIssueStatus, StagedIssueSummary, StagedStatus};
pub use policy::{BoundaryPolicy, PolicyVerdict};
