use clap::{Parser, Subcommand};

pub mod adapters;
pub mod agent;
pub mod assets;
pub mod commands;
pub mod config;

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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Use { adapter } => commands::use_cmd::run(&adapter),
        Commands::Spec { input } => commands::spec::run(&input.join(" ")),
        Commands::Run { spec } => commands::run::run(&spec),
    }
}
