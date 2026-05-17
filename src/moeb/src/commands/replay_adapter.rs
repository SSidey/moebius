use anyhow::{Context, Result};
use std::collections::VecDeque;

use crate::adapters::{AgentResponse, Message, ToolDef};
use crate::ports::AiPort;

pub(super) struct ReplayAiAdapter {
    responses: std::sync::Mutex<VecDeque<serde_json::Value>>,
}

impl ReplayAiAdapter {
    pub(super) fn new(responses: VecDeque<serde_json::Value>) -> Self {
        Self { responses: std::sync::Mutex::new(responses) }
    }
}

impl AiPort for ReplayAiAdapter {
    fn send(&self, _messages: &[Message], _tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("ReplayAiAdapter: no more saved responses in trace"))?;
        parse_response_body(&body)
    }
}

fn parse_response_body(body: &serde_json::Value) -> Result<AgentResponse> {
    // Try Anthropic response format first
    if let Some(stop_reason) = body["stop_reason"].as_str() {
        if stop_reason == "tool_use" {
            let content = body["content"]
                .as_array()
                .context("Missing content array in replay response")?;
            let calls: Result<Vec<_>> = content
                .iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .map(|b| {
                    Ok(crate::adapters::ToolCall {
                        id: b["id"].as_str().context("missing id")?.to_string(),
                        name: b["name"].as_str().context("missing name")?.to_string(),
                        arguments: serde_json::to_string(&b["input"])
                            .context("failed to serialize input")?,
                    })
                })
                .collect();
            return Ok(AgentResponse::ToolCalls(calls?));
        }
        let text = body["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|b| b["type"].as_str() == Some("text"))
                    .and_then(|b| b["text"].as_str())
            })
            .unwrap_or("")
            .to_string();
        return Ok(AgentResponse::Text(text));
    }

    // Try OpenAI response format
    if let Some(finish_reason) = body.pointer("/choices/0/finish_reason").and_then(|v| v.as_str()) {
        if finish_reason == "tool_calls" {
            let calls = body
                .pointer("/choices/0/message/tool_calls")
                .and_then(|v| v.as_array())
                .context("Expected tool_calls array")?
                .iter()
                .map(|v| {
                    Ok(crate::adapters::ToolCall {
                        id: v["id"].as_str().context("missing id")?.to_string(),
                        name: v.pointer("/function/name")
                            .and_then(|v| v.as_str())
                            .context("missing function.name")?
                            .to_string(),
                        arguments: v.pointer("/function/arguments")
                            .and_then(|v| v.as_str())
                            .context("missing function.arguments")?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(AgentResponse::ToolCalls(calls));
        }
        let text = body
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(AgentResponse::Text(text));
    }

    // Fallback: treat as text
    Ok(AgentResponse::Text(
        body.as_str().unwrap_or("").to_string(),
    ))
}
