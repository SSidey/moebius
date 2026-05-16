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

        assert_eq!(AdapterConfig::default().effective_timeout_secs(600), 600);

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

    #[test]
    fn compaction_defaults_and_round_trip() {
        let (_dir, _guard) = in_temp_dir();
        fs::create_dir_all(MOEB_DIR).unwrap();
        let mut config = MoebConfig::default();
        config.compaction_enabled = Some(false);
        config.compaction_threshold = Some(12_345);
        config.compaction_keep_turns = Some(7);
        config.save().unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert!(!loaded.effective_compaction_enabled());
        assert_eq!(loaded.effective_compaction_threshold(), 12_345);
        assert_eq!(loaded.effective_compaction_keep_turns(), 7);
    }
