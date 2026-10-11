//! Cleanroom CLI binary entrypoint for k0maru-agent-router.

use clap::{Args, Parser, Subcommand};
use k0maru_router::coordinator::{RouteOutcome, RouterCoordinator};
use k0maru_router::driver::TmuxDriver;
use k0maru_router::envelope::TaskEnvelope;
use k0maru_router::error::RouterError;
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

fn resolve_driver(name: &str) -> Result<TmuxDriver, RouterError> {
    match name.to_lowercase().as_str() {
        "tmux" => Ok(TmuxDriver::new()),
        _ => Err(RouterError::DriverUnavailable {
            driver: "unsupported",
            reason: format!("Driver '{name}' is not supported. Supported drivers: tmux"),
        }),
    }
}

fn handle_status(args: StatusArgs) -> Result<(), RouterError> {
    let driver = TmuxDriver::new();
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

fn handle_route(args: RouteArgs) -> Result<(), RouterError> {
    let driver = resolve_driver(&args.driver)?;
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let file = args.resolved_file();

    let envelope = TaskEnvelope::from_file(file)?;
    let outcome = coordinator.route_envelope(&envelope, args.dry_run)?;

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
            RouteOutcome::Dispatched { session, .. } => {
                println!(
                    "[DISPATCHED] Task '{}' routed to pane {} in {}",
                    envelope.id,
                    session.pane_id,
                    session.cwd.display()
                );
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
    let driver = TmuxDriver::new();
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
    let driver = TmuxDriver::new();
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let id = args.resolved_id();
    let content = coordinator.inspect_staged(id)?;
    print!("{content}");
    Ok(())
}

fn handle_outbox_approve(args: OutboxApproveArgs) -> Result<(), RouterError> {
    let driver = TmuxDriver::new();
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
    let driver = TmuxDriver::new();
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
    let coordinator = RouterCoordinator::new(driver).with_outbox_dir(&args.outbox_dir);
    let server = k0maru_router::McpServer::new(coordinator);
    server.run_stdio().map_err(RouterError::Io)
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
