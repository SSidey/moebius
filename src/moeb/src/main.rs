use anyhow::Context;
use clap::{Parser, Subcommand};

use adapters::cli::CliAdapter;
use ports::{AdapterManagementPort, InitPort, ListAdaptersPort, RunPort, SpecPort, UseAdapterPort};

pub mod adapters;
pub mod agent;
pub mod assets;
pub mod commands;
pub mod compaction;
pub mod config;
pub mod domain;
pub mod ports;
pub mod tools;
pub mod trace;
pub mod version_tests;

#[derive(Parser)]
#[command(
    name = "moeb",
    about = "Declarative harness kernel",
    version = env!("CARGO_PKG_VERSION")
)]
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
        /// Force full file content in trace (overrides LOG_FILE_CONTENT)
        #[arg(long)]
        embed_files: bool,
        /// Force hash-only file content in trace (overrides LOG_FILE_CONTENT)
        #[arg(long)]
        hash_files: bool,
    },
    /// Run the next implementation step for a specification
    Run {
        spec: String,
        /// Force full file content in trace (overrides LOG_FILE_CONTENT)
        #[arg(long)]
        embed_files: bool,
        /// Force hash-only file content in trace (overrides LOG_FILE_CONTENT)
        #[arg(long)]
        hash_files: bool,
    },
    /// List all adapters and their configured state
    Adapters,
    /// Manage a specific adapter's configuration or credentials
    Adapter {
        name: String,
        #[command(subcommand)]
        action: AdapterAction,
    },
    /// Set or list persistent kernel configuration values
    Configure {
        /// Configuration key (RUN_RETENTION, LOG_FILE_CONTENT)
        key: Option<String>,
        /// Value to assign
        value: Option<String>,
        /// List all configuration keys and their current values
        #[arg(long)]
        list: bool,
    },
    /// Replay a captured trace without calling the real API
    Replay {
        /// Path to the trace JSON file
        trace_file: String,
        /// Attempt number to replay (default: last successful, or last if all failed)
        #[arg(long)]
        attempt: Option<u32>,
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
        Commands::Spec { input, embed_files, hash_files } => {
            if embed_files && hash_files {
                anyhow::bail!("--embed-files and --hash-files are mutually exclusive.");
            }
            let file_content_mode = resolve_file_content_mode(embed_files, hash_files);
            SpecPort::run(&adapter, &input.join(" "), file_content_mode)
        }
        Commands::Run { spec, embed_files, hash_files } => {
            if embed_files && hash_files {
                anyhow::bail!("--embed-files and --hash-files are mutually exclusive.");
            }
            let file_content_mode = resolve_file_content_mode(embed_files, hash_files);
            RunPort::run(&adapter, &spec, file_content_mode)
        }
        Commands::Adapters => ListAdaptersPort::run(&adapter),
        Commands::Adapter { name, action: AdapterAction::Config { key, value } } => {
            AdapterManagementPort::configure(&adapter, &name, &key, &value)
        }
        Commands::Adapter { name, action: AdapterAction::Release } => {
            AdapterManagementPort::release(&adapter, &name)
        }
        Commands::Configure { key, value, list } => {
            if list {
                commands::configure::run_list()?;
            } else {
                let k = key.context("Key is required. Usage: moeb configure <KEY> <VALUE>")?;
                let v = value.context("Value is required. Usage: moeb configure <KEY> <VALUE>")?;
                commands::configure::run_configure(&k, &v)?;
            }
            Ok(())
        }
        Commands::Replay { trace_file, attempt } => {
            commands::replay::run_replay(&trace_file, attempt)
        }
    }
}

fn resolve_file_content_mode(embed_files: bool, hash_files: bool) -> trace::FileContentMode {
    if embed_files {
        trace::FileContentMode::Embed
    } else if hash_files {
        trace::FileContentMode::Hash
    } else if config::MoebConfig::load().unwrap_or_default().effective_log_file_content() {
        trace::FileContentMode::Embed
    } else {
        trace::FileContentMode::Hash
    }
}
