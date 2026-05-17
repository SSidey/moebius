use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::adapters::{Message, ToolDef};
use crate::agent::run_agent_loop_traced;
use crate::trace::{
    AgentFinishReason, FileContentMode, TraceCommand, TraceConfig, TraceContext, TraceEnvelope,
    TraceEvent,
};

#[path = "replay_adapter.rs"]
mod replay_adapter;
use self::replay_adapter::ReplayAiAdapter;

#[path = "replay_executor.rs"]
mod replay_executor;
use self::replay_executor::ReplayToolExecutor;

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

    let replay_state = crate::run_state::new_shared_run_state();
    let tools: Vec<ToolDef> = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&replay_state)).definitions();
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
        crate::agent::CompactionConfig::default(),
        replay_state,
    )?;

    if !result.is_empty() {
        println!("{}", result);
    }

    Ok(())
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
