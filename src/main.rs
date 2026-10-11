use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "k0maru-router", version, about = "K0maru-Harness Agent Router")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check system status and driver availability
    Status,
    /// Probe active multiplexer sessions matching a target workspace CWD
    Probe {
        #[arg(short, long)]
        workspace: String,
    },
    /// Route or stage a task envelope JSON file
    Route {
        #[arg(short, long)]
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status => {
            println!("k0maru-router v{} initialized", env!("CARGO_PKG_VERSION"));
        }
        Commands::Probe { workspace } => {
            println!("Probing workspace: {}", workspace);
        }
        Commands::Route { file } => {
            println!("Routing envelope from file: {}", file);
        }
    }
}
