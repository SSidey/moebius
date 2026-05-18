use anyhow::Result;

use crate::config::{MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai", "anthropic", "gemini", "ollama"];

fn secret_key_for(adapter: &str) -> Option<&'static str> {
    match adapter {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gemini" => Some("GEMINI_API_KEY"),
        _ => None,
    }
}

pub fn run() -> Result<()> {
    let config = MoebConfig::load().unwrap_or_default();
    let secrets = Secrets::load().unwrap_or_else(|_| Secrets::empty());

    println!("{:<12} {:<16} {}", "ADAPTER", "STATE", "ACTIVE");
    for &name in KNOWN_ADAPTERS {
        let secret_key = secret_key_for(name).unwrap_or("");
        let configured = if name == "ollama" { true } else { !secret_key.is_empty() && secrets.get(secret_key).is_some() };
        let state = if configured { "configured" } else { "not configured" };
        let active = if config.active_adapter.as_deref() == Some(name) { "yes" } else { "no" };
        println!("{:<12} {:<16} {}", name, state, active);
    }
    if config.effective_prompt_cache() {
        println!("Prompt cache: enabled   (Anthropic: explicit; OpenAI: automatic)");
        println!("  To disable: moeb configure PROMPT_CACHE false");
    } else {
        println!("Prompt cache: disabled  (Anthropic: no cache_control sent; OpenAI: unaffected)");
        println!("  To enable:  moeb configure PROMPT_CACHE true");
    }
    Ok(())
}
