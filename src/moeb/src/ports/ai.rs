use anyhow::Result;

use crate::adapters::{AgentResponse, Message, ToolDef};

/// Secondary port — implemented by AI provider adapters (e.g. OpenAI).
/// The domain calls this port; concrete adapters implement it.
pub trait AiPort: Send + Sync {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse>;
}
