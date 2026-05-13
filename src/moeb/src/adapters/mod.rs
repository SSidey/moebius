pub mod anthropic;
pub mod cli;
pub mod embedded_assets;
pub mod openai;
pub mod retry;

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Wire types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    System(String),
    User(String),
    Assistant(String),
    AssistantToolCalls(Vec<ToolCall>),
    ToolResult { call_id: String, content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
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
