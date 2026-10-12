//! Cleanroom CLI binary entrypoint for k0maru-agent-router.

use clap::{Args, Parser, Subcommand};
use k0maru_router::coordinator::{RouteOutcome, RouterCoordinator};
use k0maru_router::decision::{
    CascadingDecisionEngine, DeterministicRuleEngine, LayaDecisionClient,
};
use k0maru_router::driver::{AnyDriver, HerdrDriver, MultiplexerDriver, TmuxDriver};
use k0maru_router::envelope::TaskEnvelope;
use k0maru_router::error::RouterError;
use k0maru_router::memory::NoopMemoryHook;
use k0maru_router::receipt::ExecutionReceipt;
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
            Commands::ListStaged(args) => args.json,
            Commands::InspectStaged(_) => false,
            Commands::Approve(args) => args.json,
            Commands::Reject(args) => args.json,
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

    // --- Ergonomic Top-Level Aliases ---
    /// List staged issues awaiting human review (alias for 'outbox list')
    #[command(name = "list-staged")]
    ListStaged(OutboxListArgs),

    /// Inspect a staged issue draft (alias for 'outbox inspect')
    #[command(name = "inspect-staged")]
    InspectStaged(OutboxInspectArgs),

    /// Approve a staged issue draft for outbound dispatch (alias for 'outbox approve')
    #[command(name = "approve")]
    Approve(OutboxApproveArgs),

    /// Reject a staged issue draft (alias for 'outbox reject')
    #[command(name = "reject")]
    Reject(OutboxRejectArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Path to the staged outbox directory
    #[arg(long, default_value = ".scratch/staged")]
    pub outbox_dir: PathBuf,

    /// Multiplexer driver to inspect (supported: tmux, herdr, auto)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

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

    /// Multiplexer driver to use (supported: tmux)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

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

    /// Multiplexer driver to use (supported: tmux)
    #[arg(long, default_value = "tmux")]
    pub driver: String,

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

    /// Path to receipts storage directory
    #[arg(long, default_value = ".scratch/receipts")]
    pub receipt_dir: PathBuf,

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

    /// Path to receipts storage directory
    #[arg(long, default_value = ".scratch/receipts")]
    pub receipt_dir: PathBuf,

    /// Output status in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReceiptListArgs {
    /// Path to receipts storage directory
    #[arg(long, default_value = ".scratch/receipts")]
    pub receipt_dir: PathBuf,

    /// Output listing in machine-readable JSON format
    #[arg(long)]
    pub json: bool,
}

fn resolve_driver(name: &str) -> Result<AnyDriver, RouterError> {
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
    let driver = resolve_driver(&args.driver)?;
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
    let driver = resolve_driver(&args.driver)?;
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
    let driver = resolve_driver(&args.driver)?;
    let mut coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    coordinator = configure_coordinator(
        coordinator,
        args.rule_file.as_deref(),
        args.laya_url.as_deref(),
        args.no_memory,
    )?;
    let file = args.resolved_file();

    let candidate_entries = if let Some(ref cpath) = args.candidates_file {
        load_candidates_file(cpath)?
    } else {
        Vec::new()
    };
    let candidates_slice: Vec<(&str, &str)> = candidate_entries
        .iter()
        .map(|(id, desc)| (id.as_str(), desc.as_str()))
        .collect();

    let envelope = TaskEnvelope::from_file(file)?;
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

    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&args.receipt_dir);
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
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&args.receipt_dir);
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
    let driver = resolve_driver("auto")?;
    let coordinator = RouterCoordinator::new(driver).with_receipt_dir(&args.receipt_dir);
    let receipts = coordinator.list_receipts()?;

    if args.json {
        let summaries: Vec<_> = receipts.iter().map(|r| r.summary()).collect();
        let json = serde_json::to_string_pretty(&summaries)?;
        println!("{json}");
    } else if receipts.is_empty() {
        println!(
            "No execution receipts found in {}.",
            args.receipt_dir.display()
        );
    } else {
        println!(
            "EXECUTION RECEIPTS ({} total) in {}:",
            receipts.len(),
            args.receipt_dir.display()
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
        Commands::ListStaged(args) => handle_outbox_list(args),
        Commands::InspectStaged(args) => handle_outbox_inspect(args),
        Commands::Approve(args) => handle_outbox_approve(args),
        Commands::Reject(args) => handle_outbox_reject(args),
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
                    RouterError::WorkspaceError(_) => "WorkspaceError",
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
