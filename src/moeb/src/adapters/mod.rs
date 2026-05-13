pub mod anthropic;
pub mod cli;
pub mod embedded_assets;
pub mod openai;
pub mod retry;

use anyhow::Result;

// ── Wire types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    System(String),
    User(String),
    /// Plain assistant text reply
    Assistant(String),
    /// Assistant turn that contains only tool calls (no prose content)
    AssistantToolCalls(Vec<ToolCall>),
    /// Result returned to the model after executing a tool call
    ToolResult { call_id: String, content: String },
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON string of the arguments object
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema object for the tool's parameters
    pub parameters: serde_json::Value,
}

#[derive(Debug)]
pub enum AgentResponse {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

// ── Trait ────────────────────────────────────────────────────────────────────

pub trait Adapter: Send + Sync {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse>;
}
