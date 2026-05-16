    use super::*;
    use std::env;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    use crate::adapters::retry;
    use crate::config::{tests::CWD_LOCK, AdapterConfig, MoebConfig, Secrets, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
        (dir, guard)
    }

    #[test]
    fn anthropic_adapter_uses_configured_model() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("anthropic".to_string(), AdapterConfig {
            model: Some("claude-haiku-4-5".to_string()),
            retries: None,
            timeout_secs: None,
        });
        config.save().unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "claude-haiku-4-5");
    }

    #[test]
    fn anthropic_adapter_uses_default_model_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "claude-opus-4-7");
    }

    #[test]
    fn system_message_extracted_to_top_level() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();

        assert_eq!(body["system"], "sys", "system field must be top-level");

        let msgs = body["messages"].as_array().unwrap();
        for m in msgs {
            assert_ne!(
                m["role"].as_str(),
                Some("system"),
                "no system-role entry should appear in messages array"
            );
        }
        assert_eq!(msgs.len(), 1, "only one user message expected");
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn anthropic_adapter_uses_configured_timeout() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("anthropic".to_string(), AdapterConfig {
            model: None,
            retries: None,
            timeout_secs: Some(120),
        });
        config.save().unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 120);
    }

    #[test]
    fn anthropic_adapter_uses_default_timeout_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 600);
    }

    #[test]
    fn anthropic_retry_delay_first_attempt_within_bounds() {
        let delay = retry::compute_delay(0, None);
        assert!(
            delay >= Duration::from_millis(750),
            "delay too short: {:?}",
            delay
        );
        assert!(
            delay <= Duration::from_millis(1250),
            "delay too long: {:?}",
            delay
        );
    }

    #[test]
    fn consecutive_tool_results_are_batched() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::ToolResult { call_id: "c1".to_string(), content: "r1".to_string() },
            Message::ToolResult { call_id: "c2".to_string(), content: "r2".to_string() },
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();

        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1, "two ToolResults must be batched into one user message");
        assert_eq!(msgs[0]["role"], "user");

        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "c1");
        assert_eq!(content[1]["type"], "tool_result");
        assert_eq!(content[1]["tool_use_id"], "c2");
    }

    #[test]
    fn build_request_body_caches_system_when_prompt_cache_true() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], true).unwrap();

        let system = body["system"].as_array()
            .expect("system must be an array when prompt_cache=true");
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["type"], "text");
        assert_eq!(system[0]["text"], "sys");
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn build_request_body_plain_string_system_when_prompt_cache_false() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();
        assert_eq!(body["system"], "sys", "system must be a plain string when prompt_cache=false");
    }
