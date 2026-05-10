use anyhow::{Context, Result};
use serde_json::json;

use crate::config::Secrets;
use super::{Adapter, AgentResponse, Message, ToolCall, ToolDef};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const MODEL: &str = "gpt-4o";

pub struct OpenAiAdapter {
    api_key: String,
    client: reqwest::blocking::Client,
}

impl OpenAiAdapter {
    pub fn from_secrets() -> Result<Self> {
        let secrets = Secrets::load()?;
        let api_key = secrets
            .get("OPENAI_API_KEY")
            .context("OPENAI_API_KEY not set. Run `moeb use openai` first.")?
            .to_string();
        Ok(Self {
            api_key,
            client: reqwest::blocking::Client::new(),
        })
    }
}

impl Adapter for OpenAiAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = json!({
            "model": MODEL,
            "messages": messages.iter().map(to_openai_message).collect::<Vec<_>>(),
            "tools": tools.iter().map(to_openai_tool).collect::<Vec<_>>(),
        });

        let response = self
            .client
            .post(API_URL)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("Failed to reach OpenAI API")?;

        let status = response.status();
        let text = response.text().context("Failed to read OpenAI response body")?;

        if !status.is_success() {
            anyhow::bail!("OpenAI API error {}: {}", status, text);
        }

        let value: serde_json::Value =
            serde_json::from_str(&text).context("Failed to parse OpenAI response JSON")?;

        parse_response(&value)
    }
}

// ── Serialisation helpers ─────────────────────────────────────────────────────

fn to_openai_message(msg: &Message) -> serde_json::Value {
    match msg {
        Message::System(content) => json!({ "role": "system", "content": content }),
        Message::User(content) => json!({ "role": "user", "content": content }),
        Message::Assistant(content) => json!({ "role": "assistant", "content": content }),
        Message::AssistantToolCalls(calls) => json!({
            "role": "assistant",
            "content": null,
            "tool_calls": calls.iter().map(|c| json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })).collect::<Vec<_>>()
        }),
        Message::ToolResult { call_id, content } => json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content
        }),
    }
}

fn to_openai_tool(def: &ToolDef) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": def.name,
            "description": def.description,
            "parameters": def.parameters
        }
    })
}

// ── Response parsing ──────────────────────────────────────────────────────────

fn parse_response(value: &serde_json::Value) -> Result<AgentResponse> {
    let message = value
        .pointer("/choices/0/message")
        .context("Missing choices[0].message in OpenAI response")?;

    let finish_reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if finish_reason == "tool_calls" {
        let calls = message["tool_calls"]
            .as_array()
            .context("Expected tool_calls array in assistant message")?
            .iter()
            .map(parse_tool_call)
            .collect::<Result<Vec<_>>>()?;
        return Ok(AgentResponse::ToolCalls(calls));
    }

    let content = message["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    Ok(AgentResponse::Text(content))
}

fn parse_tool_call(value: &serde_json::Value) -> Result<ToolCall> {
    Ok(ToolCall {
        id: value["id"]
            .as_str()
            .context("Tool call missing id")?
            .to_string(),
        name: value
            .pointer("/function/name")
            .and_then(|v| v.as_str())
            .context("Tool call missing function.name")?
            .to_string(),
        arguments: value
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .context("Tool call missing function.arguments")?
            .to_string(),
    })
}
