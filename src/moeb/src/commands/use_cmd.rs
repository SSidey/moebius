use anyhow::{Context, Result};
use rpassword::prompt_password;

use crate::adapters::anthropic::DEFAULT_TIMEOUT_SECS as ANTHROPIC_DEFAULT_TIMEOUT_SECS;
use crate::adapters::gemini::DEFAULT_TIMEOUT_SECS as GEMINI_DEFAULT_TIMEOUT_SECS;
use crate::config::{MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai", "anthropic", "gemini", "ollama"];

const OPENAI_DEFAULT_MODEL: &str = "gpt-4o";
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-4-7";
const GEMINI_DEFAULT_MODEL: &str = "gemini-2.0-flash";
const OLLAMA_DEFAULT_MODEL: &str = "llama3.1";

pub fn run(adapter: &str) -> Result<()> {
    match adapter {
        "openai" => configure_openai(),
        "anthropic" => configure_anthropic(),
        "gemini" => configure_gemini(),
        "ollama" => configure_ollama(),
        other => anyhow::bail!(
            "Unknown adapter '{}'. Available adapters: {}",
            other,
            KNOWN_ADAPTERS.join(", ")
        ),
    }
}

fn credential_key_for(adapter: &str) -> &'static str {
    match adapter {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => unreachable!("caller validates adapter name"),
    }
}

fn adapter_display_name(adapter: &str) -> &str {
    match adapter {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "gemini" => "Gemini",
        "ollama" => "Ollama",
        _ => adapter,
    }
}

fn configure_adapter(
    adapter: &str,
    secret_key: &str,
    mut read_key: impl FnMut(&str) -> Result<String>,
    print_summary: fn(&MoebConfig),
) -> Result<()> {
    let secrets = Secrets::load()?;
    let already_configured = secrets.get(secret_key).is_some();

    let prompt = if already_configured {
        println!("A key is already set for {}.", adapter_display_name(adapter));
        format!("Enter new API key (or press Enter to keep the existing one): ")
    } else {
        format!("Enter {} API key: ", adapter_display_name(adapter))
    };

    let new_key = read_key(&prompt)?;
    let trimmed = new_key.trim();

    if trimmed.is_empty() {
        if !already_configured {
            anyhow::bail!("API key must not be empty.");
        }
    } else {
        let mut secrets = Secrets::load()?;
        secrets.set(secret_key, trimmed)?;
    }

    let mut config = MoebConfig::load()?;
    config.active_adapter = Some(adapter.to_string());
    config.save()?;

    println!("{} adapter configured.", adapter_display_name(adapter));
    print_summary(&config);
    Ok(())
}

fn configure_openai() -> Result<()> {
    configure_adapter(
        "openai",
        credential_key_for("openai"),
        |prompt| prompt_password(prompt).context("Failed to read API key"),
        print_openai_config_summary,
    )
}

fn configure_anthropic() -> Result<()> {
    configure_adapter(
        "anthropic",
        credential_key_for("anthropic"),
        |prompt| prompt_password(prompt).context("Failed to read API key"),
        print_anthropic_config_summary,
    )
}

fn configure_gemini() -> Result<()> {
    configure_adapter(
        "gemini",
        credential_key_for("gemini"),
        |prompt| prompt_password(prompt).context("Failed to read API key"),
        print_gemini_config_summary,
    )
}

fn configure_ollama() -> Result<()> {
    let mut config = MoebConfig::load()?;
    config.active_adapter = Some("ollama".to_string());
    config.save()?;
    println!("Ollama adapter configured.");
    print_ollama_config_summary(&config);
    Ok(())
}

pub fn print_anthropic_config_summary(config: &MoebConfig) {
    let ac = config.adapter_config("anthropic");
    let model = ac.effective_model(ANTHROPIC_DEFAULT_MODEL);
    let retries = ac.effective_retries();
    let timeout = ac.effective_timeout_secs(ANTHROPIC_DEFAULT_TIMEOUT_SECS);

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
    println!(
        "  {:<8} {:<16} moeb adapter anthropic config TIMEOUT <seconds>",
        "TIMEOUT", timeout
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

pub fn print_gemini_config_summary(config: &MoebConfig) {
    let ac = config.adapter_config("gemini");
    let model = ac.effective_model(GEMINI_DEFAULT_MODEL);
    let retries = ac.effective_retries();
    let timeout = ac.effective_timeout_secs(GEMINI_DEFAULT_TIMEOUT_SECS);

    println!();
    println!("Configuration options (current effective values):");
    println!(
        "  {:<8} {:<16} moeb adapter gemini config MODEL <value>",
        "MODEL", model
    );
    println!(
        "  {:<8} {:<16} moeb adapter gemini config RETRIES <count>",
        "RETRIES", retries
    );
    println!(
        "  {:<8} {:<16} moeb adapter gemini config TIMEOUT <seconds>",
        "TIMEOUT", timeout
    );
    println!();
    println!("To remove credentials: moeb adapter gemini release");
}

pub fn print_ollama_config_summary(config: &MoebConfig) {
    let ac = config.adapter_config("ollama");
    let model = ac.effective_model(OLLAMA_DEFAULT_MODEL);
    let retries = ac.effective_retries();

    println!();
    println!("Configuration options (current effective values):");
    println!(
        "  {:<8} {:<16} moeb adapter ollama config MODEL <value>",
        "MODEL", model
    );
    println!(
        "  {:<8} {:<16} moeb adapter ollama config RETRIES <count>",
        "RETRIES", retries
    );
    println!();
    println!("Ollama uses a local service and does not require API credentials.");
}

#[cfg(test)]
#[path = "use_cmd_tests.rs"]
mod tests;
