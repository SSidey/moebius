use anyhow::Result;

use crate::config::{AdapterConfig, MoebConfig, Secrets};

const KNOWN_ADAPTERS: &[&str] = &["openai", "anthropic"];

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
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use tempfile::TempDir;

    use crate::config::{tests::CWD_LOCK, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
        (dir, guard)
    }

    #[test]
    fn configure_openai_model_updates_config() {
        let (_dir, _guard) = in_temp_dir();
        configure("openai", "MODEL", "gpt-4o-mini").unwrap();
        let config = MoebConfig::load().unwrap();
        assert_eq!(config.adapter_config("openai").model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn configure_openai_retries_updates_config() {
        let (_dir, _guard) = in_temp_dir();
        configure("openai", "RETRIES", "5").unwrap();
        let config = MoebConfig::load().unwrap();
        assert_eq!(config.adapter_config("openai").retries, Some(5));
    }

    #[test]
    fn configure_rejects_invalid_retries() {
        let (_dir, _guard) = in_temp_dir();
        let err = configure("openai", "RETRIES", "abc").unwrap_err();
        assert!(
            err.to_string().contains("integer"),
            "expected 'integer' in error, got: {err}"
        );
    }

    #[test]
    fn configure_rejects_unknown_key() {
        let (_dir, _guard) = in_temp_dir();
        let err = configure("openai", "TIMEOUT", "30").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("TIMEOUT"), "expected key name in error: {msg}");
        assert!(msg.contains("MODEL") || msg.contains("RETRIES"), "expected valid keys in error: {msg}");
    }

    #[test]
    fn release_removes_secret_and_clears_active_adapter() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-test").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.active_adapter = Some("openai".to_string());
        config.save().unwrap();

        release("openai").unwrap();

        let secrets = Secrets::load().unwrap();
        assert!(secrets.get("OPENAI_API_KEY").is_none(), "secret must be absent after release");
        let config = MoebConfig::load().unwrap();
        assert!(config.active_adapter.is_none(), "active_adapter must be None after release");
    }

    #[test]
    fn release_leaves_adapter_config_intact() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-test").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.active_adapter = Some("openai".to_string());
        config.adapters.insert("openai".to_string(), AdapterConfig {
            model: Some("gpt-4o-mini".to_string()),
            retries: Some(2),
            timeout_secs: None,
        });
        config.save().unwrap();

        release("openai").unwrap();

        let config = MoebConfig::load().unwrap();
        let ac = config.adapter_config("openai");
        assert_eq!(ac.model.as_deref(), Some("gpt-4o-mini"), "model must survive release");
        assert_eq!(ac.retries, Some(2), "retries must survive release");
    }

    #[test]
    fn list_adapters_shows_configured_state() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-test").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.active_adapter = Some("openai".to_string());
        config.save().unwrap();

        // Just verify the run() function doesn't error; output goes to stdout
        crate::commands::adapters::run().unwrap();
    }

    #[test]
    fn configure_anthropic_model_updates_config() {
        let (_dir, _guard) = in_temp_dir();
        configure("anthropic", "MODEL", "claude-haiku-4-5").unwrap();
        let config = MoebConfig::load().unwrap();
        assert_eq!(
            config.adapter_config("anthropic").model.as_deref(),
            Some("claude-haiku-4-5")
        );
    }

    #[test]
    fn configure_anthropic_timeout_updates_config() {
        let (_dir, _guard) = in_temp_dir();
        configure("anthropic", "TIMEOUT", "300").unwrap();
        let config = MoebConfig::load().unwrap();
        assert_eq!(config.adapter_config("anthropic").timeout_secs, Some(300));
    }

    #[test]
    fn configure_anthropic_timeout_rejects_invalid_value() {
        let (_dir, _guard) = in_temp_dir();
        let err = configure("anthropic", "TIMEOUT", "fast").unwrap_err();
        assert!(
            err.to_string().contains("seconds"),
            "expected 'seconds' in error, got: {err}"
        );
    }

    #[test]
    fn release_anthropic_removes_secret() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-test").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.active_adapter = Some("anthropic".to_string());
        config.save().unwrap();

        release("anthropic").unwrap();

        let secrets = Secrets::load().unwrap();
        assert!(secrets.get("ANTHROPIC_API_KEY").is_none(), "secret must be absent after release");
        let config = MoebConfig::load().unwrap();
        assert!(config.active_adapter.is_none(), "active_adapter must be None after release");
    }

    #[test]
    fn openai_adapter_uses_configured_model() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("openai".to_string(), AdapterConfig {
            model: Some("gpt-4o-mini".to_string()),
            retries: None,
            timeout_secs: None,
        });
        config.save().unwrap();

        let adapter = crate::adapters::openai::OpenAiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "gpt-4o-mini");
    }
}
