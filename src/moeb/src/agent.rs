use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::adapters::{AgentResponse, Message, ToolDef};
use crate::ports::{AiPort, ToolExecutorPort};
use crate::run_state::SharedRunState;
use crate::trace::{
    apply_content_policy, AgentFinishReason, AgentFinishedEvent, CompactionEvent, FileContentMode,
    ToolCallEvent, TraceContext, TraceEvent, TurnEndEvent, TurnResponseType, TurnStartEvent,
};

pub const MAX_TURNS: usize = 50;

pub struct CompactionConfig {
    pub enabled: bool,
    pub threshold: usize,
    pub keep_turns: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self { enabled: false, threshold: 80_000, keep_turns: 3 }
    }
}

// ── Agent loop ────────────────────────────────────────────────────────────────

/// Drive an agent loop until the model produces a plain text response or MAX_TURNS is reached.
pub fn run_agent_loop(
    adapter: &dyn AiPort,
    initial_prompt: &str,
    working_dir: &Path,
) -> Result<String> {
    let state = crate::run_state::new_shared_run_state();
    let tools = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&state)).definitions();
    let messages: Vec<Message> = vec![Message::User(initial_prompt.to_string())];
    let noop_trace = Arc::new(crate::trace::TraceContext::new(crate::trace::TraceConfig {
        command: crate::trace::TraceCommand::Run,
        spec: String::new(),
        adapter: String::new(),
        model: String::new(),
        retention: 0,
        file_content_mode: FileContentMode::Embed,
    }));
    let executor = crate::tools::RealToolExecutor::new(std::sync::Arc::clone(&state));
    run_agent_loop_inner(adapter, &executor, &tools, working_dir, messages, MAX_TURNS, &noop_trace, 1, true, CompactionConfig::default(), state)
}

pub fn run_agent_loop_traced(
    adapter: &dyn AiPort,
    tool_exec: &dyn ToolExecutorPort,
    tools: &[ToolDef],
    working_dir: &Path,
    initial_messages: Vec<Message>,
    max_turns: usize,
    trace: &TraceContext,
    attempt: u32,
    compaction_config: CompactionConfig,
    state: SharedRunState,
) -> Result<String> {
    run_agent_loop_inner(adapter, tool_exec, tools, working_dir, initial_messages, max_turns, trace, attempt, false, compaction_config, state)
}

/// Variant for `moeb run`: continues the loop on text turns until at least one `write_file`
/// has been dispatched, then accepts the next text turn as a completion summary.
pub fn run_agent_loop_run_mode(
    adapter: &dyn AiPort,
    tool_exec: &dyn ToolExecutorPort,
    tools: &[ToolDef],
    working_dir: &Path,
    initial_messages: Vec<Message>,
    max_turns: usize,
    trace: &TraceContext,
    attempt: u32,
    compaction_config: CompactionConfig,
    state: SharedRunState,
) -> Result<String> {
    run_agent_loop_inner(adapter, tool_exec, tools, working_dir, initial_messages, max_turns, trace, attempt, true, compaction_config, state)
}

fn continue_nudge(state: &SharedRunState) -> String {
    let locked = state.lock().unwrap();
    if !locked.task_list_created() {
        return "Continue. Call create_task_list with your numbered plan, then call \
                write_file (or other tools) to implement the next step.".to_string();
    }
    let pending = locked.pending_tasks();
    if pending.is_empty() {
        "Continue. All tasks complete — call verify_rubrics with your rubric verdicts, \
         then provide your completion summary.".to_string()
    } else {
        let list = pending
            .iter()
            .map(|t| format!("[{}] {}", t.id, t.description))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "Continue. Remaining tasks: {}. Call write_file (or other tools) to \
             implement the next step.",
            list
        )
    }
}

