use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::{Message, ToolDef};
use super::MAX_TOKENS;

pub(crate) fn build_request_body(
    model: &str,
    messages: &[Message],
    tools: &[ToolDef],
    prompt_cache: bool,
) -> Result<Value> {
    let mut system_prompt: Option<String> = None;
    let non_system: Vec<&Message> = messages
        .iter()
        .filter(|m| {
            if let Message::System(content) = m {
                if system_prompt.is_none() {
                    system_prompt = Some(content.clone());
                }
                false
            } else {
                true
            }
        })
        .collect();

    let anthropic_messages = build_messages(&non_system)?;

    let tools_json: Vec<Value> = tools
        .iter()
        .map(|def| json!({
            "name": def.name,
            "description": def.description,
            "input_schema": def.parameters,
        }))
        .collect();

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model));
    body.insert("max_tokens".into(), json!(MAX_TOKENS));
    if let Some(sys) = system_prompt {
        if prompt_cache {
            body.insert("system".into(), json!([{
                "type": "text",
                "text": sys,
                "cache_control": {"type": "ephemeral"}
            }]));
        } else {
            body.insert("system".into(), json!(sys));
        }
    }
    body.insert("messages".into(), json!(anthropic_messages));
    if !tools_json.is_empty() {
        body.insert("tools".into(), json!(tools_json));
    }

    Ok(Value::Object(body))
}

pub(crate) fn build_messages(messages: &[&Message]) -> Result<Vec<Value>> {
    let mut result: Vec<Value> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        if let Message::ToolResult { .. } = messages[i] {
            let mut tool_results: Vec<Value> = Vec::new();
            while i < messages.len() {
                if let Message::ToolResult { call_id, content } = messages[i] {
                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": content,
                    }));
                    i += 1;
                } else {
                    break;
                }
            }
            result.push(json!({
                "role": "user",
                "content": tool_results,
            }));
        } else {
            result.push(to_anthropic_message(messages[i])?);
            i += 1;
        }
    }

    Ok(result)
}

fn to_anthropic_message(msg: &Message) -> Result<Value> {
    match msg {
        Message::System(_) => unreachable!("System messages are filtered before build_messages"),
        Message::User(content) => Ok(json!({
            "role": "user",
            "content": content,
        })),
        Message::Assistant(content) => Ok(json!({
            "role": "assistant",
            "content": [{"type": "text", "text": content}],
        })),
        Message::AssistantToolCalls(calls) => {
            let blocks: Result<Vec<Value>> = calls
                .iter()
                .map(|call| {
                    let input: Value = serde_json::from_str(&call.arguments)
                        .with_context(|| format!("Failed to parse tool arguments for '{}'", call.name))?;
                    Ok(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": input,
                    }))
                })
                .collect();
            Ok(json!({
                "role": "assistant",
                "content": blocks?,
            }))
        }
        Message::ToolResult { .. } => unreachable!("ToolResult handled in batch loop"),
    }
}
