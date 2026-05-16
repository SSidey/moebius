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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_threshold: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_keep_turns: Option<u32>,
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

    pub fn effective_compaction_enabled(&self) -> bool {
        self.compaction_enabled.unwrap_or(true)
    }

    pub fn effective_compaction_threshold(&self) -> usize {
        self.compaction_threshold.unwrap_or(80_000)
    }

    pub fn effective_compaction_keep_turns(&self) -> u32 {
        self.compaction_keep_turns.unwrap_or(3)
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
#[path = "config_tests.rs"]
pub(crate) mod tests;
