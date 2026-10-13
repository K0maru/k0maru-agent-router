//! Cleanroom CLI binary entrypoint for k0maru-agent-router.

use clap::{Args, Parser, Subcommand};
use k0maru_router::coordinator::{RouteOutcome, RouterCoordinator};
use k0maru_router::decision::{
    CascadingDecisionEngine, DeterministicRuleEngine, LayaDecisionClient,
};
use k0maru_router::driver::{AnyDriver, HerdrDriver, MultiplexerDriver, TmuxDriver};
use k0maru_router::envelope::TaskEnvelope;
use k0maru_router::error::RouterError;
use k0maru_router::kdsl::{
    envelope_to_packet, estimate_tokens, kdsl_contract_snippet, packet_to_envelope,
    packet_token_budget, parse_kdsl,
};
use k0maru_router::memory::NoopMemoryHook;
use k0maru_router::orchestrator::{discover_default_workspaces, format_ascii_table};
use k0maru_router::receipt::{resolve_receipts_dir, ExecutionReceipt};
use k0maru_router::signal::{
    resolve_agent_pane, resolve_signals_dir, SignalStatus, TaskSignal, DEFAULT_SIGNAL_SCHEMA,
};
use k0maru_router::tuple_space::{resolve_tuples_dir, Tuple, TuplePattern, TupleSpaceEngine};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "k0maru-router",
    version,
    about = "Cleanroom, single-static-binary Agent Router for K0maru-Harness",
    long_about = "Autonomous, decoupled cross-repository agent routing microkernel. Bridges independent coding agents across distinct workspaces, repositories, and multiplexer panes."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    fn is_json(&self) -> bool {
        match &self.command {
            Commands::Status(args) => args.json,
            Commands::Probe(args) => args.json,
            Commands::Route(args) => args.json,
            Commands::Outbox(cmd) => match &cmd.action {
                OutboxAction::List(args) => args.json,
                OutboxAction::Inspect(_) => false,
                OutboxAction::Approve(args) => args.json,
                OutboxAction::Reject(args) => args.json,
            },
            Commands::Receipt(cmd) => match &cmd.action {
                ReceiptAction::Ingest(args) => args.json,
                ReceiptAction::Status(args) => args.json,
                ReceiptAction::List(args) => args.json,
            },
            Commands::Signal(args) => args.json,
            Commands::Orchestrator(cmd) => match &cmd.action {
                Some(OrchestratorAction::Status(args)) => args.json,
                None => cmd.args.json,
            },
            Commands::Pipeline(cmd) => match &cmd.action {
                Some(PipelineAction::Check(args)) => args.json,
                Some(PipelineAction::Run(args)) => args.json,
                None => cmd.run_args.json,
            },
            Commands::Kdsl(cmd) => match &cmd.action {
                KdslAction::Encode(args) => args.json,
                KdslAction::Decode(args) => args.json,
                KdslAction::Contract(args) => args.json,
            },
            Commands::Tuple(cmd) => match &cmd.action {
                TupleAction::Out(args) => args.json,
                TupleAction::In(args) => args.json,
                TupleAction::Rd(args) => args.json,
                TupleAction::List(args) => args.json,
            },
            Commands::Worker(args) => args.json,
            Commands::Mcp(_) => false,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check system status, driver availability, and outbox counts
    Status(StatusArgs),

    /// Probe active multiplexer sessions matching a target workspace CWD
    Probe(ProbeArgs),

    /// Route or stage a task envelope JSON file
    Route(RouteArgs),

    /// Run as a JSON-RPC 2.0 stdio Model Context Protocol (MCP) server
    Mcp(McpArgs),

    /// Manage staged external issues in the HITL outbox holding area
    Outbox(OutboxCommand),

    /// Ingest, inspect, and verify agent execution receipts
    Receipt(ReceiptCommand),

    /// Emit task execution completion, failure, or in-progress signals
    Signal(SignalArgs),

    /// Inspect status of workspaces, git states, latest signals, and receipts
    Orchestrator(OrchestratorCommand),

    /// Execute decoupled end-to-end task pipelines across optional harness components
    #[command(alias = "e2e")]
    Pipeline(PipelineCommand),

    /// Encode, decode, and inspect machine-native K-DSL 0.1 wire packets
    Kdsl(KdslCommand),

    /// Linda Tuple Space primitives (out, in, rd, list) for associative agent communication
    Tuple(TupleCommand),

    /// Autonomous reactive worker loop consuming tasks from the Linda Tuple Space
    Worker(WorkerArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Multiplexer driver to inspect (supported: tmux, herdr, auto)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

    /// Optional remote node hostname or Tailscale alias (e.g. "company-linux")
    #[arg(long)]
    pub node: Option<String>,

    /// Remote multiplexer backend (supported: auto, tmux, herdr)
    #[arg(long, default_value = "auto")]
    pub remote_backend: String,

    /// Output status in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("ws_group").required(true).args(["workspace", "positional_workspace"])))]
pub struct ProbeArgs {
    /// Target workspace directory to probe (canonicalized before resolution)
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,

    /// Positional workspace directory fallback
    #[arg(index = 1)]
    pub positional_workspace: Option<PathBuf>,

    /// Multiplexer driver to use (supported: tmux, herdr, auto)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

    /// Optional remote node hostname or Tailscale alias (e.g. "company-linux")
    #[arg(long)]
    pub node: Option<String>,

    /// Remote multiplexer backend (supported: auto, tmux, herdr)
    #[arg(long, default_value = "auto")]
    pub remote_backend: String,

    /// Output probe outcome in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

impl ProbeArgs {
    pub fn resolved_workspace(&self) -> &Path {
        self.workspace
            .as_deref()
            .or(self.positional_workspace.as_deref())
            .unwrap_or_else(|| Path::new("."))
    }
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("file_group").required(true).args(["file", "positional_file"])))]
pub struct RouteArgs {
    /// Path to the TaskEnvelope JSON file
    #[arg(short, long)]
    pub file: Option<PathBuf>,

    /// Positional envelope file fallback
    #[arg(index = 1)]
    pub positional_file: Option<PathBuf>,

    /// Multiplexer driver to use (supported: tmux, herdr, auto)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

    /// Optional remote node hostname or Tailscale alias (e.g. "company-linux")
    #[arg(long)]
    pub node: Option<String>,

    /// Remote multiplexer backend (supported: auto, tmux, herdr)
    #[arg(long, default_value = "auto")]
    pub remote_backend: String,

    /// Path to the staged outbox directory for untrusted external tasks
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Optional path to a JSON rule file for deterministic decision routing
    #[arg(long)]
    pub rule_file: Option<PathBuf>,

    /// Optional path to a JSON file containing candidate targets for decision routing
    #[arg(long)]
    pub candidates_file: Option<PathBuf>,

    /// Optional Laya / ModernBERT decision server base URL (e.g. http://127.0.0.1:8000)
    #[arg(long)]
    pub laya_url: Option<String>,

    /// Disable cognitive memory hook preloading
    #[arg(long)]
    pub no_memory: bool,

    /// Force task execution inside an isolated workspace sandbox
    #[arg(long)]
    pub isolate: bool,

    /// Dry-run mode: verify policy and addressing without mutating multiplexer state or creating outbox drafts
    #[arg(long)]
    pub dry_run: bool,

    /// Output route outcome in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

impl RouteArgs {
    pub fn resolved_file(&self) -> &Path {
        self.file
            .as_deref()
            .or(self.positional_file.as_deref())
            .unwrap_or_else(|| Path::new(""))
    }
}

#[derive(Args, Debug, Clone)]
pub struct McpArgs {
    /// Multiplexer driver to use (supported: tmux)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Optional path to a JSON rule file for deterministic decision routing
    #[arg(long)]
    pub rule_file: Option<PathBuf>,

    /// Optional Laya / ModernBERT decision server base URL (e.g. http://127.0.0.1:8000)
    #[arg(long)]
    pub laya_url: Option<String>,

    /// Disable cognitive memory hook preloading
    #[arg(long)]
    pub no_memory: bool,
}

#[derive(Args, Debug, Clone)]
pub struct OutboxCommand {
    #[command(subcommand)]
    pub action: OutboxAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum OutboxAction {
    /// List staged issue drafts in the outbox
    List(OutboxListArgs),

    /// Inspect the full markdown content of a staged issue draft
    Inspect(OutboxInspectArgs),

    /// Approve a staged issue draft for outbound dispatch
    Approve(OutboxApproveArgs),

    /// Reject a staged issue draft
    Reject(OutboxRejectArgs),
}

#[derive(Args, Debug, Clone)]
pub struct OutboxListArgs {
    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Output listing in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("inspect_id_group").required(true).args(["id", "positional_id"])))]
pub struct OutboxInspectArgs {
    /// Issue identifier (e.g. task_9f82a1b4)
    #[arg(short, long)]
    pub id: Option<String>,

    /// Positional issue identifier fallback
    #[arg(index = 1)]
    pub positional_id: Option<String>,

    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,
}

impl OutboxInspectArgs {
    pub fn resolved_id(&self) -> &str {
        self.id
            .as_deref()
            .or(self.positional_id.as_deref())
            .unwrap_or("")
    }
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("approve_id_group").required(true).args(["id", "positional_id"])))]
pub struct OutboxApproveArgs {
    /// Issue identifier to approve (e.g. task_9f82a1b4)
    #[arg(short, long)]
    pub id: Option<String>,

