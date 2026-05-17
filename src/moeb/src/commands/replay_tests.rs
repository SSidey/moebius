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