fn run_agent_loop_inner(
    adapter: &dyn AiPort,
    tool_exec: &dyn ToolExecutorPort,
    tools: &[ToolDef],
    working_dir: &Path,
    initial_messages: Vec<Message>,
    max_turns: usize,
    trace: &TraceContext,
    attempt: u32,
    require_write_before_completion: bool,
    compaction_config: CompactionConfig,
    state: SharedRunState,
) -> Result<String> {
    let mut messages = initial_messages;
    let mut write_file_dispatched = false;
    let mut consecutive_text_turns: u32 = 0;

    for turn in 0..max_turns {
        let turn_num = (turn + 1) as u32;

        trace.current_turn.store(turn_num, std::sync::atomic::Ordering::SeqCst);
        trace.current_attempt.store(attempt, std::sync::atomic::Ordering::SeqCst);

        if compaction_config.enabled {
            if let Some(stats) = crate::compaction::compact_history(
                &mut messages,
                compaction_config.threshold,
                compaction_config.keep_turns,
            ) {
                trace.push(TraceEvent::Compaction(CompactionEvent {
                    attempt,
                    turn: turn_num,
                    chars_before: stats.chars_before,
                    chars_after: stats.chars_after,
                    messages_compacted: stats.messages_compacted,
                }));
            }
        }

        trace.push(TraceEvent::TurnStart(TurnStartEvent {
            attempt,
            turn: turn_num,
            messages_sent: messages
                .iter()
                .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
                .collect(),
        }));

        let response = adapter
            .send(&messages, tools)
            .with_context(|| format!("AI adapter call failed on turn {}", turn_num))?;

        match &response {
            AgentResponse::Text(ref text) => {
                if require_write_before_completion {
                    consecutive_text_turns += 1;

                    if consecutive_text_turns >= 3 {
                        eprintln!(
                            "[moeb] warning: agent received {} consecutive text turns with no tool calls. \
                             The model did not produce any file modifications. \
                             Check that the specification path is correct, inspect the prompt, and retry.",
                            consecutive_text_turns
                        );
                        eprintln!("[moeb] agent finished after {} turn(s)", turn_num);
                        trace.push(TraceEvent::TurnEnd(TurnEndEvent {
                            attempt,
                            turn: turn_num,
                            response_type: TurnResponseType::Text,
                            response_content: serde_json::Value::String(text.clone()),
                        }));
                        trace.push(TraceEvent::AgentFinished(AgentFinishedEvent {
                            attempt,
                            turns: turn_num,
                            reason: AgentFinishReason::MaxTurns,
                        }));
                        return Ok(text.clone());
                    }

                    if write_file_dispatched {
                        eprintln!("[moeb] agent finished after {} turn(s)", turn_num);
                        trace.push(TraceEvent::TurnEnd(TurnEndEvent {
                            attempt,
                            turn: turn_num,
                            response_type: TurnResponseType::Text,
                            response_content: serde_json::Value::String(text.clone()),
                        }));
                        trace.push(TraceEvent::AgentFinished(AgentFinishedEvent {
                            attempt,
                            turns: turn_num,
                            reason: AgentFinishReason::Completion,
                        }));
                        return Ok(text.clone());
                    }

                    // Model produced a planning or preamble turn without calling tools.
                    // Append it to the conversation and ask the model to proceed to tool calls.
                    trace.push(TraceEvent::TurnEnd(TurnEndEvent {
                        attempt,
                        turn: turn_num,
                        response_type: TurnResponseType::Text,
                        response_content: serde_json::Value::String(text.clone()),
                    }));
                    messages.push(Message::Assistant(text.clone()));
                    messages.push(Message::User(continue_nudge(&state)));
                    // do NOT break or return — continue to the next iteration
                } else {
                    eprintln!("[moeb] agent finished after {} turn(s)", turn_num);
                    trace.push(TraceEvent::TurnEnd(TurnEndEvent {
                        attempt,
                        turn: turn_num,
                        response_type: TurnResponseType::Text,
                        response_content: serde_json::Value::String(text.clone()),
                    }));
                    trace.push(TraceEvent::AgentFinished(AgentFinishedEvent {
                        attempt,
                        turns: turn_num,
                        reason: AgentFinishReason::Completion,
                    }));
                    return Ok(text.clone());
                }
            }

            AgentResponse::ToolCalls(calls) => {
                eprintln!("[moeb] turn {}: {} tool call(s)", turn_num, calls.len());
                for call in calls {
                    let preview = truncate(&call.arguments, 120);
                    eprintln!("  → {}({})", call.name, preview);
                }

                for call in calls {
                    if call.name == "write_file" {
                        write_file_dispatched = true;
                    }
                }
                consecutive_text_turns = 0;

                trace.push(TraceEvent::TurnEnd(TurnEndEvent {
                    attempt,
                    turn: turn_num,
                    response_type: TurnResponseType::ToolCalls,
                    response_content: serde_json::to_value(calls).unwrap_or(serde_json::Value::Null),
                }));

                messages.push(Message::AssistantToolCalls(calls.clone()));

                for call in calls {
                    let args: serde_json::Value = serde_json::from_str(&call.arguments)
                        .with_context(|| format!("Invalid JSON arguments for tool '{}'", call.name))?;

                    let start = std::time::Instant::now();
                    let exec_result = tool_exec.execute(&call.name, &call.id, &args, working_dir, turn_num);
                    let duration_ms = start.elapsed().as_millis() as u64;

                    let content = match exec_result {
                        Ok((text, cache_hit)) => {
                            eprintln!("  ✓ {}: {} chars", call.name, text.len());
                            let mode = trace.file_content_mode();
                            let (stored, hash, chars) =
                                apply_content_policy(&call.name, &Ok(text.clone()), mode);
                            trace.push(TraceEvent::ToolCall(ToolCallEvent {
                                attempt,
                                turn: turn_num,
                                call_id: call.id.clone(),
                                tool: call.name.clone(),
                                args: args.clone(),
                                result: stored,
                                content_hash: hash,
                                chars,
                                success: true,
                                duration_ms,
                                cache_hit,
                            }));
                            text
                        }
                        Err(e) => {
                            eprintln!("  ✗ {}: {}", call.name, e);
                            let err_str = format!("Error: {}", e);
                            trace.push(TraceEvent::ToolCall(ToolCallEvent {
                                attempt,
                                turn: turn_num,
                                call_id: call.id.clone(),
                                tool: call.name.clone(),
                                args: args.clone(),
                                result: Some(err_str.clone()),
                                content_hash: None,
                                chars: 0,
                                success: false,
                                duration_ms,
                                cache_hit: false,
                            }));
                            err_str
                        }
                    };
                    messages.push(Message::ToolResult {
                        call_id: call.id.clone(),
                        content,
                    });
                }
            }
        }
    }

    eprintln!(
        "[moeb] warning: agent loop reached the maximum of {} turns and was halted.",
        max_turns
    );
    trace.push(TraceEvent::AgentFinished(AgentFinishedEvent {
        attempt,
        turns: max_turns as u32,
        reason: AgentFinishReason::MaxTurns,
    }));
    Ok(String::new())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
