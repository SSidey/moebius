use anyhow::{Context, Result};
use serde_json::json;
use std::sync::Arc;

use crate::config::MoebConfig;
use crate::ports::AiPort;
use crate::trace::{HttpRequestEvent, HttpRetryEvent, TraceContext, TraceEvent};
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

const API_URL: &str = "http://localhost:11434/v1/chat/completions";
const DEFAULT_MODEL: &str = "llama3.1";

pub struct OllamaAdapter {
    pub model: String,
    pub retries: u32,
    pub base_url: String,
    client: reqwest::blocking::Client,
    trace: Arc<TraceContext>,
}

impl OllamaAdapter {
    pub fn from_secrets_and_config() -> Result<Self> {
        let noop_trace = Arc::new(TraceContext::new(crate::trace::TraceConfig {
            command: crate::trace::TraceCommand::Run,
            spec: String::new(),
            adapter: String::new(),
            model: String::new(),
            retention: 0,
            file_content_mode: crate::trace::FileContentMode::Embed,
        }));
        Self::from_secrets_and_config_with_trace(noop_trace)
    }

    pub fn from_secrets_and_config_with_trace(trace: Arc<TraceContext>) -> Result<Self> {
        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config("ollama");
        Ok(Self {
            model: adapter_cfg.effective_model(DEFAULT_MODEL),
            retries: adapter_cfg.effective_retries(),
            base_url: API_URL.to_string(),
            client: reqwest::blocking::Client::new(),
            trace,
        })
    }
}

impl AiPort for OllamaAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        Adapter::send(self, messages, tools)
    }
}

impl Adapter for OllamaAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = json!({
            "model": self.model,
            "messages": messages.iter().map(to_openai_message).collect::<Vec<_>>(),
            "tools": tools.iter().map(to_openai_tool).collect::<Vec<_>>(),
        });

        let attempt = self.trace.current_attempt.load(std::sync::atomic::Ordering::SeqCst);
        let turn = self.trace.current_turn.load(std::sync::atomic::Ordering::SeqCst);
        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for http_attempt in 0..max_attempts {
            let start = std::time::Instant::now();
            let response = match self.client.post(&self.base_url).json(&body).send() {
                Err(e) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    let reason = format!("transport error: {}", e);
                    let delay = retry::compute_delay(http_attempt, None);
                    if http_attempt + 1 < max_attempts {
                        self.trace.push(TraceEvent::HttpRetry(HttpRetryEvent {
                            attempt,
                            turn,
                            http_attempt: http_attempt + 1,
                            delay_ms: delay.as_millis() as u64,
                            reason: reason.clone(),
                        }));
                        std::thread::sleep(delay);
                    }
                    self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                        attempt,
                        turn,
                        http_attempt: http_attempt + 1,
                        url: self.base_url.clone(),
                        request_body: body.clone(),
                        response_status: 0,
                        response_headers: json!({}),
                        response_body: json!({"error": reason}),
                        duration_ms,
                    }));
                    last_err = Some(anyhow::anyhow!("Failed to reach Ollama API: {}", e));
                    continue;
                }
                Ok(r) => r,
            };

            let duration_ms = start.elapsed().as_millis() as u64;
            let status = response.status();
            let status_u16 = status.as_u16();
            let retry_after = if status_u16 == 429 {
                retry::parse_retry_after(response.headers())
            } else {
                None
            };
            let response_headers = headers_to_json(response.headers());
            let text = response.text().context("Failed to read Ollama response body")?;
            let response_body: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|_| serde_json::Value::String(text.clone()));

            self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                attempt,
                turn,
                http_attempt: http_attempt + 1,
                url: self.base_url.clone(),
                request_body: body.clone(),
                response_status: status_u16,
                response_headers,
                response_body: response_body.clone(),
                duration_ms,
            }));

            if status_u16 == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("Ollama API error {}: {}", status, text));
                if http_attempt + 1 < max_attempts {
                    let delay = retry::compute_delay(http_attempt, retry_after);
                    self.trace.push(TraceEvent::HttpRetry(HttpRetryEvent {
                        attempt,
                        turn,
                        http_attempt: http_attempt + 1,
                        delay_ms: delay.as_millis() as u64,
                        reason: format!("HTTP {} {}", status_u16, status.canonical_reason().unwrap_or("Server Error")),
                    }));
                    std::thread::sleep(delay);
                }
                continue;
            }

            if !status.is_success() {
                anyhow::bail!("Ollama API error {}: {}", status, text);
            }

            return parse_response(&response_body);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Ollama API request failed")))
    }
}

fn headers_to_json(headers: &reqwest::header::HeaderMap) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            map.insert(name.to_string(), serde_json::Value::String(v.to_string()));
        }
    }
    serde_json::Value::Object(map)
}

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

fn parse_response(value: &serde_json::Value) -> Result<AgentResponse> {
    let message = value
        .pointer("/choices/0/message")
        .context("Missing choices[0].message in Ollama response")?;
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

    let content = message["content"].as_str().unwrap_or("").to_string();
    Ok(AgentResponse::Text(content))
}

fn parse_tool_call(value: &serde_json::Value) -> Result<ToolCall> {
    Ok(ToolCall {
        id: value["id"].as_str().context("Tool call missing id")?.to_string(),
        name: value.pointer("/function/name").and_then(|v| v.as_str()).context("Tool call missing function.name")?.to_string(),
        arguments: value.pointer("/function/arguments").and_then(|v| v.as_str()).context("Tool call missing function.arguments")?.to_string(),
    })
}
