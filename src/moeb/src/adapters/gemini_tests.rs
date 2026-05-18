    use super::*;
    use std::env;
    use std::fs;
    use tempfile::TempDir;

    use crate::adapters::{Message, ToolCall};
    use crate::config::{tests::CWD_LOCK, AdapterConfig, MoebConfig, Secrets, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
        (dir, guard)
    }

    #[test]
    fn gemini_adapter_uses_configured_model() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("GEMINI_API_KEY", "dummy-key").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("gemini".to_string(), AdapterConfig {
            model: Some("gemini-1.5-pro".to_string()),
            retries: None,
            timeout_secs: None,
        });
        config.save().unwrap();

        let adapter = GeminiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "gemini-1.5-pro");
    }

    #[test]
    fn gemini_adapter_uses_default_model_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("GEMINI_API_KEY", "dummy-key").unwrap();

        let adapter = GeminiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "gemini-2.0-flash");
    }

    #[test]
    fn gemini_adapter_uses_configured_timeout() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("GEMINI_API_KEY", "dummy-key").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("gemini".to_string(), AdapterConfig {
            model: None,
            retries: None,
            timeout_secs: Some(120),
        });
        config.save().unwrap();

        let adapter = GeminiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 120);
    }

    #[test]
    fn gemini_adapter_uses_default_timeout_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("GEMINI_API_KEY", "dummy-key").unwrap();

        let adapter = GeminiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 600);
    }

    #[test]
    fn system_message_becomes_system_instruction() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("be helpful".to_string()),
            Message::User("hello".to_string()),
        ];
        let body = build_request_body(&messages, &[]);

        assert!(body.get("system_instruction").is_some(), "system_instruction must be present");
        assert_eq!(body["system_instruction"]["parts"][0]["text"], "be helpful");

        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1, "only the user turn should be in contents");
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn parse_response_returns_text() {
        let response = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": "hello there"}]
                },
                "finishReason": "STOP"
            }]
        });

        let result = gemini_io::parse_response(&response).unwrap();
        match result {
            AgentResponse::Text(t) => assert_eq!(t, "hello there"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_response_returns_tool_calls() {
        let response = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "read_file",
                            "args": {"path": "src/main.rs"}
                        }
                    }]
                },
                "finishReason": "STOP"
            }]
        });

        let result = gemini_io::parse_response(&response).unwrap();
        match result {
            AgentResponse::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "gemini_call_0");
                assert_eq!(calls[0].name, "read_file");
                let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
                assert_eq!(args["path"], "src/main.rs");
            }
            other => panic!("expected ToolCalls, got {:?}", other),
        }
    }

    #[test]
    fn consecutive_tool_results_batched_into_one_user_turn() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::AssistantToolCalls(vec![
                ToolCall { id: "gemini_call_0".to_string(), name: "tool_a".to_string(), arguments: "{}".to_string() },
                ToolCall { id: "gemini_call_1".to_string(), name: "tool_b".to_string(), arguments: "{}".to_string() },
            ]),
            Message::ToolResult { call_id: "gemini_call_0".to_string(), content: "result_a".to_string() },
            Message::ToolResult { call_id: "gemini_call_1".to_string(), content: "result_b".to_string() },
        ];
        let body = build_request_body(&messages, &[]);
        let contents = body["contents"].as_array().unwrap();

        assert_eq!(contents.len(), 2, "model turn + one batched user turn");
        assert_eq!(contents[0]["role"], "model");
        assert_eq!(contents[1]["role"], "user");

        let parts = contents[1]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionResponse"]["name"], "tool_a");
        assert_eq!(parts[1]["functionResponse"]["name"], "tool_b");
    }

    #[test]
    fn tool_result_uses_function_name_from_preceding_tool_calls() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::AssistantToolCalls(vec![
                ToolCall { id: "gemini_call_0".to_string(), name: "write_file".to_string(), arguments: "{}".to_string() },
            ]),
            Message::ToolResult { call_id: "gemini_call_0".to_string(), content: "ok".to_string() },
        ];
        let body = build_request_body(&messages, &[]);
        let contents = body["contents"].as_array().unwrap();

        let parts = contents[1]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["functionResponse"]["name"], "write_file",
            "functionResponse name must match the original tool call name");
    }
