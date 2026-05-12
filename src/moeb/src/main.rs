use clap::{Parser, Subcommand};

use adapters::cli::CliAdapter;
use ports::{AdapterManagementPort, InitPort, ListAdaptersPort, RunPort, SpecPort, UseAdapterPort};

pub mod adapters;
pub mod agent;
pub mod assets;
pub mod commands;
pub mod config;
pub mod domain;
pub mod ports;

#[derive(Parser)]
#[command(name = "moeb", about = "Declarative harness kernel")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise the .moeb/ harness in the current directory
    Init,
    /// Configure an AI adapter
    Use { adapter: String },
    /// Produce a specification from raw input
    Spec {
        #[arg(trailing_var_arg = true)]
        input: Vec<String>,
    },
    /// Run the next implementation step for a specification
    Run { spec: String },
    /// List all adapters and their configured state
    Adapters,
    /// Manage a specific adapter's configuration or credentials
    Adapter {
        name: String,
        #[command(subcommand)]
        action: AdapterAction,
    },
}

#[derive(Subcommand)]
enum AdapterAction {
    /// Set a configuration value for this adapter
    Config { key: String, value: String },
    /// Remove this adapter's credentials
    Release,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let adapter = CliAdapter;
    match cli.command {
        Commands::Init => InitPort::run(&adapter),
        Commands::Use { adapter: name } => UseAdapterPort::run(&adapter, &name),
        Commands::Spec { input } => SpecPort::run(&adapter, &input.join(" ")),
        Commands::Run { spec } => RunPort::run(&adapter, &spec),
        Commands::Adapters => ListAdaptersPort::run(&adapter),
        Commands::Adapter { name, action: AdapterAction::Config { key, value } } => {
            AdapterManagementPort::configure(&adapter, &name, &key, &value)
        }
        Commands::Adapter { name, action: AdapterAction::Release } => {
            AdapterManagementPort::release(&adapter, &name)
        }
    }
}
