use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::{AgentResponse, Message, ToolCall, ToolDef};

pub(crate) fn build_request_body(messages: &[Message], tools: &[ToolDef]) -> Value {
    let (system_instruction, contents) = build_contents(messages);
    let mut body = serde_json::Map::new();
    if let Some(sys) = system_instruction {
        body.insert("system_instruction".into(), json!({ "parts": [{ "text": sys }] }));
    }
    body.insert("contents".into(), json!(contents));
    if !tools.is_empty() {
        let func_decls: Vec<Value> = tools.iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        })).collect();
        body.insert("tools".into(), json!([{ "function_declarations": func_decls }]));
        body.insert("tool_config".into(), json!({
            "function_calling_config": { "mode": "AUTO" }
        }));
    }
    body.insert("generationConfig".into(), json!({ "maxOutputTokens": 8192 }));
    Value::Object(body)
}

pub(crate) fn build_contents(messages: &[Message]) -> (Option<String>, Vec<Value>) {
    let mut system_instruction: Option<String> = None;
    let mut call_id_to_name: HashMap<String, String> = HashMap::new();
    let mut contents: Vec<Value> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        match &messages[i] {
            Message::System(content) => {
                system_instruction = Some(content.clone());
                i += 1;
            }
            Message::User(content) => {
                contents.push(json!({ "role": "user", "parts": [{"text": content}] }));
                i += 1;
            }
            Message::Assistant(content) => {
                contents.push(json!({ "role": "model", "parts": [{"text": content}] }));
                i += 1;
            }
            Message::AssistantToolCalls(calls) => {
                for c in calls {
                    call_id_to_name.insert(c.id.clone(), c.name.clone());
                }
                let parts: Vec<Value> = calls.iter().map(|c| {
                    let args: Value = serde_json::from_str(&c.arguments).unwrap_or(json!({}));
                    json!({ "functionCall": { "name": c.name, "args": args } })
                }).collect();
                contents.push(json!({ "role": "model", "parts": parts }));
                i += 1;
            }
            Message::ToolResult { .. } => {
                let mut parts: Vec<Value> = Vec::new();
                while i < messages.len() {
                    if let Message::ToolResult { call_id, content } = &messages[i] {
                        let name = call_id_to_name
                            .get(call_id)
                            .cloned()
                            .unwrap_or_else(|| call_id.clone());
                        parts.push(json!({
                            "functionResponse": {
                                "name": name,
                                "response": { "content": content }
                            }
                        }));
                        i += 1;
                    } else {
                        break;
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }

    (system_instruction, contents)
}

pub(super) fn parse_response(value: &Value) -> Result<AgentResponse> {
    let parts = value
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
        .context("Missing candidates[0].content.parts in Gemini response")?;

    let func_call_parts: Vec<&Value> = parts
        .iter()
        .filter(|p| p.get("functionCall").is_some())
        .collect();

    if !func_call_parts.is_empty() {
        let calls: Result<Vec<ToolCall>> = func_call_parts
            .iter()
            .enumerate()
            .map(|(i, p)| parse_tool_call(i, &p["functionCall"]))
            .collect();
        return Ok(AgentResponse::ToolCalls(calls?));
    }

    let text = parts
        .iter()
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    Ok(AgentResponse::Text(text))
}

fn parse_tool_call(index: usize, fc: &Value) -> Result<ToolCall> {
    let name = fc["name"]
        .as_str()
        .context("functionCall missing name")?
        .to_string();
    let arguments = serde_json::to_string(&fc["args"])
        .context("Failed to serialise functionCall args")?;
    Ok(ToolCall {
        id: format!("gemini_call_{}", index),
        name,
        arguments,
    })
}
