use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::config::{MoebConfig, Secrets};
use crate::ports::AiPort;
use crate::trace::{HttpRequestEvent, HttpRetryEvent, TraceContext, TraceEvent};
use super::{retry, Adapter, AgentResponse, Message, ToolCall, ToolDef};

#[path = "gemini_io.rs"]
mod gemini_io;
pub(crate) use self::gemini_io::build_request_body;
use self::gemini_io::parse_response;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const DEFAULT_MODEL: &str = "gemini-2.0-flash";
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

pub struct GeminiAdapter {
    api_key: String,
    pub model: String,
    pub retries: u32,
    pub timeout_secs: u64,
    client: reqwest::blocking::Client,
    trace: Arc<TraceContext>,
}

impl GeminiAdapter {
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
            .get("GEMINI_API_KEY")
            .context("GEMINI_API_KEY not set. Run `moeb use gemini` first.")?
            .to_string();
        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config("gemini");
        Ok(Self {
            api_key,
            model: adapter_cfg.effective_model(DEFAULT_MODEL),
            retries: adapter_cfg.effective_retries(),
            timeout_secs: adapter_cfg.effective_timeout_secs(DEFAULT_TIMEOUT_SECS),
            client: reqwest::blocking::Client::new(),
            trace,
        })
    }
}

impl AiPort for GeminiAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        Adapter::send(self, messages, tools)
    }
}

impl Adapter for GeminiAdapter {
    fn send(&self, messages: &[Message], tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = build_request_body(messages, tools);
        let url = format!("{}/{}:generateContent", API_BASE, self.model);
        let attempt = self.trace.current_attempt.load(std::sync::atomic::Ordering::SeqCst);
        let turn = self.trace.current_turn.load(std::sync::atomic::Ordering::SeqCst);
        let max_attempts = self.retries + 1;
        let mut last_err: Option<anyhow::Error> = None;

        for http_attempt in 0..max_attempts {
            let start = std::time::Instant::now();
            let response = match self
                .client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("content-type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .json(&body)
                .send()
            {
                Err(e) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    let reason = format!("transport error: {}", e);
                    let delay = retry::compute_delay(http_attempt, None);
                    if http_attempt + 1 < max_attempts {
                        self.trace.push(TraceEvent::HttpRetry(HttpRetryEvent {
                            attempt, turn, http_attempt: http_attempt + 1,
                            delay_ms: delay.as_millis() as u64, reason: reason.clone(),
                        }));
                        std::thread::sleep(delay);
                    }
                    self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                        attempt, turn, http_attempt: http_attempt + 1,
                        url: url.clone(), request_body: body.clone(),
                        response_status: 0, response_headers: json!({}),
                        response_body: json!({"error": reason}), duration_ms,
                    }));
                    last_err = Some(anyhow::anyhow!("Failed to reach Gemini API: {}", e));
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
            let text = response.text().context("Failed to read Gemini response body")?;
            let response_body: Value = serde_json::from_str(&text)
                .unwrap_or_else(|_| Value::String(text.clone()));

            self.trace.push(TraceEvent::HttpRequest(HttpRequestEvent {
                attempt, turn, http_attempt: http_attempt + 1, url: url.clone(),
                request_body: body.clone(), response_status: status_u16,
                response_headers, response_body: response_body.clone(), duration_ms,
            }));

            if status_u16 == 429 || status.is_server_error() {
                last_err = Some(anyhow::anyhow!("Gemini API error {}: {}", status, text));
                if http_attempt + 1 < max_attempts {
                    let delay = retry::compute_delay(http_attempt, retry_after);
                    let reason = if status_u16 == 429 {
                        "HTTP 429 Too Many Requests".to_string()
                    } else {
                        format!("HTTP {} {}", status_u16,
                            status.canonical_reason().unwrap_or("Server Error"))
                    };
                    self.trace.push(TraceEvent::HttpRetry(HttpRetryEvent {
                        attempt, turn, http_attempt: http_attempt + 1,
                        delay_ms: delay.as_millis() as u64, reason,
                    }));
                    std::thread::sleep(delay);
                }
                continue;
            }

            if !status.is_success() {
                anyhow::bail!("Gemini API error {}: {}", status, text);
            }

            return parse_response(&response_body);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Gemini API request failed")))
    }
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

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
