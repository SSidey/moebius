use anyhow::{Context, Result};
use serde_json::json;

use super::{AgentResponse, Message, ToolCall, ToolDef};

pub(super) fn to_openai_message(msg: &Message) -> serde_json::Value {
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

pub(super) fn to_openai_tool(def: &ToolDef) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": def.name,
            "description": def.description,
            "parameters": def.parameters
        }
    })
}

pub(super) fn parse_response(value: &serde_json::Value) -> Result<AgentResponse> {
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