    /// Positional issue identifier fallback
    #[arg(index = 1)]
    pub positional_id: Option<String>,

    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Output approval outcome in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

impl OutboxApproveArgs {
    pub fn resolved_id(&self) -> &str {
        self.id
            .as_deref()
            .or(self.positional_id.as_deref())
            .unwrap_or("")
    }
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("reject_id_group").required(true).args(["id", "positional_id"])))]
pub struct OutboxRejectArgs {
    /// Issue identifier to reject (e.g. task_9f82a1b4)
    #[arg(short, long)]
    pub id: Option<String>,

    /// Positional issue identifier fallback
    #[arg(index = 1)]
    pub positional_id: Option<String>,

    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Output rejection outcome in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

impl OutboxRejectArgs {
    pub fn resolved_id(&self) -> &str {
        self.id
            .as_deref()
            .or(self.positional_id.as_deref())
            .unwrap_or("")
    }
}

#[derive(Args, Debug, Clone)]
pub struct ReceiptCommand {
    #[command(subcommand)]
    pub action: ReceiptAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ReceiptAction {
    /// Ingest and validate an execution receipt JSON file
    Ingest(ReceiptIngestArgs),

    /// Query and inspect a stored execution receipt
    Status(ReceiptStatusArgs),

    /// List all stored execution receipts
    List(ReceiptListArgs),
}

#[derive(Args, Debug, Clone)]
#[command(group(clap::ArgGroup::new("receipt_file_group").required(true).args(["file", "positional_file"])))]
pub struct ReceiptIngestArgs {
    /// Path to execution receipt JSON file
    #[arg(short, long)]
    pub file: Option<PathBuf>,

    /// Positional receipt file fallback
    #[arg(index = 1)]
    pub positional_file: Option<PathBuf>,

    /// Path to optional patch file for SHA-256 integrity verification
    #[arg(long)]
    pub patch: Option<PathBuf>,

    /// Path to receipts storage directory (defaults to $K0MARU_RECEIPT_DIR or ~/.k0maru/receipts)
    #[arg(long)]
    pub receipt_dir: Option<PathBuf>,

    /// Output result in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

impl ReceiptIngestArgs {
    pub fn resolved_file(&self) -> &Path {
        self.file
            .as_deref()
            .or(self.positional_file.as_deref())
            .unwrap_or_else(|| Path::new(""))
    }
}

#[derive(Args, Debug, Clone)]
pub struct ReceiptStatusArgs {
    /// Task identifier to query receipt for
    #[arg(index = 1)]
    pub task_id: String,

    /// Path to receipts storage directory (defaults to $K0MARU_RECEIPT_DIR or ~/.k0maru/receipts)
    #[arg(long)]
    pub receipt_dir: Option<PathBuf>,

    /// Output status in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReceiptListArgs {
    /// Path to receipts storage directory (defaults to $K0MARU_RECEIPT_DIR or ~/.k0maru/receipts)
    #[arg(long)]
    pub receipt_dir: Option<PathBuf>,

    /// Output listing in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct SignalArgs {
    /// Task identifier (e.g. task_router_orchestrator_bus_01)
    #[arg(long)]
    pub task_id: String,

    /// Task execution status ('completed', 'failed', 'in_progress')
    #[arg(long, default_value = "completed")]
    pub status: SignalStatus,

    /// Optional summary or notes of task execution
    #[arg(long)]
    pub summary: Option<String>,

    /// Path to signals storage directory (defaults to $K0MARU_SIGNAL_DIR or ~/.k0maru/signals)
    #[arg(long)]
    pub signal_dir: Option<PathBuf>,

    /// Agent pane identifier (defaults to $HERDR_PANE or $TMUX_PANE)
    #[arg(long)]
    pub agent_pane: Option<String>,

    /// Target workspace directory (defaults to current directory)
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,

    /// Output emitted signal in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct OrchestratorCommand {
    #[command(subcommand)]
    pub action: Option<OrchestratorAction>,

    #[command(flatten)]
    pub args: OrchestratorStatusArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum OrchestratorAction {
    /// Inspect status of workspaces, git states, latest signals, and receipts
    Status(OrchestratorStatusArgs),
}

#[derive(Args, Debug, Clone, Default)]
pub struct OrchestratorStatusArgs {
    /// Target workspaces (comma-separated or multiple, defaulting to auto-discovery)
    #[arg(short, long, value_delimiter = ',')]
    pub workspaces: Vec<PathBuf>,

