use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use crate::trace::{CacheUsageEvent, HttpRequestEvent, HttpRetryEvent, QuotaWarningEvent, TraceContext, TraceEvent};
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

#[path = "anthropic_request.rs"]
mod anthropic_request;
pub(crate) use self::anthropic_request::build_request_body;

#[path = "anthropic_response.rs"]
mod anthropic_response;
use self::anthropic_response::parse_response;

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
