use crate::adapters::Message;

pub struct CompactionStats {
    pub chars_before: usize,
    pub chars_after: usize,
    pub messages_compacted: usize,
}

/// Mutates `messages` in place, replacing old ToolResult contents with summary
/// lines when total ToolResult char count exceeds `threshold`.
///
/// The initial user message (index 0) and all non-ToolResult messages are
/// never modified. The most recent `keep_turns` complete tool-call turns
/// are kept verbatim regardless of size.
///
/// Returns compaction stats if any compaction was performed, or `None`
/// if the threshold was not exceeded.
pub fn compact_history(
    messages: &mut Vec<Message>,
    threshold: usize,
    keep_turns: u32,
) -> Option<CompactionStats> {
    let chars_before: usize = messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { content, .. } => Some(content.len()),
            _ => None,
        })
        .sum();

    if chars_before <= threshold {
        return None;
    }

    let boundary_idx = find_boundary(messages, keep_turns);

    let mut messages_compacted = 0usize;
    for i in 0..boundary_idx {
        if let Message::ToolResult { call_id, content } = &messages[i] {
            if !content.starts_with("[compacted:") {
                let original_len = content.len();
                let placeholder = format!("[compacted: {}, {} chars]", call_id, original_len);
                if let Message::ToolResult { content, .. } = &mut messages[i] {
                    *content = placeholder;
                }
                messages_compacted += 1;
            }
        }
    }

    if messages_compacted == 0 {
        return None;
    }

    let chars_after: usize = messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { content, .. } => Some(content.len()),
            _ => None,
        })
        .sum();

    Some(CompactionStats {
        chars_before,
        chars_after,
        messages_compacted,
    })
}

/// Returns the index of the oldest AssistantToolCalls message that is within
/// the keep window. ToolResults before this index are compaction candidates.
fn find_boundary(messages: &[Message], keep_turns: u32) -> usize {
    if keep_turns == 0 {
        return messages.len();
    }

    let mut turns_seen = 0u32;
    let mut i = messages.len();

    while i > 0 {
        i -= 1;
        if matches!(&messages[i], Message::AssistantToolCalls(_)) {
            turns_seen += 1;
            if turns_seen >= keep_turns {
                return i;
            }
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{Message, ToolCall};

    fn tool_result(call_id: &str, content: &str) -> Message {
        Message::ToolResult {
            call_id: call_id.to_string(),
            content: content.to_string(),
        }
    }

    fn assistant_tool_calls(id: &str) -> Message {
        Message::AssistantToolCalls(vec![ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: "{}".to_string(),
        }])
    }

    fn big_content(n: usize) -> String {
        "x".repeat(n)
    }

    #[test]
    fn below_threshold_no_compaction() {
        let mut messages = vec![
            Message::User("prompt".to_string()),
            assistant_tool_calls("c1"),
            tool_result("c1", "small content"),
        ];
        let result = compact_history(&mut messages, 100_000, 3);
        assert!(result.is_none(), "should not compact below threshold");
        if let Message::ToolResult { content, .. } = &messages[2] {
            assert_eq!(content, "small content");
        }
    }

    #[test]
    fn above_threshold_old_results_compacted() {
        let large = big_content(50_000);
        let mut messages = vec![
            Message::User("prompt".to_string()),
            assistant_tool_calls("c1"),
            tool_result("c1", &large),
            tool_result("c2", &large),
            assistant_tool_calls("c3"),
            tool_result("c3", &large),
            assistant_tool_calls("c4"),
            tool_result("c4", &large),
        ];
        // threshold = 80_000, total = 200_000, keep_turns = 1
        let stats = compact_history(&mut messages, 80_000, 1).expect("should compact");
        // keep_turns=1: keep last 1 ATCalls turn (c4, TR4). Boundary at ATCalls(c4) index=6.
        // Candidates: 0..6 → TR(c1), TR(c2), TR(c3) are compacted; ATCalls are skipped.
        assert_eq!(stats.messages_compacted, 3);
        assert!(stats.chars_after < stats.chars_before);
        // Verify compacted messages
        if let Message::ToolResult { content, .. } = &messages[2] {
            assert!(content.starts_with("[compacted:"), "c1 should be compacted");
        }
        if let Message::ToolResult { content, .. } = &messages[3] {
            assert!(content.starts_with("[compacted:"), "c2 should be compacted");
        }
        if let Message::ToolResult { content, .. } = &messages[5] {
            assert!(content.starts_with("[compacted:"), "c3 should be compacted");
        }
        // Verify kept message
        if let Message::ToolResult { content, .. } = &messages[7] {
            assert_eq!(content.len(), large.len(), "c4 should be untouched");
        }
    }

    #[test]
    fn keep_window_respected() {
        let large = big_content(30_000);
        let mut messages = vec![
            Message::User("prompt".to_string()),
            assistant_tool_calls("c1"),
            tool_result("c1", &large),
            assistant_tool_calls("c2"),
            tool_result("c2", &large),
            assistant_tool_calls("c3"),
            tool_result("c3", &large),
        ];
        // Total = 90_000, threshold = 80_000, keep_turns = 3
        // Scan backward: ATCalls(c3)=1, ATCalls(c2)=2, ATCalls(c1)=3 → boundary=1
        // Candidates: 0..1 = [User] → no ToolResults → nothing compacted
        let result = compact_history(&mut messages, 80_000, 3);
        assert!(result.is_none(), "all 3 turns are in the keep window");
    }

    #[test]
    fn no_double_compaction() {
        let large = big_content(50_000);
        let already = "[compacted: c1, 50000 chars]";
        let mut messages = vec![
            Message::User("prompt".to_string()),
            assistant_tool_calls("c1"),
            tool_result("c1", already),
            tool_result("c2", &large),
            assistant_tool_calls("c3"),
            tool_result("c3", &large),
            assistant_tool_calls("c4"),
            tool_result("c4", &large),
        ];
        let stats = compact_history(&mut messages, 80_000, 1).expect("should compact");
        // c1 is already compacted → skip; c2 is candidate → compact; c3 is candidate → compact
        assert_eq!(stats.messages_compacted, 2, "only 2 non-compacted candidates");
        if let Message::ToolResult { content, .. } = &messages[2] {
            assert_eq!(content, already, "already-compacted must not be modified");
        }
    }

    #[test]
    fn single_turn_nothing_to_compact() {
        let large = big_content(100_000);
        let mut messages = vec![
            Message::User("prompt".to_string()),
            assistant_tool_calls("c1"),
            tool_result("c1", &large),
        ];
        // keep_turns=3: scan backward, only 1 ATCalls found → boundary=0 → no candidates
        let result = compact_history(&mut messages, 80_000, 3);
        assert!(result.is_none(), "single turn: nothing outside keep window");
    }
}