    /// Path to signals storage directory (defaults to $K0MARU_SIGNAL_DIR or ~/.k0maru/signals)
    #[arg(long)]
    pub signal_dir: Option<PathBuf>,

    /// Path to receipts storage directory (defaults to $K0MARU_RECEIPT_DIR or ~/.k0maru/receipts)
    #[arg(long)]
    pub receipt_dir: Option<PathBuf>,

    /// Multiplexer driver to inspect (supported: tmux, herdr, auto)
    #[arg(long, default_value = "auto")]
    pub driver: String,

    /// Output status in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PipelineCommand {
    #[command(subcommand)]
    pub action: Option<PipelineAction>,

    #[command(flatten)]
    pub run_args: PipelineRunArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PipelineAction {
    /// Check the availability and readiness of optional K0maru-Harness components
    Check(PipelineCheckArgs),
    /// Run an end-to-end task execution pipeline
    Run(PipelineRunArgs),
}

#[derive(Args, Debug, Clone, Default)]
pub struct PipelineCheckArgs {
    /// Output result as machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PipelineRunArgs {
    /// Unique task identifier
    #[arg(long, default_value = "task_pipeline_e2e")]
    pub task_id: String,

    /// Target workspace or repository directory
    #[arg(long, short, default_value = ".")]
    pub workspace: PathBuf,

    /// High-level task intent
    #[arg(long, default_value = "verify_or_execute")]
    pub intent: String,

    /// Optional git patch file to apply
    #[arg(long)]
    pub patch: Option<PathBuf>,

    /// Optional explicit test runner override (e.g. cargo, pytest, npm)
    #[arg(long)]
    pub runner: Option<String>,

    /// Isolate execution in a Git Worktree sandbox via k0maru-workspace (optional)
    #[arg(long)]
    pub isolate: bool,

    /// Execute physical test oracle verification via k0maru-verifier (optional)
    #[arg(long, default_value_t = true)]
    pub verify: bool,

    /// Automatically merge and teardown sandbox on verification pass (No-Receipt-No-Merge)
    #[arg(long)]
    pub merge: bool,

    /// Emit task execution completion signal to ~/.k0maru/signals/
    #[arg(long, default_value_t = true)]
    pub signal: bool,

    /// Crystallize task outcome into k0maru memory hub (optional)
    #[arg(long)]
    pub flush: bool,

    /// Allowed files whitelist for scope guard
    #[arg(long = "allowed-files")]
    pub allowed_files: Vec<String>,

    /// Optional remote node hostname or Tailscale alias (e.g. "company-linux")
    #[arg(long)]
    pub node: Option<String>,

    /// Remote multiplexer backend (supported: auto, tmux, herdr)
    #[arg(long, default_value = "auto")]
    pub remote_backend: String,

    /// Output report as JSON
    #[arg(long)]
    pub json: bool,
}

impl Default for PipelineRunArgs {
    fn default() -> Self {
        Self {
            task_id: "task_pipeline_e2e".to_string(),
            workspace: PathBuf::from("."),
            intent: "verify_or_execute".to_string(),
            patch: None,
            runner: None,
            isolate: false,
            verify: true,
            merge: false,
            signal: true,
            flush: false,
            allowed_files: Vec::new(),
            node: None,
            remote_backend: "auto".to_string(),
            json: false,
        }
    }
}

#[derive(Args, Debug, Clone)]
pub struct KdslCommand {
    #[command(subcommand)]
    pub action: KdslAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum KdslAction {
    /// Encode a TaskEnvelope JSON file into a dense K-DSL 0.1 wire string
    Encode(KdslEncodeArgs),
    /// Decode a K-DSL 0.1 wire string into structured AST JSON
    Decode(KdslDecodeArgs),
    /// Output the machine-native K-DSL system prompt contract
    Contract(KdslContractArgs),
}

#[derive(Args, Debug, Clone)]
pub struct KdslEncodeArgs {
    /// Path to the TaskEnvelope JSON file (or '-' for stdin)
    pub file: PathBuf,

