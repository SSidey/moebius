use anyhow::Result;
use std::collections::HashMap;

use crate::ports::ToolExecutorPort;

pub(super) struct ReplayToolExecutor {
    results: HashMap<String, String>,
}

impl ReplayToolExecutor {
    pub(super) fn new(results: HashMap<String, String>) -> Self {
        Self { results }
    }
}

impl ToolExecutorPort for ReplayToolExecutor {
    fn execute(
        &self,
        _name: &str,
        call_id: &str,
        _args: &serde_json::Value,
        _working_dir: &std::path::Path,
        _current_turn: u32,
    ) -> Result<(String, bool)> {
        self.results
            .get(call_id)
            .cloned()
            .map(|r| (r, false))
            .ok_or_else(|| anyhow::anyhow!("ReplayToolExecutor: no saved result for call_id '{}'", call_id))
    }
}
