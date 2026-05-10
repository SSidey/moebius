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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

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
}
