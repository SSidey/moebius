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

    #[test]
    fn use_openai_without_existing_key_requires_non_empty_input() {
        let (_dir, _guard) = setup_moeb_dir();
        let err = configure_adapter("openai", "OPENAI_API_KEY", |_| Ok(String::new()), print_openai_config_summary).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "unexpected error: {err}");
    }

    #[test]
    fn use_openai_without_existing_key_stores_provided_key() {
        let (_dir, _guard) = setup_moeb_dir();
        configure_adapter("openai", "OPENAI_API_KEY", |_| Ok("sk-new".to_string()), print_openai_config_summary).unwrap();
        assert_eq!(read_secret("OPENAI_API_KEY").as_deref(), Some("sk-new"));
        assert_eq!(read_active_adapter().as_deref(), Some("openai"));
    }

    #[test]
    fn use_openai_with_existing_key_empty_input_keeps_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-existing");
        configure_adapter("openai", "OPENAI_API_KEY", |_| Ok(String::new()), print_openai_config_summary).unwrap();
        assert_eq!(read_secret("OPENAI_API_KEY").as_deref(), Some("sk-existing"));
        assert_eq!(read_active_adapter().as_deref(), Some("openai"));
    }

    #[test]
    fn use_openai_with_existing_key_new_input_replaces_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-old");
        configure_adapter("openai", "OPENAI_API_KEY", |_| Ok("sk-new".to_string()), print_openai_config_summary).unwrap();
        assert_eq!(read_secret("OPENAI_API_KEY").as_deref(), Some("sk-new"));
    }

    #[test]
    fn use_openai_with_existing_key_prompt_mentions_enter_option() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("OPENAI_API_KEY", "sk-existing");
        let mut captured_prompt = String::new();
        configure_adapter("openai", "OPENAI_API_KEY", |prompt| { captured_prompt = prompt.to_string(); Ok(String::new()) }, print_openai_config_summary).unwrap();
        assert!(captured_prompt.contains("press Enter to keep the existing one"), "prompt must mention Enter option; got: {captured_prompt}");
    }

    #[test]
    fn use_anthropic_without_existing_key_requires_non_empty_input() {
        let (_dir, _guard) = setup_moeb_dir();
        let err = configure_adapter("anthropic", "ANTHROPIC_API_KEY", |_| Ok(String::new()), print_anthropic_config_summary).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "unexpected error: {err}");
    }

    #[test]
    fn use_anthropic_with_existing_key_empty_input_keeps_key() {
        let (_dir, _guard) = setup_moeb_dir();
        seed_secret("ANTHROPIC_API_KEY", "sk-ant-existing");
        configure_adapter("anthropic", "ANTHROPIC_API_KEY", |_| Ok(String::new()), print_anthropic_config_summary).unwrap();
        assert_eq!(read_secret("ANTHROPIC_API_KEY").as_deref(), Some("sk-ant-existing"));
        assert_eq!(read_active_adapter().as_deref(), Some("anthropic"));
    }

    #[test]
    fn use_ollama_sets_active_adapter_without_secret() {
        let (_dir, _guard) = setup_moeb_dir();
        configure_ollama().unwrap();
        assert_eq!(read_active_adapter().as_deref(), Some("ollama"));
    }
