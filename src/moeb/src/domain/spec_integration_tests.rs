    use super::*;
    use crate::adapters::{AgentResponse, Message, ToolCall, ToolDef};
    use crate::ports::AiPort;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn spec_body() -> String {
        [
            "# Token Rotation Spec",
            "",
            "## Raw Requirement",
            "Rotate tokens.",
            "",
            "## Description",
            "Technical approach.",
            "",
            "```mermaid",
            "graph TD",
            "  A --> B",
            "```",
            "",
            "## Backlinks",
            "None.",
            "",
            "## Steps",
            "### Step 1",
            "Do it.",
            "",
            "## Decisions",
            "### Decision 1",
            "Chose X.",
            "",
            "## Rubric",
            "### Structured",
            "Criteria.",
        ]
        .join("\n")
    }

    fn spec_doc(domain: &str, slug: &str) -> String {
        format!("---\ndomain: {}\nslug: {}\nstatus: active\n---\n{}", domain, slug, spec_body())
    }

    struct MockAi {
        responses: Mutex<VecDeque<AgentResponse>>,
    }

    impl MockAi {
        fn new(responses: Vec<AgentResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into_iter().collect()),
            })
        }
    }

    impl AiPort for MockAi {
        fn send(&self, _: &[Message], _: &[ToolDef]) -> anyhow::Result<AgentResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockAi: no more queued responses"))
        }
    }

    fn setup_harness(tmp: &tempfile::TempDir) {
        let dir = tmp.path();
        fs::create_dir_all(dir.join("specifications")).unwrap();
        fs::write(dir.join("README.md"), "# Harness\n").unwrap();
    }

    #[test]
    fn run_in_creates_spec_file_at_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::Text("Registered.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 1, FileContentMode::Embed).unwrap();

        let spec_path = tmp.path().join("specifications/auth/auth.token-rotation.md");
        assert!(spec_path.exists(), "spec file must be created");
        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.starts_with("# Token Rotation Spec"), "body must not contain frontmatter");
    }

    #[test]
    fn run_in_readme_updated_when_agent_writes_it() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let updated_readme = "# Harness\n\n| Token Rotation | Rotate tokens | [specifications/auth/auth.token-rotation.md](specifications/auth/auth.token-rotation.md) |\n";
        let write_args = serde_json::json!({
            "path": "README.md",
            "content": updated_readme,
        })
        .to_string();

        let ai = MockAi::new(vec![
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::ToolCalls(vec![ToolCall {
                id: "r1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "README.md"}).to_string(),
            }]),
            AgentResponse::ToolCalls(vec![ToolCall {
                id: "c1".to_string(),
                name: "write_file".to_string(),
                arguments: write_args,
            }]),
            AgentResponse::Text("Done.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 1, FileContentMode::Embed).unwrap();

        let readme = fs::read_to_string(tmp.path().join("README.md")).unwrap();
        assert!(
            readme.contains("specifications/auth/auth.token-rotation.md"),
            "README must contain the spec link"
        );
    }

    #[test]
    fn run_in_retries_on_validation_failure() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text("No frontmatter here, just prose.".to_string()),
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::Text("Registered.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 2, FileContentMode::Embed).unwrap();

        let spec_path = tmp.path().join("specifications/auth/auth.token-rotation.md");
        assert!(spec_path.exists(), "spec file must be created after retry");
    }

    #[test]
    fn run_in_fails_after_exhausting_retries() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text("No frontmatter — attempt 1.".to_string()),
            AgentResponse::Text("No frontmatter — attempt 2.".to_string()),
        ]);
        let err = SpecService::new(ai)
            .run_in("rotate tokens", tmp.path(), 2, FileContentMode::Embed)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed after 2 attempt(s)"),
            "expected exhaustion message, got: {}",
            msg
        );
    }
