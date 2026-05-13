use anyhow::{Context, Result};
use rpassword::prompt_password;

use crate::adapters::anthropic::DEFAULT_TIMEOUT_SECS as ANTHROPIC_DEFAULT_TIMEOUT_SECS;
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

fn credential_key_for(adapter: &str) -> &'static str {
    match adapter {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        _ => unreachable!("caller validates adapter name"),
    }
}

fn adapter_display_name(adapter: &str) -> &str {
    match adapter {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
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
        // existing key retained — nothing to write
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::CWD_LOCK;
    use std::fs;
    use tempfile::TempDir;

    fn setup_moeb_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(".moeb").expect("create .moeb");
        (dir, guard)
    }

    fn seed_secret(key: &str, value: &str) {
        let path = std::path::Path::new(".moeb/.secrets");
        let line = format!("{}={}\n", key, value);
        fs::write(path, line).expect("write secret");
    }

    fn read_secret(key: &str) -> Option<String> {
        let secrets = Secrets::load().expect("load secrets");
        secrets.get(key).map(|s| s.to_string())
    }

    fn read_active_adapter() -> Option<String> {
        MoebConfig::load().ok().and_then(|c| c.active_adapter)
    }

    // ── OpenAI — no existing key ─────────────────────────────────────────────

    #[test]
    fn use_openai_without_existing_key_requires_non_empty_input() {
        let (_dir, _guard) = setup_moeb_dir();
        let err = configure_adapter(
            "openai",
            "OPENAI_API_KEY",
            |_| Ok(String::new()),
            print_openai_config_summary,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn use_openai_without_existing_key_stores_provided_key() {
        let (_dir, _guard) = setup_moeb_dir();
        configure_adapter(
            "openai",
            "OPENAI_API_KEY",
            |_| Ok("sk-new".to_string()),
            print_openai_config_summary,
        )
        .unwrap();
        assert_eq!(read_secret("OPENAI_API_KEY").as_deref(), Some("sk-new"));
        assert_eq!(read_active_adapter().as_deref(), Some("openai"));
    }

    // ── OpenAI — existing key ────────────────────────────────────────────────

    #[test]
    fn use_openai_with_existing_key_empty_input_keeps_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-existing");
        configure_adapter(
            "openai",
            "OPENAI_API_KEY",
            |_| Ok(String::new()),
            print_openai_config_summary,
        )
        .unwrap();
        assert_eq!(
            read_secret("OPENAI_API_KEY").as_deref(),
            Some("sk-existing"),
            "existing key must be preserved"
        );
        assert_eq!(read_active_adapter().as_deref(), Some("openai"));
    }

    #[test]
    fn use_openai_with_existing_key_new_input_replaces_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-old");
        configure_adapter(
            "openai",
            "OPENAI_API_KEY",
            |_| Ok("sk-new".to_string()),
            print_openai_config_summary,
        )
        .unwrap();
        assert_eq!(read_secret("OPENAI_API_KEY").as_deref(), Some("sk-new"));
    }

    #[test]
    fn use_openai_with_existing_key_prompt_mentions_enter_option() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-existing");
        let mut captured_prompt = String::new();
        configure_adapter(
            "openai",
            "OPENAI_API_KEY",
            |prompt| {
                captured_prompt = prompt.to_string();
                Ok(String::new())
            },
            print_openai_config_summary,
        )
        .unwrap();
        assert!(
            captured_prompt.contains("press Enter to keep the existing one"),
            "prompt must mention Enter option; got: {captured_prompt}"
        );
    }

    // ── Anthropic — no existing key ──────────────────────────────────────────

    #[test]
    fn use_anthropic_without_existing_key_requires_non_empty_input() {
        let (_dir, _guard) = setup_moeb_dir();
        let err = configure_adapter(
            "anthropic",
            "ANTHROPIC_API_KEY",
            |_| Ok(String::new()),
            print_anthropic_config_summary,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn use_anthropic_with_existing_key_empty_input_keeps_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("ANTHROPIC_API_KEY", "sk-ant-existing");
        configure_adapter(
            "anthropic",
            "ANTHROPIC_API_KEY",
            |_| Ok(String::new()),
            print_anthropic_config_summary,
        )
        .unwrap();
        assert_eq!(
            read_secret("ANTHROPIC_API_KEY").as_deref(),
            Some("sk-ant-existing"),
            "existing Anthropic key must be preserved"
        );
        assert_eq!(read_active_adapter().as_deref(), Some("anthropic"));
    }
}
