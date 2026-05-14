use std::path::Path;
use anyhow::Result;

/// Secondary port — implemented by tool executors (real and replay).
/// The agent loop calls this port; concrete implementations live in `tools/`.
pub trait ToolExecutorPort: Send + Sync {
    fn execute(
        &self,
        name: &str,
        call_id: &str,
        args: &serde_json::Value,
        working_dir: &Path,
        current_turn: u32,
    ) -> Result<(String, bool)>;
}
