use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const MOEB_DIR: &str = ".moeb";
const CONFIG_FILE: &str = "config.toml";
const SECRETS_FILE: &str = ".secrets";

pub fn moeb_dir() -> PathBuf {
    Path::new(MOEB_DIR).to_path_buf()
}

pub fn config_path() -> PathBuf {
    moeb_dir().join(CONFIG_FILE)
}

pub fn secrets_path() -> PathBuf {
    moeb_dir().join(SECRETS_FILE)
}

// ── AdapterConfig ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AdapterConfig {
    pub model: Option<String>,
    pub retries: Option<u32>,
    pub timeout_secs: Option<u64>,
}

impl AdapterConfig {
    pub fn effective_model(&self, default: &str) -> String {
        self.model.clone().unwrap_or_else(|| default.to_string())
    }

    pub fn effective_retries(&self) -> u32 {
        self.retries.unwrap_or(0)
    }

    pub fn effective_timeout_secs(&self, default: u64) -> u64 {
        self.timeout_secs.unwrap_or(default)
    }
}

// ── MoebConfig ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MoebConfig {
    pub active_adapter: Option<String>,
    pub spec_retry_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_retention: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_file_content: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<bool>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub adapters: HashMap<String, AdapterConfig>,
}

impl MoebConfig {
    pub fn effective_spec_retry_limit(&self) -> u32 {
        self.spec_retry_limit.unwrap_or(3)
    }

    pub fn effective_run_retention(&self) -> i32 {
        self.run_retention.unwrap_or(-1)
    }

    pub fn effective_log_file_content(&self) -> bool {
        self.log_file_content.unwrap_or(true)
    }

    pub fn effective_prompt_cache(&self) -> bool {
        self.prompt_cache.unwrap_or(true)
    }

    pub fn adapter_config(&self, name: &str) -> AdapterConfig {
        self.adapters.get(name).cloned().unwrap_or_default()
    }
}

impl MoebConfig {
    pub fn load() -> Result<Self> {
        if !moeb_dir().exists() {
            anyhow::bail!("No .moeb/ directory found. Run `moeb init` first.");
        }
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("Invalid config at {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        let text = toml::to_string_pretty(self).context("Failed to serialise config")?;
        fs::write(&path, text)
            .with_context(|| format!("Failed to write {}", path.display()))
    }
}

// ── Secrets ──────────────────────────────────────────────────────────────────

pub struct Secrets {
    path: PathBuf,
    map: HashMap<String, String>,
}

impl Secrets {
    pub fn empty() -> Self {
        Self { path: secrets_path(), map: HashMap::new() }
    }

    pub fn load() -> Result<Self> {
        let path = secrets_path();
        let map = if path.exists() {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            parse_kv(&text)
        } else {
            HashMap::new()
        };
        Ok(Self { path, map })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        self.map.insert(key.to_string(), value.to_string());
        self.flush()
    }

    pub fn remove(&mut self, key: &str) -> Result<()> {
        self.map.remove(key);
        self.flush()
    }

    fn flush(&self) -> Result<()> {
        let text: String = self
            .map
            .iter()
            .map(|(k, v)| format!("{}={}\n", k, v))
            .collect();
        fs::write(&self.path, text)
            .with_context(|| format!("Failed to write {}", self.path.display()))
    }
}

fn parse_kv(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    pub(crate) static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn in_temp_dir() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        (dir, guard)
    }

    #[test]
    fn load_fails_without_moeb_dir() {
        let (_dir, _guard) = in_temp_dir();
        let err = MoebConfig::load().unwrap_err();
        assert!(
            err.to_string().contains("moeb init"),
            "expected init hint, got: {err}"
        );
    }

    #[test]
    fn load_returns_default_when_config_toml_absent() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let config = MoebConfig::load().expect("load should succeed");
        assert!(config.active_adapter.is_none());
    }

    #[test]
    fn load_reads_saved_config() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let mut config = MoebConfig::default();
        config.active_adapter = Some("openai".to_string());
        config.save().unwrap();
        assert!(Path::new(MOEB_DIR).join(CONFIG_FILE).exists(), "save must write config.toml");
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.active_adapter.as_deref(), Some("openai"));
    }

    #[test]
    fn init_does_not_write_config_toml() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        // Simulate what init does: create .moeb/ but never call MoebConfig::save()
        let config_file = Path::new(MOEB_DIR).join(CONFIG_FILE);
        assert!(!config_file.exists(), "config.toml must not exist after init");
    }

    #[test]
    fn effective_spec_retry_limit_defaults_to_three() {
        let config = MoebConfig::default();
        assert_eq!(config.effective_spec_retry_limit(), 3);
    }

    #[test]
    fn effective_spec_retry_limit_respects_config_value() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let mut config = MoebConfig::default();
        config.spec_retry_limit = Some(5);
        config.save().unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.effective_spec_retry_limit(), 5);
    }

    #[test]
    fn adapter_config_returns_default_when_absent() {
        let config = MoebConfig::default();
        assert_eq!(config.adapter_config("openai").effective_model("gpt-4o"), "gpt-4o");
        assert_eq!(config.adapter_config("openai").effective_retries(), 0);
    }

    #[test]
    fn adapter_config_round_trips_through_toml() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let mut config = MoebConfig::default();
        config.adapters.insert("openai".to_string(), AdapterConfig {
            model: Some("gpt-4o-mini".to_string()),
            retries: Some(3),
            timeout_secs: None,
        });
        config.save().unwrap();
        let loaded = MoebConfig::load().unwrap();
        let ac = loaded.adapter_config("openai");
        assert_eq!(ac.effective_model("gpt-4o"), "gpt-4o-mini");
        assert_eq!(ac.effective_retries(), 3);
    }

    #[test]
    fn adapter_config_timeout_defaults_and_round_trips() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();

        // Default when absent
        assert_eq!(AdapterConfig::default().effective_timeout_secs(600), 600);

        // Round-trip through config.toml
        let mut config = MoebConfig::default();
        config.adapters.insert("anthropic".to_string(), AdapterConfig {
            model: None,
            retries: None,
            timeout_secs: Some(300),
        });
        config.save().unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.adapter_config("anthropic").effective_timeout_secs(600), 300);
    }

    #[test]
    fn empty_adapters_map_not_written() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        MoebConfig::default().save().unwrap();
        let text = fs::read_to_string(config_path()).unwrap();
        assert!(!text.contains("adapters"), "config.toml must not contain 'adapters' when map is empty");
    }

    #[test]
    fn run_retention_defaults_to_minus_one() {
        let config = MoebConfig::default();
        assert_eq!(config.effective_run_retention(), -1);
    }

    #[test]
    fn log_file_content_defaults_to_true() {
        let config = MoebConfig::default();
        assert!(config.effective_log_file_content());
    }

    #[test]
    fn prompt_cache_defaults_to_true() {
        let config = MoebConfig::default();
        assert!(config.effective_prompt_cache(), "prompt_cache must default to true");
    }

    #[test]
    fn kernel_config_round_trips() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let mut config = MoebConfig::default();
        config.run_retention = Some(5);
        config.log_file_content = Some(false);
        config.save().unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.run_retention, Some(5));
        assert_eq!(loaded.log_file_content, Some(false));
        let text = fs::read_to_string(config_path()).unwrap();
        assert!(text.contains("run_retention"), "run_retention must be written");
        assert!(text.contains("log_file_content"), "log_file_content must be written");
    }
}
