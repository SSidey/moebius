use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const MOEB_DIR: &str = ".moeb";
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

// ── MoebConfig ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MoebConfig {
    pub active_adapter: Option<String>,
}

impl MoebConfig {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            anyhow::bail!(
                "No .moeb/ directory found. Run `moeb init` first."
            );
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
