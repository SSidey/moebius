    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use crate::adapters::{AgentResponse, Message, ToolCall, ToolDef};
    use crate::config::tests::CWD_LOCK;
    use tempfile::TempDir;

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_current_dir(dir.path()).expect("set_current_dir");
        (dir, guard)
    }

    fn noop_trace() -> crate::trace::TraceContext {
        crate::trace::TraceContext::new(crate::trace::TraceConfig {
            command: crate::trace::TraceCommand::Run,
            spec: String::new(),
            adapter: String::new(),
            model: String::new(),
            retention: 0,
            file_content_mode: FileContentMode::Embed,
        })
    }

    struct SequenceAdapter {
        responses: Mutex<VecDeque<AgentResponse>>,
    }

    impl SequenceAdapter {
        fn new(responses: Vec<AgentResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    impl AiPort for SequenceAdapter {
        fn send(&self, _: &[Message], _: &[ToolDef]) -> anyhow::Result<AgentResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("SequenceAdapter: no more queued responses"))
        }
    }

    struct AcceptAllExecutor;

    impl ToolExecutorPort for AcceptAllExecutor {
        fn execute(
            &self,
            name: &str,
            _call_id: &str,
            args: &serde_json::Value,
            _working_dir: &std::path::Path,
            _current_turn: u32,
        ) -> anyhow::Result<(String, bool)> {
            if name == "write_file" {
                let path = args["path"].as_str().unwrap_or("unknown");
                Ok((format!("Wrote 7 bytes to {}", path), false))
            } else {
                Ok(("ok".to_string(), false))
            }
        }
    }

    fn write_file_call(path: &str, content: &str) -> ToolCall {
        ToolCall {
            id: "c1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path": path, "content": content}).to_string(),
        }
    }

    fn run_stub(adapter: SequenceAdapter) -> anyhow::Result<String> {
        let trace = noop_trace();
        run_agent_loop_run_mode(
            &adapter,
            &AcceptAllExecutor,
            &[],
            std::path::Path::new("."),
            vec![Message::User("prompt".into())],
            50,
            &trace,
            1,
            CompactionConfig::default(),
        )
    }

    #[test]
    fn text_turn_without_write_does_not_terminate_loop() {
        let (_dir, _guard) = in_temp_dir();
        let adapter = SequenceAdapter::new(vec![
            AgentResponse::Text("let me start".into()),
            AgentResponse::ToolCalls(vec![write_file_call("src/x.rs", "content")]),
            AgentResponse::Text("Done.".into()),
        ]);
        assert_eq!(run_stub(adapter).unwrap(), "Done.");
    }

    #[test]
    fn three_consecutive_text_turns_terminates_with_warning() {
        let (_dir, _guard) = in_temp_dir();
        let adapter = SequenceAdapter::new(vec![
            AgentResponse::Text("thinking\u{2026}".into()),
            AgentResponse::Text("thinking\u{2026}".into()),
            AgentResponse::Text("thinking\u{2026}".into()),
        ]);
        assert!(run_stub(adapter).is_ok());
    }

    #[test]
    fn text_turn_after_write_terminates_immediately() {
        let (_dir, _guard) = in_temp_dir();
        // Exactly two responses queued; a third adapter call would return Err.
        let adapter = SequenceAdapter::new(vec![
            AgentResponse::ToolCalls(vec![write_file_call("src/y.rs", "y")]),
            AgentResponse::Text("Implementation complete.".into()),
        ]);
        assert_eq!(run_stub(adapter).unwrap(), "Implementation complete.");
    }

    #[test]
    fn consecutive_text_counter_resets_on_tool_call() {
        let (_dir, _guard) = in_temp_dir();
        let adapter = SequenceAdapter::new(vec![
            AgentResponse::Text("planning".into()),           // counter = 1
            AgentResponse::Text("planning".into()),           // counter = 2
            AgentResponse::ToolCalls(vec![write_file_call("src/z.rs", "z")]), // counter resets to 0
            AgentResponse::Text("planning again".into()),     // counter = 1, write_file_dispatched → clean exit
        ]);
        // Must exit cleanly at turn 4 without the three-turn warning.
        assert_eq!(run_stub(adapter).unwrap(), "planning again");
    }
