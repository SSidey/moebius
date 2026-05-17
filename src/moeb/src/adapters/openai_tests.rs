use super::*;
use std::env;
use std::fs;
use tempfile::TempDir;

use crate::config::{tests::CWD_LOCK, AdapterConfig, MoebConfig, Secrets, MOEB_DIR};

fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    env::set_current_dir(dir.path()).expect("set_current_dir");
    fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
    (dir, guard)
}

#[test]
fn openai_adapter_uses_configured_retries() {
    let (_dir, _guard) = in_temp_dir();
    let mut secrets = Secrets::load().unwrap();
    secrets.set("OPENAI_API_KEY", "sk-dummy").unwrap();
    let mut config = MoebConfig::load().unwrap();
    config.adapters.insert("openai".to_string(), AdapterConfig {
        model: None,
        retries: Some(2),
        timeout_secs: None,
    });
    config.save().unwrap();

    let adapter = OpenAiAdapter::from_secrets_and_config().unwrap();
    assert_eq!(adapter.retries, 2);
}

#[test]
fn openai_adapter_uses_default_retries_when_absent() {
    let (_dir, _guard) = in_temp_dir();
    let mut secrets = Secrets::load().unwrap();
    secrets.set("OPENAI_API_KEY", "sk-dummy").unwrap();

    let adapter = OpenAiAdapter::from_secrets_and_config().unwrap();
    assert_eq!(adapter.retries, 0);
}
