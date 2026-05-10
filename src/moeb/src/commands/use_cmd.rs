use anyhow::{Context, Result};
use rpassword::prompt_password;

use crate::config::{MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai"];

pub fn run(adapter: &str) -> Result<()> {
    match adapter {
        "openai" => configure_openai(),
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
    Ok(())
}
