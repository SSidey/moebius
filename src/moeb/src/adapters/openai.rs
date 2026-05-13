use anyhow::{Context, Result};
use serde_json::json;

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o";

pub struct OpenAiAdapter {
    api_key: String,
    pub model: String,
    pub retries: u32,
    client: reqwest::blocking::Client,
}

impl OpenAiAdapter {
    pub fn from_secrets_and_config() -> Result<Self> {
        let secrets = Secrets::load()?;
        let api_key = secrets
            .get("OPENAI_API_KEY")
            .context("OPENAI_API_KEY not set. Run `moeb use openai` first.")?
            .to_string();
        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config("openai");
        Ok(Self {
            api_key,
            model: adapter_cfg.effective_model(DEFAULT_MODEL),
            retries: adapter_cfg.effective_retries(),
            client: reqwest::blocking::Client::new(),
        })
    }
}

impl AiPort for OpenAiAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        Adapter::send(self, messages, tools)
    }
}

impl Adapter for OpenAiAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = json!({
            "model": self.model,
            "messages": messages.iter().map(to_openai_message).collect::<Vec<_>>(),
            "tools": tools.iter().map(to_openai_tool).collect::<Vec<_>>(),
        });

        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..max_attempts {
            let response = match self
                .client
                .post(API_URL)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
            {
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("Failed to reach OpenAI API: {}", e));
                    if attempt + 1 < max_attempts {
                        std::thread::sleep(retry::compute_delay(attempt, None));
                    }
                    continue;
                }
                Ok(r) => r,
            };

            let status = response.status();
            let retry_after = if status.as_u16() == 429 {
                retry::parse_retry_after(response.headers())
            } else {
                None
            };
            if status.is_success() {
                retry::warn_if_quota_low(response.headers(), "x-ratelimit-remaining-requests");
            }
            let text = response.text().context("Failed to read OpenAI response body")?;

            if status.as_u16() == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("OpenAI API error {}: {}", status, text));
                if attempt + 1 < max_attempts {
                    std::thread::sleep(retry::compute_delay(attempt, retry_after));
                }
                continue;
            }

            if !status.is_success() {
                anyhow::bail!("OpenAI API error {}: {}", status, text);
            }

            let value: serde_json::Value =
                serde_json::from_str(&text).context("Failed to parse OpenAI response JSON")?;

            return parse_response(&value);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("OpenAI API request failed")))
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use tempfile::TempDir;

    use crate::config::{tests::CWD_LOCK, AdapterConfig, MoebConfig, Secrets, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
        (dir, guard)
    }

    #[test]
    fn openai_adapter_uses_configured_retries() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("openai".to_string(), AdapterConfig {
            model: None,
            retries: Some(2),
            timeout_secs: None,
        });
        config.save().unwrap();

        let adapter = OpenAiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.retries, 2);
    }

    #[test]
    fn openai_adapter_uses_default_retries_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("OPENAI_API_KEY", "sk-dummy").unwrap();

        let adapter = OpenAiAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.retries, 0);
    }
}
