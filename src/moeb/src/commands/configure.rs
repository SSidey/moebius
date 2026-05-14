use anyhow::Result;

use crate::config::MoebConfig;

pub fn run_configure(key: &str, value: &str) -> Result<()> {
    match key.to_ascii_uppercase().as_str() {
        "RUN_RETENTION" => {
            let parsed: i32 = value.parse().map_err(|_| {
                anyhow::anyhow!(
                    "RUN_RETENTION requires an integer value (e.g. -1, 0, 10). Got: \"{}\"",
                    value
                )
            })?;
            if parsed < -1 {
                anyhow::bail!(
                    "RUN_RETENTION must be -1 (unlimited), 0 (disabled), or a positive integer."
                );
            }
            let mut cfg = MoebConfig::load()?;
            cfg.run_retention = Some(parsed);
            cfg.save()?;
            println!("RUN_RETENTION set to {}.", parsed);
        }
        "LOG_FILE_CONTENT" => {
            let parsed = match value.to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => anyhow::bail!(
                    "LOG_FILE_CONTENT requires true or false. Got: \"{}\"",
                    value
                ),
            };
            let mut cfg = MoebConfig::load()?;
            cfg.log_file_content = Some(parsed);
            cfg.save()?;
            println!("LOG_FILE_CONTENT set to {}.", parsed);
        }
        "PROMPT_CACHE" => {
            let v = value.to_lowercase();
            let parsed = match v.as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => anyhow::bail!(
                    "Invalid value '{}' for PROMPT_CACHE. Use true or false.",
                    value
                ),
            };
            let mut cfg = MoebConfig::load()?;
            cfg.prompt_cache = Some(parsed);
            cfg.save()?;
            println!("PROMPT_CACHE set to {}", cfg.effective_prompt_cache());
        }
        other => anyhow::bail!(
            "Unknown configuration key \"{}\". Valid keys: RUN_RETENTION, LOG_FILE_CONTENT, PROMPT_CACHE",
            other
        ),
    }
    Ok(())
}

pub fn run_list() -> Result<()> {
    let cfg = MoebConfig::load().unwrap_or_default();
    let retention = cfg.effective_run_retention();
    let log_content = cfg.effective_log_file_content();
    let prompt_cache = cfg.effective_prompt_cache();
    println!(
        "{:<20} {:<8} {:<10} {}",
        "KEY", "VALUE", "DEFAULT", "DESCRIPTION"
    );
    println!(
        "{:<20} {:<8} {:<10} {}",
        "RUN_RETENTION",
        retention,
        "-1",
        "Trace retention per spec (-1=unlimited, 0=disabled, N=keep N)"
    );
    println!(
        "{:<20} {:<8} {:<10} {}",
        "LOG_FILE_CONTENT",
        log_content,
        "true",
        "Embed file content in traces (false=hash-only, disables replay)"
    );
    println!(
        "{:<20} {:<8} {:<10} {}",
        "PROMPT_CACHE",
        prompt_cache,
        "true",
        "Enable Anthropic prompt caching (cache_control on system prompt)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    use crate::config::{tests::CWD_LOCK, MoebConfig, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb");
        (dir, guard)
    }

    #[test]
    fn configure_run_retention_sets_value() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("RUN_RETENTION", "10").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.run_retention, Some(10));
    }

    #[test]
    fn configure_log_file_content_false() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("LOG_FILE_CONTENT", "false").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.log_file_content, Some(false));
    }

    #[test]
    fn configure_prompt_cache_false_round_trips() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("PROMPT_CACHE", "false").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.prompt_cache, Some(false));
        assert!(!loaded.effective_prompt_cache());
    }

    #[test]
    fn configure_rejects_invalid_retention() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("RUN_RETENTION", "abc").unwrap_err();
        assert!(
            err.to_string().contains("integer"),
            "expected 'integer' in error, got: {}",
            err
        );
    }

    #[test]
    fn configure_rejects_retention_below_minus_one() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("RUN_RETENTION", "-5").unwrap_err();
        assert!(
            err.to_string().contains("-1"),
            "expected '-1' in error, got: {}",
            err
        );
    }

    #[test]
    fn configure_rejects_unknown_key() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("TIMEOUT", "30").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Unknown configuration key"),
            "expected 'Unknown configuration key', got: {}",
            msg
        );
        assert!(
            msg.contains("RUN_RETENTION") && msg.contains("LOG_FILE_CONTENT") && msg.contains("PROMPT_CACHE"),
            "expected valid keys listed, got: {}",
            msg
        );
    }
}
