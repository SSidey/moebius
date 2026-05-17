use anyhow::{Context, Result};
use serde_json::json;
use std::sync::Arc;

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use crate::trace::{CacheUsageEvent, HttpRequestEvent, HttpRetryEvent, QuotaWarningEvent, TraceContext, TraceEvent};
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

#[path = "openai_io.rs"]
mod openai_io;
use self::openai_io::{to_openai_message, to_openai_tool, parse_response};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o";

pub struct OpenAiAdapter {
    api_key: String,
    pub model: String,
    pub retries: u32,
    client: reqwest::blocking::Client,
    trace: Arc<TraceContext>,
}

impl OpenAiAdapter {
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
            trace,
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

        let attempt = self.trace.current_attempt.load(std::sync::atomic::Ordering::SeqCst);
        let turn = self.trace.current_turn.load(std::sync::atomic::Ordering::SeqCst);

        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for http_attempt in 0..max_attempts {
            let start = std::time::Instant::now();

            let response = match self
                .client
                .post(API_URL)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
            {
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
                        url: API_URL.to_string(),
                        request_body: strip_auth_from_body(body.clone()),
                        response_status: 0,
                        response_headers: json!({}),
                        response_body: json!({"error": reason}),
                        duration_ms,
                    }));
                    last_err = Some(anyhow::anyhow!("Failed to reach OpenAI API: {}", e));
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
                    .get("x-ratelimit-remaining-requests")
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
            let text = response.text().context("Failed to read OpenAI response body")?;
            let response_body: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|_| serde_json::Value::String(text.clone()));

            self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                attempt,
                turn,
                http_attempt: http_attempt + 1,
                url: API_URL.to_string(),
                request_body: strip_auth_from_body(body.clone()),
                response_status: status_u16,
                response_headers: strip_auth_headers(response_headers),
                response_body: response_body.clone(),
                duration_ms,
            }));

            if status.is_success() {
                let cache_read = response_body
                    .pointer("/usage/prompt_tokens_details/cached_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if cache_read > 0 {
                    self.trace.push(TraceEvent::CacheUsage(CacheUsageEvent {
                        attempt,
                        turn,
                        cache_read_tokens: cache_read,
                        cache_created_tokens: 0,
                    }));
                }
            }

            if status_u16 == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("OpenAI API error {}: {}", status, text));
                if http_attempt + 1 < max_attempts {
                    let delay = retry::compute_delay(http_attempt, retry_after);
                    let reason = if status_u16 == 429 {
                        "HTTP 429 Too Many Requests".to_string()
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
                anyhow::bail!("OpenAI API error {}: {}", status, text);
            }

            return parse_response(&response_body);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("OpenAI API request failed")))
    }
}

fn strip_auth_from_body(mut v: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("authorization");
        obj.remove("Authorization");
    }
    v
}

fn strip_auth_headers(mut v: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        let keys_to_remove: Vec<_> = obj
            .keys()
            .filter(|k| k.to_lowercase() == "authorization")
            .cloned()
            .collect();
        for k in keys_to_remove {
            obj.remove(&k);
        }
    }
    v
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

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