    /// Output result as JSON containing wire string, token estimate, and savings
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct KdslDecodeArgs {
    /// K-DSL wire string (e.g., "!δ:0.1|...")
    pub wire: String,

    /// Output full TaskEnvelope JSON instead of KdslPacket JSON
    #[arg(long)]
    pub envelope: bool,

    /// Output as JSON
    #[arg(long, default_value_t = true)]
    pub json: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct KdslContractArgs {
    /// Output contract wrapped in JSON object
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct TupleCommand {
    #[command(subcommand)]
    pub action: TupleAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TupleAction {
    /// Atomically write a tuple into the tuple space (out primitive)
    Out(TupleOutArgs),
    /// Atomically find, claim, and consume a matching tuple (in primitive)
    #[command(name = "in", alias = "take")]
    In(TupleInArgs),
    /// Non-destructively read the first matching tuple (rd primitive)
    Rd(TupleRdArgs),
    /// List all matching tuples without consumption
    List(TupleListArgs),
}

#[derive(Args, Debug, Clone)]
pub struct TupleOutArgs {
    /// Tuple classification or event kind (e.g. TASK_READY, TASK_CLAIMED)
    #[arg(long)]
    pub kind: String,

    /// Target repository or domain identifier (e.g. AUTH-SRV)
    #[arg(long)]
    pub domain: String,

    /// Associated task identifier
    #[arg(long)]
    pub task_id: String,

    /// Tuple payload (JSON string or K-DSL wire string)
    #[arg(long)]
    pub payload: String,

    /// Optional time-to-live in seconds
    #[arg(long)]
    pub ttl: Option<u64>,

    /// Optional explicit tuple identifier
    #[arg(long)]
    pub id: Option<String>,

    /// Override tuple space directory (defaults to ~/.k0maru/tuples)
    #[arg(long)]
    pub tuple_dir: Option<PathBuf>,

    /// Output JSON formatted tuple
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct TupleInArgs {
    /// Optional kind filter
    #[arg(long)]
    pub kind: Option<String>,

    /// Optional domain filter
    #[arg(long)]
    pub domain: Option<String>,

    /// Optional task_id filter
    #[arg(long)]
    pub task_id: Option<String>,

    /// Wait reactively until a matching tuple is published or timeout expires
    #[arg(long)]
    pub wait: bool,

    /// Maximum time to wait in seconds (defaults to 30 if --wait is specified)
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Override tuple space directory (defaults to ~/.k0maru/tuples)
    #[arg(long)]
    pub tuple_dir: Option<PathBuf>,

    /// Output JSON formatted tuple
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct TupleRdArgs {
    /// Optional kind filter
    #[arg(long)]
    pub kind: Option<String>,

    /// Optional domain filter
    #[arg(long)]
    pub domain: Option<String>,

    /// Optional task_id filter
    #[arg(long)]
    pub task_id: Option<String>,

    /// Wait reactively until a matching tuple is published or timeout expires
    #[arg(long)]
    pub wait: bool,

    /// Maximum time to wait in seconds (defaults to 30 if --wait is specified)
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Override tuple space directory (defaults to ~/.k0maru/tuples)
    #[arg(long)]
    pub tuple_dir: Option<PathBuf>,

    /// Output JSON formatted tuple
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct TupleListArgs {
    /// Optional kind filter
    #[arg(long)]
    pub kind: Option<String>,

    /// Optional domain filter
    #[arg(long)]
    pub domain: Option<String>,

    /// Optional task_id filter
    #[arg(long)]
    pub task_id: Option<String>,

    /// Override tuple space directory (defaults to ~/.k0maru/tuples)
    #[arg(long)]
    pub tuple_dir: Option<PathBuf>,

    /// Output JSON formatted list
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct WorkerArgs {
    /// Target domain to consume tasks for (e.g. ROUTER, AUTH-SRV)
    #[arg(long, short)]
    pub domain: String,

    /// Task event kind to listen for (default: TASK_READY)
    #[arg(long, default_value = "TASK_READY")]
    pub kind: String,

    /// Optional task ID to wait for specifically
    #[arg(long)]
    pub task_id: Option<String>,

    /// Process a single task and exit
    #[arg(long)]
    pub once: bool,

    /// Idle timeout in seconds before worker exits if no task is claimed
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Shell command or script to execute when a task is claimed.
    /// Environment variables set: K0MARU_TASK_ID, K0MARU_DOMAIN, K0MARU_PAYLOAD, K0MARU_TUPLE_ID
    #[arg(long)]
    pub exec: Option<String>,

    /// Override tuple space directory (defaults to ~/.k0maru/tuples)
    #[arg(long)]
    pub tuple_dir: Option<PathBuf>,

    /// Output events as machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

fn resolve_driver(name: &str) -> Result<AnyDriver, RouterError> {
    resolve_driver_with_remote(name, None, None)
}

fn resolve_driver_with_remote(
    name: &str,
    node: Option<&str>,
    remote_backend: Option<&str>,
) -> Result<AnyDriver, RouterError> {
    if let Some(n) = node {
        if !k0maru_router::is_local_node(Some(n)) {
            let backend = match remote_backend {
                Some(b) => b.parse::<k0maru_router::RemoteBackend>()?,
                None => k0maru_router::RemoteBackend::Auto,
            };
            return Ok(AnyDriver::Remote(
                k0maru_router::RemoteSshDriver::new(n).with_backend(backend),
            ));
        }
    }
    match name.to_lowercase().as_str() {
        "tmux" => Ok(AnyDriver::Tmux(TmuxDriver::new())),
        "herdr" => Ok(AnyDriver::Herdr(HerdrDriver::new())),
        "auto" => {
            let herdr = HerdrDriver::new();
            if herdr.is_available() {
                Ok(AnyDriver::Herdr(herdr))
            } else {
                Ok(AnyDriver::Tmux(TmuxDriver::new()))
            }
        }
        _ => Err(RouterError::DriverUnavailable {
            driver: "unsupported",
            reason: format!(
                "Driver '{name}' is not supported. Supported drivers: tmux, herdr, auto"
            ),
        }),
    }
}

fn handle_status(args: StatusArgs) -> Result<(), RouterError> {
    let driver = resolve_driver_with_remote(
        &args.driver,
        args.node.as_deref(),
        Some(&args.remote_backend),
    )?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let status = coordinator.system_status()?;

    if args.json {
        let json = serde_json::to_string_pretty(&status)?;
        println!("{json}");
    } else {
        println!("k0maru-router v{}", status.version);
        println!(
            "Multiplexer Driver: {} (available: {}, active sessions: {})",
            status.driver_name(),
            status.driver_available(),
            status.sessions_count()
        );
        println!(
            "HITL Outbox ({}): Total {} (Pending: {}, Approved: {}, Rejected: {})",
            status.outbox.directory.display(),
            status.outbox_total(),
            status.outbox_pending(),
            status.outbox_approved(),
            status.outbox_rejected()
        );
    }
    Ok(())
}

fn handle_probe(args: ProbeArgs) -> Result<(), RouterError> {
    let driver = resolve_driver_with_remote(
        &args.driver,
        args.node.as_deref(),
        Some(&args.remote_backend),
    )?;
    let coordinator = RouterCoordinator::new(driver);
    let workspace = args.resolved_workspace();
    let resolution = coordinator.probe_workspace(workspace)?;

    if args.json {
        let json = serde_json::to_string_pretty(&resolution)?;
        println!("{json}");
    } else {
        match resolution {
            k0maru_router::SessionResolution::HotSession(session) => {
                println!(
                    "HotSession: matched live pane {} at {} (status: {:?}, command: {:?})",
                    session.pane_id,
                    session.cwd.display(),
                    session.status,
                    session.current_command.as_deref().unwrap_or("none")
                );
            }
            k0maru_router::SessionResolution::ColdSpawn { canonical_path } => {
                println!(
                    "ColdSpawn: no active session found for workspace {}",
                    canonical_path.display()
                );
            }
        }
    }
    Ok(())
}

fn configure_coordinator<D: MultiplexerDriver>(
    mut coordinator: RouterCoordinator<D>,
    rule_file: Option<&Path>,
    laya_url: Option<&str>,
    no_memory: bool,
) -> Result<RouterCoordinator<D>, RouterError> {
    let mut cascade = CascadingDecisionEngine::new();
    let mut has_engine = false;

    if let Some(path) = rule_file {
        let rule_engine = DeterministicRuleEngine::from_file(path)?;
        cascade = cascade.with_engine(rule_engine);
        has_engine = true;
    }

    if let Some(url) = laya_url {
        let laya_client = LayaDecisionClient::new().with_base_url(url);
        cascade = cascade.with_engine(laya_client);
        has_engine = true;
    }

    if has_engine {
        coordinator = coordinator.with_decision_engine(cascade);
    }

    if no_memory {
        coordinator = coordinator.with_memory_hook(NoopMemoryHook);
    }

    Ok(coordinator)
}

#[derive(Debug, Deserialize)]
struct CandidateCliEntry {
    #[serde(alias = "identifier")]
    id: String,
    #[serde(default)]
    description: String,
}

fn load_candidates_file(path: &Path) -> Result<Vec<(String, String)>, RouterError> {
    let content = std::fs::read_to_string(path)?;
    if let Ok(vec) = serde_json::from_str::<Vec<CandidateCliEntry>>(&content) {
        return Ok(vec.into_iter().map(|c| (c.id, c.description)).collect());
    }
    #[derive(Deserialize)]
    struct Wrapper {
        candidates: Vec<CandidateCliEntry>,
    }
    if let Ok(w) = serde_json::from_str::<Wrapper>(&content) {
        return Ok(w
            .candidates
            .into_iter()
            .map(|c| (c.id, c.description))
            .collect());
    }
    Err(RouterError::InvalidEnvelope(format!(
        "Failed to parse candidates file '{}'",
        path.display()
    )))
}

fn handle_route(args: RouteArgs) -> Result<(), RouterError> {
    let file = args.resolved_file();
    let mut envelope = TaskEnvelope::from_file(file)?;
    if let Some(ref node) = args.node {
        envelope.target = envelope.target.with_node(node);
    }
    let target_node = envelope.target.node.as_deref();
    let driver = resolve_driver_with_remote(&args.driver, target_node, Some(&args.remote_backend))?;
    let mut coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    coordinator = configure_coordinator(
        coordinator,
        args.rule_file.as_deref(),
        args.laya_url.as_deref(),
        args.no_memory,
    )?;

    let candidate_entries = if let Some(ref cpath) = args.candidates_file {
        load_candidates_file(cpath)?
    } else {
        Vec::new()
    };
    let candidates_slice: Vec<(&str, &str)> = candidate_entries
        .iter()
        .map(|(id, desc)| (id.as_str(), desc.as_str()))
        .collect();

    let outcome =
        coordinator.route_task(&envelope, &candidates_slice, args.dry_run, args.isolate)?;

    if args.json {
        let json = serde_json::to_string_pretty(&outcome)?;
        println!("{json}");
    } else {
        match outcome {
            RouteOutcome::Staged { issue_path, reason } => {
                println!(
                    "[QUARANTINED] External task '{}' staged to HITL outbox: {}",
                    envelope.id,
                    issue_path.display()
                );
                println!("Reason: {reason}");
                println!();
                println!(
                    "To inspect: k0maru-router outbox inspect --id {}",
                    envelope.id
                );
                println!(
                    "To approve: k0maru-router outbox approve --id {}",
                    envelope.id
                );
            }
            RouteOutcome::Dispatched {
                session, sandbox, ..
            } => {
                if let Some(ref sb) = sandbox {
                    println!(
                        "[DISPATCHED] Task '{}' routed to pane {} in isolated sandbox {}",
                        envelope.id,
                        session.pane_id,
                        sb.display()
                    );
                } else {
                    println!(
                        "[DISPATCHED] Task '{}' routed to pane {} in {}",
                        envelope.id,
                        session.pane_id,
                        session.cwd.display()
                    );
                }
                println!("Prompt successfully delivered.");
            }
            RouteOutcome::DryRun {
                verdict,
                resolution,
            } => {
                println!("[DRY-RUN] Task '{}' routing plan:", envelope.id);
                println!("  Verdict:    {verdict}");
                if let Some(res) = resolution {
                    match res {
                        k0maru_router::SessionResolution::HotSession(s) => {
                            println!("  Resolution: HotSession (Pane {})", s.pane_id);
                            println!("  Workspace:  {}", s.cwd.display());
                            println!("  Action:     send_prompt (simulated, no mutation)");
                        }
                        k0maru_router::SessionResolution::ColdSpawn { canonical_path } => {
                            println!("  Resolution: ColdSpawn");
                            println!("  Workspace:  {}", canonical_path.display());
                            println!(
                                "  Action:     spawn_pane & send_prompt (simulated, no mutation)"
                            );
                        }
                    }
                } else {
                    println!(
                        "  Action:     stage to {}/issue_{}.md (simulated, no mutation)",
                        args.outbox_dir.display(),
                        envelope.id
                    );
                }
            }
        }
    }
    Ok(())
}

fn handle_outbox_list(args: OutboxListArgs) -> Result<(), RouterError> {
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let summaries = coordinator.list_staged()?;

    if args.json {
        let json = serde_json::to_string_pretty(&summaries)?;
        println!("{json}");
    } else if summaries.is_empty() {
        println!(
            "No staged issues found in HITL Outbox ({}).",
            coordinator.outbox_dir().display()
        );
    } else {
        println!(
            "STAGED ISSUES ({} total) in {}:",
            summaries.len(),
            coordinator.outbox_dir().display()
        );
        println!(
            "{:<16} {:<10} {:<16} {:<10} TITLE",
            "ID", "STATUS", "TARGET", "SEVERITY"
        );
        println!("{}", "-".repeat(80));
        for s in summaries {
            println!(
                "{:<16} {:<10} {:<16} {:<10} {}",
                s.id, s.status, s.target, s.severity, s.title
            );
        }
    }
    Ok(())
}

fn handle_outbox_inspect(args: OutboxInspectArgs) -> Result<(), RouterError> {
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let id = args.resolved_id();
    let content = coordinator.inspect_staged(id)?;
    print!("{content}");
    Ok(())
}

fn handle_outbox_approve(args: OutboxApproveArgs) -> Result<(), RouterError> {
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let id = args.resolved_id();
    let path = coordinator.approve_staged(id)?;

    if args.json {
        let json = serde_json::json!({
            "id": id,
            "status": "Approved",
            "file_path": path,
            "message": format!("Issue '{id}' successfully approved for outbound transmission.")
        });
        println!("{json}");
    } else {
        println!("Approved staged issue '{id}' ({}).", path.display());
        println!("Status updated to: Approved.");
    }
    Ok(())
}

fn handle_outbox_reject(args: OutboxRejectArgs) -> Result<(), RouterError> {
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let id = args.resolved_id();
    coordinator.reject_staged(id)?;

    if args.json {
        let json = serde_json::json!({
            "id": id,
            "status": "Rejected",
            "message": format!("Issue '{id}' successfully rejected.")
        });
        println!("{json}");
    } else {
        println!("Rejected staged issue '{id}'.");
        println!("Status updated to: Rejected.");
    }
    Ok(())
}

fn handle_mcp(args: McpArgs) -> Result<(), RouterError> {
    let driver = resolve_driver(&args.driver)?;
    let mut coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    coordinator = configure_coordinator(
        coordinator,
        args.rule_file.as_deref(),
        args.laya_url.as_deref(),
        args.no_memory,
    )?;
    let server = k0maru_router::McpServer::new(coordinator);
    server.run_stdio().map_err(RouterError::Io)
}

fn handle_receipt(cmd: ReceiptCommand) -> Result<(), RouterError> {
    match cmd.action {
        ReceiptAction::Ingest(args) => handle_receipt_ingest(args),
        ReceiptAction::Status(args) => handle_receipt_status(args),
        ReceiptAction::List(args) => handle_receipt_list(args),
    }
}

fn handle_receipt_ingest(args: ReceiptIngestArgs) -> Result<(), RouterError> {
    let file = args.resolved_file();
    let contents = std::fs::read_to_string(file).map_err(|e| {
        RouterError::InvalidReceipt(format!(
            "Failed to read receipt file '{}': {e}",
            file.display()
        ))
    })?;

    let receipt: ExecutionReceipt = serde_json::from_str(&contents).map_err(|e| {
        RouterError::InvalidReceipt(format!(
            "Failed to parse receipt JSON from '{}': {e}",
            file.display()
        ))
    })?;

    let patch_bytes = if let Some(ref ppath) = args.patch {
        Some(std::fs::read(ppath).map_err(|e| {
            RouterError::InvalidReceipt(format!(
                "Failed to read patch file '{}': {e}",
                ppath.display()
            ))
        })?)
    } else {
        None
    };

    let receipt_dir = resolve_receipts_dir(args.receipt_dir.as_deref());
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&receipt_dir);
    let summary = coordinator.ingest_receipt_with_patch(receipt, patch_bytes.as_deref())?;

    if args.json {
        let json = serde_json::to_string_pretty(&summary)?;
        println!("{json}");
    } else {
        println!(
            "[RECEIPT INGESTED] Task '{}' (verdict: {})",
            summary.task_id, summary.verdict
        );
        println!(
            "  Passing:       {}",
            if summary.is_passing { "YES" } else { "NO" }
        );
        println!("  Workspace:     {}", summary.workspace.display());
        println!("  Checks:        {}", summary.checks_summary);
        if let Some(ref sha) = summary.git_patch_sha256 {
            println!("  Patch SHA-256: {}", sha);
        }
    }
    Ok(())
}

fn handle_receipt_status(args: ReceiptStatusArgs) -> Result<(), RouterError> {
    let receipt_dir = resolve_receipts_dir(args.receipt_dir.as_deref());
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&receipt_dir);
    let receipt = coordinator
        .get_receipt(&args.task_id)?
        .ok_or_else(|| RouterError::ReceiptNotFound(args.task_id.clone()))?;

    let summary = receipt.summary();
    if args.json {
        let json = serde_json::to_string_pretty(&receipt)?;
        println!("{json}");
    } else {
        println!("RECEIPT STATUS for '{}':", summary.task_id);
        println!("  Verdict:       {}", summary.verdict);
        println!(
            "  Passing:       {}",
            if summary.is_passing { "YES" } else { "NO" }
        );
        println!("  Workspace:     {}", summary.workspace.display());
        println!("  Checks:        {}", summary.checks_summary);
        if let Some(ref ts) = summary.receipt_timestamp {
            println!("  Timestamp:     {}", ts);
        }
        if let Some(ref sha) = summary.git_patch_sha256 {
            println!("  Patch SHA-256: {}", sha);
        }
    }
    Ok(())
}

fn handle_receipt_list(args: ReceiptListArgs) -> Result<(), RouterError> {
    let receipt_dir = resolve_receipts_dir(args.receipt_dir.as_deref());
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&receipt_dir);
    let receipts = coordinator.list_receipts()?;

    if args.json {
        let summaries: Vec<_> = receipts.iter().map(|r| r.summary()).collect();
        let json = serde_json::to_string_pretty(&summaries)?;
        println!("{json}");
    } else if receipts.is_empty() {
        println!("No execution receipts found in {}.", receipt_dir.display());
    } else {
        println!(
            "EXECUTION RECEIPTS ({} total) in {}:",
            receipts.len(),
            receipt_dir.display()
        );
        println!(
            "{:<20} {:<10} {:<10} WORKSPACE",
            "TASK ID", "VERDICT", "PASSING"
        );
        println!("{}", "-".repeat(70));
        for r in &receipts {
            println!(
                "{:<20} {:<10} {:<10} {}",
                r.task_id,
                r.verdict,
                if r.is_passing() { "YES" } else { "NO" },
                r.workspace.display()
            );
        }
    }
    Ok(())
}

fn handle_signal(args: SignalArgs) -> Result<(), RouterError> {
    let signal_dir = resolve_signals_dir(args.signal_dir.as_deref());
    let pane = resolve_agent_pane(args.agent_pane.as_deref());
    let canonical_ws =
        std::fs::canonicalize(&args.workspace).unwrap_or_else(|_| args.workspace.clone());

    let signal = TaskSignal {
        schema: DEFAULT_SIGNAL_SCHEMA.to_string(),
        task_id: args.task_id.clone(),
        status: args.status,
        summary: args.summary,
        agent_pane: pane,
        workspace: canonical_ws,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    signal.validate()?;

    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_signal_dir(&signal_dir);
    let emitted_path = coordinator.emit_signal(&signal)?;

    if args.json {
        let json = serde_json::to_string_pretty(&signal)?;
        println!("{json}");
    } else {
        println!("SIGNAL EMITTED for '{}':", signal.task_id);
        println!("  Status:     {}", signal.status);
        println!("  Workspace:  {}", signal.workspace.display());
        if let Some(ref pane) = signal.agent_pane {
            println!("  Agent Pane: {}", pane);
        }
        if let Some(ref sum) = signal.summary {
            println!("  Summary:    {}", sum);
        }
        println!("  Timestamp:  {}", signal.timestamp);
        println!("  Path:       {}", emitted_path.display());
    }
    Ok(())
}

fn handle_orchestrator(cmd: OrchestratorCommand) -> Result<(), RouterError> {
    let args = match cmd.action {
        Some(OrchestratorAction::Status(s)) => s,
        None => cmd.args,
    };
    let signal_dir = resolve_signals_dir(args.signal_dir.as_deref());
    let receipt_dir = resolve_receipts_dir(args.receipt_dir.as_deref());
    let driver = resolve_driver(&args.driver)?;

    let workspaces = if !args.workspaces.is_empty() {
        args.workspaces
    } else {
        discover_default_workspaces(Some(&driver as &dyn MultiplexerDriver))
    };

    let coordinator = RouterCoordinator::new(driver)
        .with_signal_dir(&signal_dir)
        .with_receipt_dir(&receipt_dir);
    let report = coordinator.orchestrator_report(&workspaces);

    if args.json {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{json}");
    } else {
        println!("{}", format_ascii_table(&report));
    }
    Ok(())
}

fn handle_pipeline(cmd: PipelineCommand) -> Result<(), RouterError> {
    match cmd.action {
        Some(PipelineAction::Check(args)) => {
            let report = k0maru_router::probe_harness_components();
            if args.json {
                let json = serde_json::to_string_pretty(&report)?;
                println!("{json}");
            } else {
                println!("=== K0maru-Harness Component Readiness ===");
                println!(
                    "Status: {}",
                    if report.is_complete {
                        "COMPLETE (All 4 Pillars Ready)"
                    } else {
                        "MODULAR (Partial Components Active)"
                    }
                );
                for c in &report.components {
                    let mark = if c.is_available { "✓" } else { "✗" };
                    let opt = if c.is_optional {
                        "(optional)"
                    } else {
                        "(required)"
                    };
                    let path_str = c.path.as_deref().unwrap_or("not found");
                    println!(
                        "  [{mark}] {:<18} {:<10} {} [{}]",
                        c.name, opt, c.description, path_str
                    );
                }
                println!("Receipts Bus : {}", report.receipts_dir);
                println!("Signals Bus  : {}", report.signals_dir);
                println!("Tuples Bus   : {}", report.tuples_dir);
            }
            Ok(())
        }
        Some(PipelineAction::Run(args)) => run_pipeline_args(args),
        None => run_pipeline_args(cmd.run_args),
    }
}

fn run_pipeline_args(args: PipelineRunArgs) -> Result<(), RouterError> {
    let is_json = args.json;
    let config = k0maru_router::PipelineConfig {
        task_id: args.task_id,
        target_workspace: args.workspace,
        intent: Some(args.intent),
        patch: args.patch,
        runner: args.runner,
        isolate: args.isolate,
        verify: args.verify,
        auto_merge: args.merge,
        emit_signal: args.signal,
        crystallize_memory: args.flush,
        allowed_files: args.allowed_files,
        node: args.node,
        remote_backend: Some(args.remote_backend),
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| RouterError::PipelineError(format!("Failed to build runtime: {e}")))?;

    let report = rt.block_on(k0maru_router::execute_pipeline(&config))?;

    if is_json {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{json}");
    } else {
        println!("=== K0maru Pipeline Run: {} ===", report.task_id);
        println!(
            "Verdict: {} (duration: {}ms)",
            report.verdict.to_uppercase(),
            report.duration_ms
        );
        println!("Stages:");
        for s in &report.stages {
            let mark = match s.status.as_str() {
                "passed" => "✓",
                "skipped" => "-",
                _ => "✗",
            };
            println!(
                "  [{mark}] {:<26} [{:>5}ms] {}",
                s.stage, s.duration_ms, s.message
            );
        }
        if let Some(ref rid) = report.receipt_id {
            println!("Receipt ID: {}", rid);
        }
        if report.signal_emitted {
            println!("Signal    : Emitted to bus");
        }
    }

    if report.verdict != "success" && report.verdict != "skipped" {
        return Err(RouterError::PipelineError(format!(
            "Pipeline stage failure in task {}",
            report.task_id
        )));
    }
    Ok(())
}

fn handle_kdsl(cmd: KdslCommand) -> Result<(), RouterError> {
    match cmd.action {
        KdslAction::Encode(args) => {
            let content = if args.file.as_os_str() == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                    .map_err(|e| RouterError::KdslError(format!("Failed to read stdin: {e}")))?;
                buf
            } else {
                std::fs::read_to_string(&args.file).map_err(|e| {
                    RouterError::KdslError(format!(
                        "Failed to read envelope file '{}': {e}",
                        args.file.display()
                    ))
                })?
            };

            let envelope: TaskEnvelope = serde_json::from_str(&content)
                .map_err(|e| RouterError::KdslError(format!("Invalid TaskEnvelope JSON: {e}")))?;

            let packet = envelope_to_packet(&envelope)?;
            let wire = packet.to_wire();

            if args.json {
                let json = serde_json::json!({
                    "status": "success",
                    "wire": wire,
                    "estimated_tokens": estimate_tokens(&wire),
                    "token_budget": packet_token_budget(&packet),
                    "packet": packet
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{wire}");
            }
            Ok(())
        }
        KdslAction::Decode(args) => {
            let packet = parse_kdsl(&args.wire)?;
            if args.envelope {
                let env = packet_to_envelope(&packet);
                println!("{}", serde_json::to_string_pretty(&env)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&packet)?);
            }
            Ok(())
        }
        KdslAction::Contract(args) => {
            let contract = kdsl_contract_snippet();
            if args.json {
                let json = serde_json::json!({
                    "protocol": "K-DSL",
                    "version": "0.1",
                    "contract": contract
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{contract}");
            }
            Ok(())
        }
    }
}

fn handle_tuple(cmd: TupleCommand) -> Result<(), RouterError> {
    match cmd.action {
        TupleAction::Out(args) => {
            let dir = resolve_tuples_dir(args.tuple_dir.as_deref());
            let engine = TupleSpaceEngine::new(dir);
            let mut tuple = Tuple::new(args.kind, args.domain, args.task_id, args.payload);
            if let Some(id) = args.id {
                tuple = tuple.with_id(id);
            }
            if let Some(ttl) = args.ttl {
                tuple = tuple.with_ttl(ttl);
            }
            let saved = engine.out(tuple)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&saved)?);
            } else {
                println!(
                    "TUPLE OUT: id={} kind={} domain={} task_id={}",
                    saved.id, saved.kind, saved.domain, saved.task_id
                );
            }
            Ok(())
        }
        TupleAction::In(args) => {
            let dir = resolve_tuples_dir(args.tuple_dir.as_deref());
            let engine = TupleSpaceEngine::new(dir);
            let mut pattern = TuplePattern::new();
            if let Some(k) = args.kind {
                pattern = pattern.with_kind(k);
            }
            if let Some(d) = args.domain {
                pattern = pattern.with_domain(d);
            }
            if let Some(t) = args.task_id {
                pattern = pattern.with_task_id(t);
            }
            let claimed = if args.wait || args.timeout.is_some() {
                let timeout_secs = args.timeout.unwrap_or(30);
                engine.take_wait(&pattern, std::time::Duration::from_secs(timeout_secs))?
            } else {
                engine.take(&pattern)?
            };
            match claimed {
                Some(t) => {
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&t)?);
                    } else {
                        println!(
                            "TUPLE IN: id={} kind={} domain={} task_id={} payload={}",
                            t.id, t.kind, t.domain, t.task_id, t.payload
                        );
                    }
                    Ok(())
                }
                None => {
                    if args.json {
                        let not_found = serde_json::json!({ "status": "not_found" });
                        println!("{}", serde_json::to_string_pretty(&not_found)?);
                    } else {
                        println!("No matching tuple found in space");
                    }
                    Ok(())
                }
            }
        }
        TupleAction::Rd(args) => {
            let dir = resolve_tuples_dir(args.tuple_dir.as_deref());
            let engine = TupleSpaceEngine::new(dir);
            let mut pattern = TuplePattern::new();
            if let Some(k) = args.kind {
                pattern = pattern.with_kind(k);
            }
            if let Some(d) = args.domain {
                pattern = pattern.with_domain(d);
            }
            if let Some(t) = args.task_id {
                pattern = pattern.with_task_id(t);
            }
            let found = if args.wait || args.timeout.is_some() {
                let timeout_secs = args.timeout.unwrap_or(30);
                engine.rd_wait(&pattern, std::time::Duration::from_secs(timeout_secs))?
            } else {
                engine.rd(&pattern)?
            };
            match found {
                Some(t) => {
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&t)?);
                    } else {
                        println!(
                            "TUPLE RD: id={} kind={} domain={} task_id={} payload={}",
                            t.id, t.kind, t.domain, t.task_id, t.payload
                        );
                    }
                    Ok(())
                }
                None => {
                    if args.json {
                        let not_found = serde_json::json!({ "status": "not_found" });
                        println!("{}", serde_json::to_string_pretty(&not_found)?);
                    } else {
                        println!("No matching tuple found in space");
                    }
                    Ok(())
                }
            }
        }
        TupleAction::List(args) => {
            let dir = resolve_tuples_dir(args.tuple_dir.as_deref());
            let engine = TupleSpaceEngine::new(dir);
            let mut pattern = TuplePattern::new();
            if let Some(k) = args.kind {
                pattern = pattern.with_kind(k);
            }
            if let Some(d) = args.domain {
                pattern = pattern.with_domain(d);
            }
            if let Some(t) = args.task_id {
                pattern = pattern.with_task_id(t);
            }
            let tuples = engine.list(&pattern)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&tuples)?);
            } else {
                println!("Found {} tuple(s):", tuples.len());
                for t in &tuples {
                    println!(
                        "  - [{}] kind={} domain={} task_id={}",
                        t.id, t.kind, t.domain, t.task_id
                    );
                }
            }
            Ok(())
        }
    }
}

fn handle_worker(args: WorkerArgs) -> Result<(), RouterError> {
    let dir = resolve_tuples_dir(args.tuple_dir.as_deref());
    let engine = TupleSpaceEngine::new(dir);

    let pattern = TuplePattern::new()
        .with_kind(&args.kind)
        .with_domain(&args.domain);
    let pattern = if let Some(ref t) = args.task_id {
        pattern.with_task_id(t)
    } else {
        pattern
    };

    if !args.json {
        println!(
            "=== K0MARU WORKER LOOP STARTED ===\nDomain : {}\nKind   : {}\nOnce   : {}\nBus    : {}",
            args.domain,
            args.kind,
            args.once,
            engine.tuple_dir().display()
        );
    }

    let start_time = std::time::Instant::now();
    let poll_timeout = std::time::Duration::from_millis(200);

    loop {
        // If overall idle timeout is set and exceeded, break
        if let Some(to) = args.timeout {
            if start_time.elapsed() >= std::time::Duration::from_secs(to) {
                if args.json {
                    let ev = serde_json::json!({ "event": "timeout", "status": "idle_timeout" });
                    println!("{}", serde_json::to_string_pretty(&ev)?);
                } else {
                    println!(
                        "[WORKER] Idle timeout reached ({}s). Exiting worker loop.",
                        to
                    );
                }
                break;
            }
        }

        // Attempt claim with short wait
        if let Some(tuple) = engine.take_wait(&pattern, poll_timeout)? {
            if args.json {
                let ev = serde_json::json!({
                    "event": "task_claimed",
                    "tuple": tuple,
                });
                println!("{}", serde_json::to_string_pretty(&ev)?);
            } else {
                println!(
                    "[WORKER] Claimed task '{}' (tuple_id: {})",
                    tuple.task_id, tuple.id
                );
            }

            // If an exec command is specified, execute it
            if let Some(ref cmd_str) = args.exec {
                if !args.json {
                    println!("[WORKER] Executing command: {}", cmd_str);
                }
                let mut cmd = std::process::Command::new("sh");
                cmd.arg("-c").arg(cmd_str);
                cmd.env("K0MARU_TASK_ID", &tuple.task_id);
                cmd.env("K0MARU_DOMAIN", &tuple.domain);
                cmd.env("K0MARU_PAYLOAD", &tuple.payload);
                cmd.env("K0MARU_TUPLE_ID", &tuple.id);

                let status = cmd.status().map_err(|e| {
                    RouterError::TupleSpaceError(format!("Worker exec failed: {e}"))
                })?;

                if !args.json {
                    println!(
                        "[WORKER] Command finished with exit code: {:?}",
                        status.code()
                    );
                }
            }

            if args.once {
                if !args.json {
                    println!("[WORKER] Task completed (--once specified). Exiting worker loop.");
                }
                break;
            }
        }
    }

    Ok(())
}

fn run(cli: Cli) -> Result<(), RouterError> {
    match cli.command {
        Commands::Status(args) => handle_status(args),
        Commands::Probe(args) => handle_probe(args),
        Commands::Route(args) => handle_route(args),
        Commands::Mcp(args) => handle_mcp(args),
        Commands::Outbox(cmd) => match cmd.action {
            OutboxAction::List(args) => handle_outbox_list(args),
            OutboxAction::Inspect(args) => handle_outbox_inspect(args),
            OutboxAction::Approve(args) => handle_outbox_approve(args),
            OutboxAction::Reject(args) => handle_outbox_reject(args),
        },
        Commands::Receipt(cmd) => handle_receipt(cmd),
        Commands::Signal(args) => handle_signal(args),
        Commands::Orchestrator(cmd) => handle_orchestrator(cmd),
        Commands::Pipeline(cmd) => handle_pipeline(cmd),
        Commands::Kdsl(cmd) => handle_kdsl(cmd),
        Commands::Tuple(cmd) => handle_tuple(cmd),
        Commands::Worker(args) => handle_worker(args),
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            e.exit(); // Automatically prints to stderr and exits with code 2 for CLI parse errors
        }
    };

