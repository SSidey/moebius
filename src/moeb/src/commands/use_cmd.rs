use anyhow::{Context, Result};
use rpassword::prompt_password;

use crate::config::{MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai", "anthropic"];

const OPENAI_DEFAULT_MODEL: &str = "gpt-4o";
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-4-7";

pub fn run(adapter: &str) -> Result<()> {
    match adapter {
        "openai" => configure_openai(),
        "anthropic" => configure_anthropic(),
        other => anyhow::bail!(
            "Unknown adapter '{}'. Available adapters: {}",
            other,
            KNOWN_ADAPTERS.join(", ")
        ),
    }
}

fn configure_openai() -> Result<()> {
    let key = prompt_password("Enter OpenAI API key: ").context("Failed to read API key")?;

    if key.trim().is_empty() {
        anyhow::bail!("API key must not be empty.");
    }

    let mut secrets = Secrets::load()?;
    secrets.set("OPENAI_API_KEY", key.trim())?;

    let mut config = MoebConfig::load()?;
    config.active_adapter = Some("openai".to_string());
    config.save()?;

    println!("OpenAI adapter configured.");
    print_openai_config_summary(&config);
    Ok(())
}

fn configure_anthropic() -> Result<()> {
    let key = prompt_password("Enter Anthropic API key: ").context("Failed to read API key")?;

    if key.trim().is_empty() {
        anyhow::bail!("API key must not be empty.");
    }

    let mut secrets = Secrets::load()?;
    secrets.set("ANTHROPIC_API_KEY", key.trim())?;

    let mut config = MoebConfig::load()?;
    config.active_adapter = Some("anthropic".to_string());
    config.save()?;

    println!("Anthropic adapter configured.");
    print_anthropic_config_summary(&config);
    Ok(())
}

pub fn print_anthropic_config_summary(config: &MoebConfig) {
    let ac = config.adapter_config("anthropic");
    let model = ac.effective_model(ANTHROPIC_DEFAULT_MODEL);
    let retries = ac.effective_retries();

    println!();
    println!("Configuration options (current effective values):");
    println!(
        "  {:<8} {:<16} moeb adapter anthropic config MODEL <value>",
        "MODEL", model
    );
    println!(
        "  {:<8} {:<16} moeb adapter anthropic config RETRIES <count>",
        "RETRIES", retries
    );
    println!();
    println!("To remove credentials: moeb adapter anthropic release");
}

pub fn print_openai_config_summary(config: &MoebConfig) {
    let ac = config.adapter_config("openai");
    let model = ac.effective_model(OPENAI_DEFAULT_MODEL);
    let retries = ac.effective_retries();

    println!();
    println!("Configuration options (current effective values):");
    println!(
        "  {:<8} {:<16} moeb adapter openai config MODEL <value>",
        "MODEL", model
    );
    println!(
        "  {:<8} {:<16} moeb adapter openai config RETRIES <count>",
        "RETRIES", retries
    );
    println!();
    println!("To remove credentials: moeb adapter openai release");
}
