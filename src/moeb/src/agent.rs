use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::adapters::{Message, ToolDef};
use crate::ports::{AiPort, ToolExecutorPort};
use crate::run_state::SharedRunState;
use crate::trace::{FileContentMode, TraceContext};

pub const MAX_TURNS: usize = 50;

pub struct CompactionConfig {
    pub enabled: bool,
    pub threshold: usize,
    pub keep_turns: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { enabled: false, threshold: 80_000, keep_turns: 3 }
    }
}

#[path = "agent_inner.rs"]
mod agent_inner;

// ── Agent loop ────────────────────────────────────────────────────────────────

/// Drive an agent loop until the model produces a plain text response or MAX_TURNS is reached.
pub fn run_agent_loop(
    adapter: &dyn AiPort,
    initial_prompt: &str,
    working_dir: &Path,
) -> Result<String> {
    let state = crate::run_state::new_shared_run_state();
    let tools = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&state)).definitions();
    let messages: Vec<Message> = vec![Message::User(initial_prompt.to_string())];
    let noop_trace = Arc::new(crate::trace::TraceContext::new(crate::trace::TraceConfig {
        command: crate::trace::TraceCommand::Run,
        spec: String::new(),
        adapter: String::new(),
        model: String::new(),
        retention: 0,
        file_content_mode: FileContentMode::Embed,
    }));
    let executor = crate::tools::RealToolExecutor::new(std::sync::Arc::clone(&state));
    agent_inner::run_agent_loop_inner(adapter, &executor, &tools, working_dir, messages, MAX_TURNS, &noop_trace, 1, true, CompactionConfig::default(), state)
}

pub fn run_agent_loop_traced(
    adapter: &dyn AiPort,
    tool_exec: &dyn ToolExecutorPort,
    tools: &[ToolDef],
    working_dir: &Path,
    initial_messages: Vec<Message>,
    max_turns: usize,
    trace: &TraceContext,
    attempt: u32,
    compaction_config: CompactionConfig,
    state: SharedRunState,
) -> Result<String> {
    agent_inner::run_agent_loop_inner(adapter, tool_exec, tools, working_dir, initial_messages, max_turns, trace, attempt, false, compaction_config, state)
}

/// Variant for `moeb run`: continues the loop on text turns until at least one `write_file`
/// has been dispatched, then accepts the next text turn as a completion summary.
pub fn run_agent_loop_run_mode(
    adapter: &dyn AiPort,
    tool_exec: &dyn ToolExecutorPort,
    tools: &[ToolDef],
    working_dir: &Path,
    initial_messages: Vec<Message>,
    max_turns: usize,
    trace: &TraceContext,
    attempt: u32,
    compaction_config: CompactionConfig,
    state: SharedRunState,
) -> Result<String> {
    agent_inner::run_agent_loop_inner(adapter, tool_exec, tools, working_dir, initial_messages, max_turns, trace, attempt, true, compaction_config, state)
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