    let is_json = cli.is_json();
    match run(cli) {
        Ok(()) => ExitCode::from(0),
        Err(err) => {
            if is_json {
                let err_type = match &err {
                    RouterError::InvalidEnvelope(_) => "InvalidEnvelope",
                    RouterError::IssueNotFound(_) => "IssueNotFound",
                    RouterError::WorkspaceNotFound(_) => "WorkspaceNotFound",
                    RouterError::DriverUnavailable { .. } => "DriverUnavailable",
                    RouterError::DriverExecutionFailed(_) => "DriverExecutionFailed",
                    RouterError::OutboxError(_) => "OutboxError",
                    RouterError::SessionNotFound(_) => "SessionNotFound",
                    RouterError::Io(_) => "Io",
                    RouterError::Json(_) => "Json",
                    RouterError::DateTimeParse(_) => "DateTimeParse",
                    RouterError::InvalidReceipt(_) => "InvalidReceipt",
                    RouterError::ReceiptNotFound(_) => "ReceiptNotFound",
                    RouterError::InvalidSignal(_) => "InvalidSignal",
                    RouterError::SignalNotFound(_) => "SignalNotFound",
                    RouterError::WorkspaceError(_) => "WorkspaceError",
                    RouterError::PipelineError(_) => "PipelineError",
                    RouterError::KdslError(_) => "KdslError",
                    RouterError::TupleSpaceError(_) => "TupleSpaceError",
                };
                let json_err = serde_json::json!({
                    "status": "error",
                    "error": err_type,
                    "message": err.to_string(),
                });
                eprintln!("{json_err}");
            } else {
                eprintln!("Error: {err}");
            }
            ExitCode::from(1)
        }
    }
}
