pub mod anthropic;
pub mod cli;
pub mod embedded_assets;
pub mod openai;
pub mod retry;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::MoebConfig;
use crate::ports::{AdapterFactoryPort, AiPort};
use crate::trace::TraceContext;

pub struct DefaultAdapterFactory;

impl AdapterFactoryPort for DefaultAdapterFactory {
    fn build(&self, trace: Arc<TraceContext>) -> Result<Arc<dyn AiPort>> {
        let cfg = MoebConfig::load().unwrap_or_default();
        let name = cfg.active_adapter.clone().unwrap_or_default();
        match name.as_str() {
            "openai" => Ok(Arc::new(
                crate::adapters::openai::OpenAiAdapter::from_secrets_and_config_with_trace(trace)?
            )),
            "anthropic" => Ok(Arc::new(
                crate::adapters::anthropic::AnthropicAdapter::from_secrets_and_config_with_trace(trace)?
            )),
            "" => anyhow::bail!("No adapter configured. Run `moeb use <adapter>` first."),
            other => anyhow::bail!(
                "Adapter '{}' is not recognised. Run `moeb use <adapter>` to reconfigure.",
                other
            ),
        }
    }
}

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
