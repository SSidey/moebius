use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-opus-4-7";
const MAX_TOKENS: u32 = 8192;
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

pub struct AnthropicAdapter {
    api_key: String,
    pub model: String,
    pub retries: u32,
    pub timeout_secs: u64,
    client: reqwest::blocking::Client,
}

impl AnthropicAdapter {
    pub fn from_secrets_and_config() -> Result<Self> {
        let secrets = Secrets::load()?;
        let api_key = secrets
            .get("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY not set. Run `moeb use anthropic` first.")?
            .to_string();
        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config("anthropic");
        Ok(Self {
            api_key,
            model: adapter_cfg.effective_model(DEFAULT_MODEL),
            retries: adapter_cfg.effective_retries(),
            timeout_secs: adapter_cfg.effective_timeout_secs(DEFAULT_TIMEOUT_SECS),
            client: reqwest::blocking::Client::new(),
        })
    }
}

impl AiPort for AnthropicAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        Adapter::send(self, messages, tools)
    }
}

impl Adapter for AnthropicAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = build_request_body(&self.model, messages, tools)?;

        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..max_attempts {
            let response = match self
                .client
                .post(API_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .json(&body)
                .send()
            {
                Err(e) => {
                    last_err = Some(anyhow::anyhow!("Failed to reach Anthropic API: {}", e));
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
                retry::warn_if_quota_low(response.headers(), "anthropic-ratelimit-requests-remaining");
            }
            let text = response.text().context("Failed to read Anthropic response body")?;

            if status.as_u16() == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("Anthropic API error {}: {}", status, text));
                if attempt + 1 < max_attempts {
                    std::thread::sleep(retry::compute_delay(attempt, retry_after));
                }
                continue;
            }

            if !status.is_success() {
                anyhow::bail!("Anthropic API error {}: {}", status, text);
            }

            let value: Value =
                serde_json::from_str(&text).context("Failed to parse Anthropic response JSON")?;

            return parse_response(&value);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Anthropic API request failed")))
    }
}

// ── Request construction ──────────────────────────────────────────────────────

pub(crate) fn build_request_body(
    model: &str,
    messages: &[Message],
    tools: &[ToolDef],
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
        body.insert("system".into(), json!(sys));
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

// ── Response parsing ──────────────────────────────────────────────────────────

fn parse_response(value: &Value) -> Result<AgentResponse> {
    let stop_reason = value["stop_reason"].as_str().unwrap_or("");
    let content = value["content"]
        .as_array()
        .context("Missing content array in Anthropic response")?;

    if stop_reason == "tool_use" {
        let calls: Result<Vec<ToolCall>> = content
            .iter()
            .filter(|block| block["type"].as_str() == Some("tool_use"))
            .map(parse_tool_call)
            .collect();
        return Ok(AgentResponse::ToolCalls(calls?));
    }

    let text = content
        .iter()
        .find(|block| block["type"].as_str() == Some("text"))
        .and_then(|block| block["text"].as_str())
        .unwrap_or("")
        .to_string();

    Ok(AgentResponse::Text(text))
}

fn parse_tool_call(block: &Value) -> Result<ToolCall> {
    let id = block["id"]
        .as_str()
        .context("Tool use block missing id")?
        .to_string();
    let name = block["name"]
        .as_str()
        .context("Tool use block missing name")?
        .to_string();
    let arguments = serde_json::to_string(&block["input"])
        .context("Failed to serialise tool call input")?;
    Ok(ToolCall { id, name, arguments })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    use crate::adapters::retry;
    use crate::config::{tests::CWD_LOCK, AdapterConfig, MoebConfig, Secrets, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb dir");
        (dir, guard)
    }

    #[test]
    fn anthropic_adapter_uses_configured_model() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("anthropic".to_string(), AdapterConfig {
            model: Some("claude-haiku-4-5".to_string()),
            retries: None,
            timeout_secs: None,
        });
        config.save().unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "claude-haiku-4-5");
    }

    #[test]
    fn anthropic_adapter_uses_default_model_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.model, "claude-opus-4-7");
    }

    #[test]
    fn system_message_extracted_to_top_level() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[]).unwrap();

        assert_eq!(body["system"], "sys", "system field must be top-level");

        let msgs = body["messages"].as_array().unwrap();
        for m in msgs {
            assert_ne!(
                m["role"].as_str(),
                Some("system"),
                "no system-role entry should appear in messages array"
            );
        }
        assert_eq!(msgs.len(), 1, "only one user message expected");
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn anthropic_adapter_uses_configured_timeout() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();
        let mut config = MoebConfig::load().unwrap();
        config.adapters.insert("anthropic".to_string(), AdapterConfig {
            model: None,
            retries: None,
            timeout_secs: Some(120),
        });
        config.save().unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 120);
    }

    #[test]
    fn anthropic_adapter_uses_default_timeout_when_absent() {
        let (_dir, _guard) = in_temp_dir();
        let mut secrets = Secrets::load().unwrap();
        secrets.set("ANTHROPIC_API_KEY", "sk-ant-dummy").unwrap();

        let adapter = AnthropicAdapter::from_secrets_and_config().unwrap();
        assert_eq!(adapter.timeout_secs, 600);
    }

    #[test]
    fn anthropic_retry_delay_first_attempt_within_bounds() {
        let delay = retry::compute_delay(0, None);
        assert!(
            delay >= Duration::from_millis(750),
            "delay too short: {:?}",
            delay
        );
        assert!(
            delay <= Duration::from_millis(1250),
            "delay too long: {:?}",
            delay
        );
    }

    #[test]
    fn consecutive_tool_results_are_batched() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::ToolResult { call_id: "c1".to_string(), content: "r1".to_string() },
            Message::ToolResult { call_id: "c2".to_string(), content: "r2".to_string() },
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[]).unwrap();

        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1, "two ToolResults must be batched into one user message");
        assert_eq!(msgs[0]["role"], "user");

        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "c1");
        assert_eq!(content[1]["type"], "tool_result");
        assert_eq!(content[1]["tool_use_id"], "c2");
    }
}
