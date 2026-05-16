use anyhow::Result;

use crate::config::{AdapterConfig, MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai", "anthropic", "ollama"];

fn secret_key_for(adapter: &str) -> Option<&'static str> {
    match adapter {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        _ => None,
    }
}

fn valid_keys_for(adapter: &str) -> &'static [&'static str] {
    match adapter {
        "openai" => &["MODEL", "RETRIES"],
        "anthropic" => &["MODEL", "RETRIES", "TIMEOUT"],
        "ollama" => &["MODEL", "RETRIES"],
        _ => &[],
    }
}

pub fn configure(adapter: &str, key: &str, value: &str) -> Result<()> {
    if !KNOWN_ADAPTERS.contains(&adapter) {
        anyhow::bail!(
            "Unknown adapter '{}'. Known adapters: {}",
            adapter,
            KNOWN_ADAPTERS.join(", ")
        );
    }

    let key_upper = key.to_uppercase();
    let valid_keys = valid_keys_for(adapter);

    if !valid_keys.iter().any(|k| k.eq_ignore_ascii_case(key)) {
        anyhow::bail!(
            "Unknown key '{}'. Valid keys for {}: {}",
            key,
            adapter,
            valid_keys.join(", ")
        );
    }

    let mut config = MoebConfig::load()?;
    let entry = config.adapters.entry(adapter.to_string()).or_insert_with(AdapterConfig::default);

    match key_upper.as_str() {
        "MODEL" => {
            if value.trim().is_empty() {
                anyhow::bail!("MODEL value must not be empty.");
            }
            entry.model = Some(value.to_string());
            config.save()?;
            println!("{} MODEL set to {}.", adapter, value);
        }
        "RETRIES" => {
            let count: u32 = value.trim().parse().map_err(|_| {
                anyhow::anyhow!(
                    "RETRIES requires a non-negative integer, got '{}'. Example: moeb adapter {} config RETRIES 3",
                    value, adapter
                )
            })?;
            entry.retries = Some(count);
            config.save()?;
            println!("{} RETRIES set to {}.", adapter, count);
        }
        "TIMEOUT" => {
            let secs: u64 = value.trim().parse().map_err(|_| {
                anyhow::anyhow!(
                    "TIMEOUT requires a positive integer (seconds), got '{}'. Example: moeb adapter {} config TIMEOUT 600",
                    value, adapter
                )
            })?;
            entry.timeout_secs = Some(secs);
            config.save()?;
            println!("{} TIMEOUT set to {} seconds.", adapter, secs);
        }
        _ => unreachable!("all valid keys are handled above"),
    }

    Ok(())
}

pub fn release(adapter: &str) -> Result<()> {
    if !KNOWN_ADAPTERS.contains(&adapter) {
        anyhow::bail!(
            "Unknown adapter '{}'. Known adapters: {}",
            adapter,
            KNOWN_ADAPTERS.join(", ")
        );
    }

    if let Some(secret_key) = secret_key_for(adapter) {
        let mut secrets = Secrets::load()?;
        secrets.remove(secret_key)?;
    }

    let mut config = MoebConfig::load()?;
    if config.active_adapter.as_deref() == Some(adapter) {
        config.active_adapter = None;
        config.save()?;
    }

    println!("{} credentials removed.", adapter);
    Ok(())
}

#[cfg(test)]
#[path = "adapter_management_tests.rs"]
mod tests;
