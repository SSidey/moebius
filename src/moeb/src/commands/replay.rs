use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::adapters::{AgentResponse, Message, ToolDef};
use crate::agent::run_agent_loop_traced;
use crate::ports::ToolExecutorPort;
use crate::ports::AiPort;
use crate::trace::{
    AgentFinishReason, FileContentMode, TraceCommand, TraceConfig, TraceContext, TraceEnvelope,
    TraceEvent,
};

// ── ReplayAiAdapter ───────────────────────────────────────────────────────────

struct ReplayAiAdapter {
    responses: std::sync::Mutex<VecDeque<serde_json::Value>>,
}

impl ReplayAiAdapter {
    fn new(responses: VecDeque<serde_json::Value>) -> Self {
        Self { responses: std::sync::Mutex::new(responses) }
    }
}

impl AiPort for ReplayAiAdapter {
    fn send(&self, _messages: &[Message], _tools: &[ToolDef]) -> Result<AgentResponse> {
        let body = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("ReplayAiAdapter: no more saved responses in trace"))?;
        parse_response_body(&body)
    }
}

fn parse_response_body(body: &serde_json::Value) -> Result<AgentResponse> {
    // Try Anthropic response format first
    if let Some(stop_reason) = body["stop_reason"].as_str() {
        if stop_reason == "tool_use" {
            let content = body["content"]
                .as_array()
                .context("Missing content array in replay response")?;
            let calls: Result<Vec<_>> = content
                .iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .map(|b| {
                    Ok(crate::adapters::ToolCall {
                        id: b["id"].as_str().context("missing id")?.to_string(),
                        name: b["name"].as_str().context("missing name")?.to_string(),
                        arguments: serde_json::to_string(&b["input"])
                            .context("failed to serialize input")?,
                    })
                })
                .collect();
            return Ok(AgentResponse::ToolCalls(calls?));
        }
        let text = body["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|b| b["type"].as_str() == Some("text"))
                    .and_then(|b| b["text"].as_str())
            })
            .unwrap_or("")
            .to_string();
        return Ok(AgentResponse::Text(text));
    }

    // Try OpenAI response format
    if let Some(finish_reason) = body.pointer("/choices/0/finish_reason").and_then(|v| v.as_str()) {
        if finish_reason == "tool_calls" {
            let calls = body
                .pointer("/choices/0/message/tool_calls")
                .and_then(|v| v.as_array())
                .context("Expected tool_calls array")?
                .iter()
                .map(|v| {
                    Ok(crate::adapters::ToolCall {
                        id: v["id"].as_str().context("missing id")?.to_string(),
                        name: v.pointer("/function/name")
                            .and_then(|v| v.as_str())
                            .context("missing function.name")?
                            .to_string(),
                        arguments: v.pointer("/function/arguments")
                            .and_then(|v| v.as_str())
                            .context("missing function.arguments")?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(AgentResponse::ToolCalls(calls));
        }
        let text = body
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(AgentResponse::Text(text));
    }

    // Fallback: treat as text
    Ok(AgentResponse::Text(
        body.as_str().unwrap_or("").to_string(),
    ))
}

// ── ReplayToolExecutor ────────────────────────────────────────────────────────

struct ReplayToolExecutor {
    results: HashMap<String, String>,
}

impl ReplayToolExecutor {
    fn new(results: HashMap<String, String>) -> Self {
        Self { results }
    }
}

impl ToolExecutorPort for ReplayToolExecutor {
    fn execute(
        &self,
        _name: &str,
        call_id: &str,
        _args: &serde_json::Value,
        _working_dir: &std::path::Path,
        _current_turn: u32,
    ) -> Result<(String, bool)> {
        self.results
            .get(call_id)
            .cloned()
            .map(|r| (r, false))
            .ok_or_else(|| anyhow::anyhow!("ReplayToolExecutor: no saved result for call_id '{}'", call_id))
    }
}

// ── Public replay entry point ─────────────────────────────────────────────────

pub fn run_replay(trace_path: &str, attempt_override: Option<u32>) -> Result<()> {
    let text = std::fs::read_to_string(trace_path)
        .with_context(|| format!("Cannot read trace file: {}", trace_path))?;
    let envelope: TraceEnvelope =
        serde_json::from_str(&text).context("Failed to parse trace file as TraceEnvelope")?;

    run_replay_from_envelope(&envelope, attempt_override)
}

pub fn run_replay_from_envelope(envelope: &TraceEnvelope, attempt_override: Option<u32>) -> Result<()> {
    // Determine target attempt
    let target_attempt = if let Some(n) = attempt_override {
        n
    } else {
        // Last successful attempt (has AgentFinished with Completion)
        let last_success = envelope.events.iter().rev().find_map(|e| {
            if let TraceEvent::AgentFinished(af) = e {
                if matches!(af.reason, AgentFinishReason::Completion) {
                    Some(af.attempt)
                } else {
                    None
                }
            } else {
                None
            }
        });
        last_success.unwrap_or_else(|| {
            // Use the highest attempt number present
            envelope
                .events
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::TurnStart(ts) => Some(ts.attempt),
                    TraceEvent::AgentFinished(af) => Some(af.attempt),
                    TraceEvent::ToolCall(tc) => Some(tc.attempt),
                    _ => None,
                })
                .max()
                .unwrap_or(1)
        })
    };

    // Validate replayability: no hash-only tool calls
    let hash_only: Vec<_> = envelope
        .events
        .iter()
        .filter_map(|e| {
            if let TraceEvent::ToolCall(tc) = e {
                if tc.attempt == target_attempt && tc.result.is_none() {
                    Some(tc)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if !hash_only.is_empty() {
        let mut msg = String::from(
            "Error: trace is not fully replayable. The following tool calls have hash-only content:\n",
        );
        for tc in &hash_only {
            msg.push_str(&format!(
                "  attempt {}, turn {}, call_id \"{}\", tool \"{}\", args {}\n",
                tc.attempt,
                tc.turn,
                tc.call_id,
                tc.tool,
                tc.args,
            ));
        }
        msg.push_str("To capture full content, re-run with: moeb run --embed-files <spec>\n");
        msg.push_str("Or set: moeb configure LOG_FILE_CONTENT true");
        anyhow::bail!("{}", msg);
    }

    // Build ReplayAiAdapter from HttpRequest events for target attempt
    let responses: VecDeque<serde_json::Value> = envelope
        .events
        .iter()
        .filter_map(|e| {
            if let TraceEvent::HttpRequest(hr) = e {
                if hr.attempt == target_attempt && hr.response_status != 0 {
                    Some(hr.response_body.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Build ReplayToolExecutor from ToolCall events for target attempt
    let results: HashMap<String, String> = envelope
        .events
        .iter()
        .filter_map(|e| {
            if let TraceEvent::ToolCall(tc) = e {
                if tc.attempt == target_attempt {
                    tc.result.as_ref().map(|r| (tc.call_id.clone(), r.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Reconstruct initial_messages from TurnStart for turn=1 of target attempt
    let initial_messages: Vec<Message> = envelope
        .events
        .iter()
        .find_map(|e| {
            if let TraceEvent::TurnStart(ts) = e {
                if ts.attempt == target_attempt && ts.turn == 1 {
                    Some(
                        ts.messages_sent
                            .iter()
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect(),
                    )
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_default();

    if initial_messages.is_empty() {
        anyhow::bail!(
            "Cannot replay: no TurnStart event for attempt {} turn 1 found in trace.",
            target_attempt
        );
    }

    let ai_stub = Arc::new(ReplayAiAdapter::new(responses));
    let tool_stub = ReplayToolExecutor::new(results);

    // Noop trace context (retention=0 means no file is written)
    let noop_trace = Arc::new(TraceContext::new(TraceConfig {
        command: TraceCommand::Run,
        spec: envelope.spec.clone(),
        adapter: envelope.adapter.clone(),
        model: envelope.model.clone(),
        retention: 0,
        file_content_mode: FileContentMode::Embed,
    }));

    let tools: Vec<ToolDef> = crate::tools::ToolRegistry::standard().definitions();
    let working_dir = std::path::Path::new(".");

    let result = run_agent_loop_traced(
        ai_stub.as_ref(),
        &tool_stub,
        &tools,
        working_dir,
        initial_messages,
        50,
        &noop_trace,
        target_attempt,
    )?;

    if !result.is_empty() {
        println!("{}", result);
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{
        AgentFinishedEvent, ToolCallEvent, TurnStartEvent, TraceCommand, TraceOutcome, TurnResponseType,
        TurnEndEvent,
    };

    fn make_envelope_with_hash_tool_call() -> TraceEnvelope {
        TraceEnvelope {
            version: 1,
            command: TraceCommand::Run,
            spec: "test.spec".to_string(),
            adapter: "anthropic".to_string(),
            model: "claude-opus-4-7".to_string(),
            started_at: "2024-01-01T00:00:00Z".to_string(),
            ended_at: "2024-01-01T00:01:00Z".to_string(),
            outcome: TraceOutcome::Failure,
            error: None,
            total_attempts: 1,
            events: vec![
                TraceEvent::TurnStart(TurnStartEvent {
                    attempt: 1,
                    turn: 1,
                    messages_sent: vec![serde_json::json!({"role": "user", "content": "hello"})],
                }),
                TraceEvent::ToolCall(ToolCallEvent {
                    attempt: 1,
                    turn: 1,
                    call_id: "call-abc123".to_string(),
                    tool: "read_file".to_string(),
                    args: serde_json::json!({"path": "src/main.rs"}),
                    result: None,
                    content_hash: Some("abc123def456".to_string()),
                    chars: 500,
                    success: true,
                    duration_ms: 5,
                    cache_hit: false,
                }),
                TraceEvent::TurnEnd(TurnEndEvent {
                    attempt: 1,
                    turn: 1,
                    response_type: TurnResponseType::Text,
                    response_content: serde_json::Value::String("done".to_string()),
                }),
                TraceEvent::AgentFinished(AgentFinishedEvent {
                    attempt: 1,
                    turns: 1,
                    reason: AgentFinishReason::Completion,
                }),
            ],
        }
    }

    #[test]
    fn replay_fails_on_hash_only_tool_call() {
        let envelope = make_envelope_with_hash_tool_call();
        let err = run_replay_from_envelope(&envelope, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not fully replayable"),
            "expected 'not fully replayable', got: {}",
            msg
        );
        assert!(
            msg.contains("call-abc123"),
            "expected call_id 'call-abc123', got: {}",
            msg
        );
    }
}
