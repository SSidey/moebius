use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use crate::trace::{CacheUsageEvent, HttpRequestEvent, HttpRetryEvent, QuotaWarningEvent, TraceContext, TraceEvent};
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
    pub prompt_cache: bool,
    client: reqwest::blocking::Client,
    trace: Arc<TraceContext>,
}

impl AnthropicAdapter {
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
            prompt_cache: cfg.effective_prompt_cache(),
            client: reqwest::blocking::Client::new(),
            trace,
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
        let body = build_request_body(&self.model, messages, tools, self.prompt_cache)?;
        let attempt = self.trace.current_attempt.load(std::sync::atomic::Ordering::SeqCst);
        let turn = self.trace.current_turn.load(std::sync::atomic::Ordering::SeqCst);

        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for http_attempt in 0..max_attempts {
            let start = std::time::Instant::now();

            let mut req = self
                .client
                .post(API_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs));
            if self.prompt_cache {
                req = req.header("anthropic-beta", "prompt-caching-2024-07-31");
            }
            let response = match req.json(&body).send() {
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
                    // Emit a dummy http_request for the failed transport call
                    self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                        attempt,
                        turn,
                        http_attempt: http_attempt + 1,
                        url: API_URL.to_string(),
                        request_body: strip_auth_from_value(body.clone()),
                        response_status: 0,
                        response_headers: json!({}),
                        response_body: json!({"error": reason}),
                        duration_ms,
                    }));
                    last_err = Some(anyhow::anyhow!("Failed to reach Anthropic API: {}", e));
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
            if status.is_success() {
                let remaining = response.headers()
                    .get("anthropic-ratelimit-requests-remaining")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                if let Some(ref rem) = remaining {
                    if rem.parse::<u64>().unwrap_or(u64::MAX) < 5 {
                        eprintln!(
                            "Warning: AI API rate limit nearly exhausted ({} requests remaining). \
                             Consider waiting before the next run or reducing concurrent usage.",
                            rem
                        );
                        self.trace.push(TraceEvent::QuotaWarning(QuotaWarningEvent {
                            attempt,
                            remaining: rem.clone(),
                        }));
                    }
                }
            }
            let text = response.text().context("Failed to read Anthropic response body")?;
            let response_body: Value = serde_json::from_str(&text)
                .unwrap_or_else(|_| Value::String(text.clone()));

            self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                attempt,
                turn,
                http_attempt: http_attempt + 1,
                url: API_URL.to_string(),
                request_body: strip_auth_from_value(body.clone()),
                response_status: status_u16,
                response_headers: strip_auth_from_headers(response_headers),
                response_body: response_body.clone(),
                duration_ms,
            }));

            if status.is_success() {
                let cache_read = response_body
                    .pointer("/usage/cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cache_created = response_body
                    .pointer("/usage/cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if cache_read > 0 || cache_created > 0 {
                    self.trace.push(TraceEvent::CacheUsage(CacheUsageEvent {
                        attempt,
                        turn,
                        cache_read_tokens: cache_read,
                        cache_created_tokens: cache_created,
                    }));
                }
            }

            if status_u16 == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("Anthropic API error {}: {}", status, text));
                if http_attempt + 1 < max_attempts {
                    let delay = retry::compute_delay(http_attempt, retry_after);
                    let reason = if status_u16 == 429 {
                        format!("HTTP 429 Too Many Requests")
                    } else {
                        format!("HTTP {} {}", status_u16, status.canonical_reason().unwrap_or("Server Error"))
                    };
                    self.trace.push(TraceEvent::HttpRetry(HttpRetryEvent {
                        attempt,
                        turn,
                        http_attempt: http_attempt + 1,
                        delay_ms: delay.as_millis() as u64,
                        reason,
                    }));
                    std::thread::sleep(delay);
                }
                continue;
            }

            if !status.is_success() {
                anyhow::bail!("Anthropic API error {}: {}", status, text);
            }

            return parse_response(&response_body);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Anthropic API request failed")))
    }
}

// ── Auth stripping ────────────────────────────────────────────────────────────

fn strip_auth_from_value(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("authorization");
        obj.remove("Authorization");
        obj.remove("x-api-key");
        obj.remove("X-Api-Key");
    }
    v
}

fn strip_auth_from_headers(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        let keys_to_remove: Vec<_> = obj
            .keys()
            .filter(|k| {
                let lower = k.to_lowercase();
                lower == "authorization" || lower == "x-api-key"
            })
            .cloned()
            .collect();
        for k in keys_to_remove {
            obj.remove(&k);
        }
    }
    v
}

fn headers_to_json(headers: &reqwest::header::HeaderMap) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            map.insert(name.to_string(), Value::String(v.to_string()));
        }
    }
    Value::Object(map)
}

// ── Request construction ──────────────────────────────────────────────────────

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
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();

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
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();

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

    #[test]
    fn build_request_body_caches_system_when_prompt_cache_true() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], true).unwrap();

        let system = body["system"].as_array()
            .expect("system must be an array when prompt_cache=true");
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["type"], "text");
        assert_eq!(system[0]["text"], "sys");
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn build_request_body_plain_string_system_when_prompt_cache_false() {
        let (_dir, _guard) = in_temp_dir();
        let messages = vec![
            Message::System("sys".to_string()),
            Message::User("hi".to_string()),
        ];
        let body = build_request_body("claude-opus-4-7", &messages, &[], false).unwrap();
        assert_eq!(body["system"], "sys", "system must be a plain string when prompt_cache=false");
    }
}
