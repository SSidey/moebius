use std::path::Path;
use std::sync::Arc;
use anyhow::{Context, Result};
use serde_json::json;

use crate::adapters::ToolDef;
use crate::ports::AiPort;
use super::ToolHandler;

pub const MAX_SUB_AGENT_TURNS: usize = 20;

pub struct SpawnAgentTool {
    pub adapter: Arc<dyn AiPort>,
}

impl ToolHandler for SpawnAgentTool {
    fn name(&self) -> &'static str { "spawn_agent" }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "spawn_agent",
            description: "Spawn a sub-agent to analyze files and return a unified diff. \
                The sub-agent can read files and use task-list tools but cannot write, \
                patch, or spawn further agents. Use this to delegate independent \
                analysis tasks. spawn_agent is synchronous — it blocks until the \
                sub-agent returns its text response.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Precise instruction for the sub-agent: which files \
                            to read and what unified diff to produce."
                    },
                    "context": {
                        "type": "string",
                        "description": "Relevant specification steps, file paths, and \
                            architectural constraints the sub-agent needs to complete \
                            its analysis."
                    }
                },
                "required": ["task", "context"]
            }),
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
        let task = args["task"].as_str().context("spawn_agent: missing 'task'")?;
        let context = args["context"].as_str().context("spawn_agent: missing 'context'")?;

        let moeb_dir = working_dir.join(".moeb");
        let sub_skill = crate::skills::load_skill(&moeb_dir, "sub-agent");
        let role_name = "run";
        let role_content = crate::skills::load_role(&moeb_dir, role_name);

        let prompt = format!(
            "{}\n\n=== Task ===\n{}\n\n=== Context ===\n{}\n\n{}",
            role_content, task, context, sub_skill
        );

        let state = crate::run_state::new_shared_run_state();
        let registry = super::ToolRegistry::sub_agent(Arc::clone(&state));
        let tool_defs = registry.definitions();
        let executor = super::RealToolExecutor::new_sub_agent(Arc::clone(&state));
        let noop_trace = Arc::new(crate::trace::TraceContext::new(crate::trace::TraceConfig {
            command: crate::trace::TraceCommand::Run,
            spec: String::new(),
            adapter: String::new(),
            model: String::new(),
            retention: 0,
            file_content_mode: crate::trace::FileContentMode::Embed,
        }));

        let initial_messages = vec![crate::adapters::Message::User(prompt)];
        let result = crate::agent::run_agent_loop_traced(
            self.adapter.as_ref(),
            &executor,
            &tool_defs,
            working_dir,
            initial_messages,
            MAX_SUB_AGENT_TURNS,
            &noop_trace,
            1,
            crate::agent::CompactionConfig::default(),
            state,
        )?;

        if result.is_empty() {
            return Ok(
                "[spawn_agent] Sub-agent returned no output. \
                 The sub-agent may have reached its turn limit without completing analysis. \
                 Perform this task directly.".to_string()
            );
        }

        Ok(result)
    }
}
