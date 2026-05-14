use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::adapters::{AgentResponse, Message, ToolDef};
use crate::ports::AiPort;
use crate::trace::{
    AgentFinishReason, AgentFinishedEvent, FileContentMode, ToolCallEvent, TraceContext, TraceEvent,
    TurnEndEvent, TurnResponseType, TurnStartEvent, apply_content_policy,
};

const MAX_TURNS: usize = 50;
const MAX_READ_BYTES: usize = 102_400; // 100 KiB per file read result
const MAX_RANGE_LINES: usize = 300;

fn truncate_to_byte_limit(content: String, limit: usize) -> String {
    if content.len() <= limit {
        return content;
    }
    let mut boundary = limit;
    while boundary > 0 && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let total = content.len();
    let shown = boundary;
    format!(
        "{}\n[... truncated: {} of {} chars shown ...]",
        &content[..boundary],
        shown,
        total
    )
}

// ── ToolExecutorPort ──────────────────────────────────────────────────────────

pub trait ToolExecutorPort: Send + Sync {
    fn execute(
        &self,
        name: &str,
        call_id: &str,
        args: &serde_json::Value,
        working_dir: &Path,
    ) -> Result<String>;
}

// ── RealToolExecutor ──────────────────────────────────────────────────────────

pub struct RealToolExecutor {
    pub trace: Arc<TraceContext>,
    pub file_content_mode: FileContentMode,
    pub attempt: u32,
    pub current_turn: std::sync::atomic::AtomicU32,
}

impl RealToolExecutor {
    pub fn new(trace: Arc<TraceContext>, file_content_mode: FileContentMode, attempt: u32) -> Self {
        Self {
            trace,
            file_content_mode,
            attempt,
            current_turn: std::sync::atomic::AtomicU32::new(1),
        }
    }

    pub fn set_turn(&self, turn: u32) {
        self.current_turn.store(turn, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ToolExecutorPort for RealToolExecutor {
    fn execute(
        &self,
        name: &str,
        call_id: &str,
        args: &serde_json::Value,
        working_dir: &Path,
    ) -> Result<String> {
        let start = std::time::Instant::now();
        let raw_result = execute_tool_inner(name, args, working_dir);
        let duration_ms = start.elapsed().as_millis() as u64;

        let turn = self.current_turn.load(std::sync::atomic::Ordering::SeqCst);
        let (stored_result, content_hash, chars) =
            apply_content_policy(name, &raw_result, self.file_content_mode);

        let success = raw_result.is_ok();
        let return_val = match &raw_result {
            Ok(s) => s.clone(),
            Err(e) => format!("Error: {}", e),
        };

        self.trace.push(TraceEvent::ToolCall(ToolCallEvent {
            attempt: self.attempt,
            turn,
            call_id: call_id.to_string(),
            tool: name.to_string(),
            args: args.clone(),
            result: stored_result,
            content_hash,
            chars,
            success,
            duration_ms,
        }));

        Ok(return_val)
    }
}

// ── Agent loop ────────────────────────────────────────────────────────────────

/// Drive an agent loop until the model produces a plain text response or MAX_TURNS is reached.
pub fn run_agent_loop(
    adapter: &dyn AiPort,
    initial_prompt: &str,
    working_dir: &Path,
) -> Result<String> {
    let tools = file_tools();
    let messages: Vec<Message> = vec![Message::User(initial_prompt.to_string())];
    let noop_trace = Arc::new(crate::trace::TraceContext::new(crate::trace::TraceConfig {
        command: crate::trace::TraceCommand::Run,
        spec: String::new(),
        adapter: String::new(),
        model: String::new(),
        retention: 0,
        file_content_mode: FileContentMode::Embed,
    }));
    let executor = RealToolExecutor::new(noop_trace.clone(), FileContentMode::Embed, 1);
    run_agent_loop_inner(adapter, &executor, &tools, working_dir, messages, MAX_TURNS, &noop_trace, 1)
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
) -> Result<String> {
    run_agent_loop_inner(adapter, tool_exec, tools, working_dir, initial_messages, max_turns, trace, attempt)
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
) -> Result<String> {
    let mut messages = initial_messages;

    for turn in 0..max_turns {
        let turn_num = (turn + 1) as u32;

        trace.current_turn.store(turn_num, std::sync::atomic::Ordering::SeqCst);
        trace.current_attempt.store(attempt, std::sync::atomic::Ordering::SeqCst);

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
            AgentResponse::Text(text) => {
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

            AgentResponse::ToolCalls(calls) => {
                eprintln!("[moeb] turn {}: {} tool call(s)", turn_num, calls.len());
                for call in calls {
                    let preview = truncate(&call.arguments, 120);
                    eprintln!("  → {}({})", call.name, preview);
                }

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
                    let content = match tool_exec.execute(&call.name, &call.id, &args, working_dir) {
                        Ok(output) => {
                            eprintln!("  ✓ {}: {} chars", call.name, output.len());
                            output
                        }
                        Err(e) => {
                            eprintln!("  ✗ {}: {}", call.name, e);
                            format!("Error: {}", e)
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

// ── Tool definitions ──────────────────────────────────────────────────────────

pub fn file_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file",
            description: "Read the full contents of a file. Path is relative to the working directory.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the working directory" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "write_file",
            description: "Write content to a file, creating any missing parent directories. Path is relative to the working directory.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the working directory" },
                    "content": { "type": "string", "description": "Full content to write to the file" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "list_directory",
            description: "List the immediate children of a directory. Directories are suffixed with /. Path is relative to the working directory.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path relative to the working directory" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "search_files",
            description: "Recursively find files under a directory, optionally filtered by file extension. Returns one relative path per line. Capped at 500 results.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Root directory to search, relative to the working directory" },
                    "extension": { "type": "string", "description": "Optional file extension to filter by, e.g. \"rs\" or \"toml\"" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "grep_files",
            description: "Search file contents for a substring. Returns matching lines as path:line_number: content. Capped at 200 matches.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Case-sensitive substring to search for" },
                    "path": { "type": "string", "description": "File or directory to search, relative to the working directory. Defaults to \".\"" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "read_files",
            description: "Read the full contents of multiple files in one call. Returns each file's path as a labelled header followed by its content. Paths are relative to the working directory.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "File paths relative to the working directory"
                    }
                },
                "required": ["paths"]
            }),
        },
        ToolDef {
            name: "read_file_range",
            description: "Read a specific range of lines from a file. Use this after grep_files identifies \
                          the relevant line number — request only the lines that contain the symbol or block \
                          you need. Lines are 1-based. Returns at most 300 lines regardless of the range \
                          requested. Paths are relative to the working directory.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the working directory"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "1-based line number to start reading from (inclusive)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "1-based line number to stop reading at (inclusive)"
                    }
                },
                "required": ["path", "start_line", "end_line"]
            }),
        },
    ]
}

// ── Tool execution (inner) ────────────────────────────────────────────────────

pub fn execute_tool_inner(name: &str, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
    match name {
        "read_file" => {
            let rel = args["path"].as_str().context("read_file: missing 'path'")?;
            let full = working_dir.join(rel);
            let content = fs::read_to_string(&full)
                .with_context(|| format!("read_file: cannot read {}", full.display()))?;
            Ok(truncate_to_byte_limit(content, MAX_READ_BYTES))
        }

        "write_file" => {
            let rel = args["path"].as_str().context("write_file: missing 'path'")?;
            let content = args["content"].as_str().context("write_file: missing 'content'")?;
            let full = working_dir.join(rel);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("write_file: cannot create {}", parent.display()))?;
            }
            fs::write(&full, content)
                .with_context(|| format!("write_file: cannot write {}", full.display()))?;
            Ok(format!("Wrote {} bytes to {}", content.len(), rel))
        }

        "list_directory" => {
            let rel = args["path"].as_str().context("list_directory: missing 'path'")?;
            let full = working_dir.join(rel);
            let mut entries: Vec<String> = fs::read_dir(&full)
                .with_context(|| format!("list_directory: cannot read {}", full.display()))?
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if e.path().is_dir() {
                        format!("{}/", name)
                    } else {
                        name
                    }
                })
                .collect();
            entries.sort();
            Ok(entries.join("\n"))
        }

        "search_files" => {
            let rel = args["path"].as_str().context("search_files: missing 'path'")?;
            let ext = args.get("extension").and_then(|v| v.as_str());
            let full = working_dir.join(rel);
            let mut found = Vec::new();
            collect_files(&full, ext, working_dir, &mut found)?;
            found.sort();
            if found.is_empty() {
                Ok("No files found.".to_string())
            } else {
                Ok(found.join("\n"))
            }
        }

        "grep_files" => {
            let pattern = args["pattern"].as_str().context("grep_files: missing 'pattern'")?;
            let rel = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let full = working_dir.join(rel);
            let mut matches = Vec::new();
            grep_in_path(&full, pattern, working_dir, &mut matches);
            if matches.is_empty() {
                Ok("No matches found.".to_string())
            } else {
                Ok(matches.join("\n"))
            }
        }

        "read_files" => {
            let paths = args["paths"]
                .as_array()
                .context("read_files: 'paths' must be an array")?;
            let mut out = String::new();
            for path_val in paths {
                let rel = path_val
                    .as_str()
                    .context("read_files: each path must be a string")?;
                let full = working_dir.join(rel);
                match fs::read_to_string(&full) {
                    Ok(content) => {
                        let capped = truncate_to_byte_limit(content, MAX_READ_BYTES);
                        out.push_str(&format!("=== {} ===\n{}\n\n", rel, capped));
                    }
                    Err(e) => {
                        out.push_str(&format!("=== {} ===\nError: {}\n\n", rel, e));
                    }
                }
            }
            Ok(out)
        }

        "read_file_range" => {
            let rel = args["path"]
                .as_str()
                .context("read_file_range: missing 'path'")?;
            let start = args["start_line"]
                .as_u64()
                .context("read_file_range: 'start_line' must be a non-negative integer")? as usize;
            let end = args["end_line"]
                .as_u64()
                .context("read_file_range: 'end_line' must be a non-negative integer")? as usize;

            if start < 1 {
                anyhow::bail!("read_file_range: 'start_line' must be >= 1 (lines are 1-based)");
            }
            if end < start {
                anyhow::bail!(
                    "read_file_range: 'end_line' ({}) must be >= 'start_line' ({})",
                    end,
                    start
                );
            }

            let full = working_dir.join(rel);
            let raw = fs::read_to_string(&full)
                .with_context(|| format!("read_file_range: cannot read {}", full.display()))?;

            let total_lines = raw.lines().count();
            let requested_end = end;
            // Cap the requested end at start + MAX_RANGE_LINES - 1, then further cap at EOF.
            let capped_end = end.min(start.saturating_add(MAX_RANGE_LINES - 1));
            let actual_end = capped_end.min(total_lines);

            let take_count = if actual_end >= start { actual_end - start + 1 } else { 0 };
            let selected: Vec<&str> = raw
                .lines()
                .skip(start - 1)
                .take(take_count)
                .collect();

            // Build the header line: reports the actual range served plus any notices.
            let served_end = if selected.is_empty() {
                start
            } else {
                start + selected.len() - 1
            };
            let mut header = format!("// {}: lines {}-{}", rel, start, served_end);
            if capped_end < requested_end {
                header.push_str(&format!(
                    " [capped at {} lines; requested end={}]",
                    MAX_RANGE_LINES, requested_end
                ));
            } else if actual_end < requested_end {
                header.push_str(&format!(" [file has only {} lines]", total_lines));
            }

            Ok(format!("{}\n{}", header, selected.join("\n")))
        }

        other => anyhow::bail!(
            "Unknown tool '{}'. Available: read_file, write_file, list_directory, search_files, grep_files, read_files, read_file_range",
            other
        ),
    }
}

fn collect_files(dir: &Path, ext: Option<&str>, base: &Path, out: &mut Vec<String>) -> Result<()> {
    const MAX: usize = 500;
    if out.len() >= MAX {
        return Ok(());
    }
    if !dir.exists() {
        anyhow::bail!("search_files: path does not exist: {}", dir.display());
    }
    if dir.is_file() {
        out.push(dir.strip_prefix(base).unwrap_or(dir).to_string_lossy().replace('\\', "/"));
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("search_files: cannot read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        if out.len() >= MAX {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, base, out)?;
        } else {
            let matches = match ext {
                Some(e) => path.extension().and_then(|x| x.to_str()) == Some(e),
                None => true,
            };
            if matches {
                out.push(path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

fn grep_in_path(path: &Path, pattern: &str, base: &Path, out: &mut Vec<String>) {
    const MAX: usize = 200;
    if out.len() >= MAX {
        return;
    }
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                if out.len() >= MAX {
                    return;
                }
                grep_in_path(&entry.path(), pattern, base, out);
            }
        }
    } else if path.is_file() {
        if let Ok(content) = fs::read_to_string(path) {
            let rel = path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/");
            for (i, line) in content.lines().enumerate() {
                if out.len() >= MAX {
                    return;
                }
                if line.contains(pattern) {
                    out.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
                }
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup(tmp: &tempfile::TempDir) {
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/foo.rs"), "fn hello() {}\nfn world() {}\n").unwrap();
        fs::write(root.join("a/bar.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(root.join("a/b/baz.rs"), "struct Baz;\nfn hello() {}\n").unwrap();
    }

    #[test]
    fn search_files_returns_all_files_without_extension_filter() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let mut found = Vec::new();
        collect_files(tmp.path(), None, tmp.path(), &mut found).unwrap();
        found.sort();
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|f| f.ends_with("foo.rs")));
        assert!(found.iter().any(|f| f.ends_with("bar.toml")));
        assert!(found.iter().any(|f| f.ends_with("baz.rs")));
    }

    #[test]
    fn search_files_filters_by_extension() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let mut found = Vec::new();
        collect_files(tmp.path(), Some("rs"), tmp.path(), &mut found).unwrap();
        found.sort();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|f| f.ends_with(".rs")));
    }

    #[test]
    fn grep_files_finds_matching_lines() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let mut matches = Vec::new();
        grep_in_path(tmp.path(), "fn hello", tmp.path(), &mut matches);
        assert_eq!(matches.len(), 2, "hello appears in two files");
        assert!(matches.iter().all(|m| m.contains("fn hello")));
    }

    #[test]
    fn grep_files_returns_empty_for_no_matches() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let mut matches = Vec::new();
        grep_in_path(tmp.path(), "nonexistent_symbol_xyz", tmp.path(), &mut matches);
        assert!(matches.is_empty());
    }

    #[test]
    fn grep_files_includes_file_and_line_number() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let mut matches = Vec::new();
        grep_in_path(tmp.path(), "fn world", tmp.path(), &mut matches);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].contains(":2:"), "line number must be 2");
        assert!(matches[0].contains("foo.rs"));
    }

    #[test]
    fn execute_tool_search_files_via_json() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let args = serde_json::json!({"path": "a/", "extension": "rs"});
        let result = execute_tool_inner("search_files", &args, tmp.path()).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn execute_tool_grep_files_via_json() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let args = serde_json::json!({"pattern": "struct Baz", "path": "a/"});
        let result = execute_tool_inner("grep_files", &args, tmp.path()).unwrap();
        assert!(result.contains("baz.rs"));
        assert!(result.contains("struct Baz"));
    }

    #[test]
    fn read_files_returns_all_contents() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("alpha.txt"), "content-alpha").unwrap();
        fs::write(tmp.path().join("beta.txt"), "content-beta").unwrap();
        let args = serde_json::json!({"paths": ["alpha.txt", "beta.txt"]});
        let result = execute_tool_inner("read_files", &args, tmp.path()).unwrap();
        assert!(result.contains("=== alpha.txt ==="));
        assert!(result.contains("content-alpha"));
        assert!(result.contains("=== beta.txt ==="));
        assert!(result.contains("content-beta"));
    }

    #[test]
    fn read_files_reports_error_inline_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("real.txt"), "real-content").unwrap();
        let args = serde_json::json!({"paths": ["real.txt", "nonexistent.txt"]});
        let result = execute_tool_inner("read_files", &args, tmp.path()).unwrap();
        assert!(result.contains("real-content"), "valid file content must be present");
        assert!(result.contains("=== nonexistent.txt ==="), "missing file must have a section header");
        assert!(result.contains("Error:"), "missing file must produce an inline error");
    }

    #[test]
    fn truncate_to_byte_limit_passes_short_content() {
        let s = "hello world".to_string();
        let result = truncate_to_byte_limit(s.clone(), MAX_READ_BYTES);
        assert_eq!(result, s);
    }

    #[test]
    fn truncate_to_byte_limit_truncates_long_content() {
        let s = "x".repeat(MAX_READ_BYTES + 1000);
        let result = truncate_to_byte_limit(s.clone(), MAX_READ_BYTES);
        assert!(result.len() <= MAX_READ_BYTES + 80);
        assert!(result.contains("[... truncated:"));
        assert!(result.contains(&format!("of {}", s.len())));
    }

    #[test]
    fn read_file_truncates_large_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES + 1000);
        fs::write(tmp.path().join("big.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "big.txt"});
        let result = execute_tool_inner("read_file", &args, tmp.path()).unwrap();
        assert!(result.contains("[... truncated:"));
        assert!(result.len() < MAX_READ_BYTES + 200);
    }

    #[test]
    fn read_files_truncates_each_file_independently() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES + 500);
        fs::write(tmp.path().join("file1.txt"), &content).unwrap();
        fs::write(tmp.path().join("file2.txt"), &content).unwrap();
        let args = serde_json::json!({"paths": ["file1.txt", "file2.txt"]});
        let result = execute_tool_inner("read_files", &args, tmp.path()).unwrap();
        let count = result.matches("[... truncated:").count();
        assert_eq!(count, 2, "both files must be independently truncated");
        assert!(result.contains("=== file1.txt ==="));
        assert!(result.contains("=== file2.txt ==="));
    }

    #[test]
    fn read_file_does_not_truncate_exact_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES);
        fs::write(tmp.path().join("exact.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "exact.txt"});
        let result = execute_tool_inner("read_file", &args, tmp.path()).unwrap();
        assert!(!result.contains("[... truncated:"));
    }

    #[test]
    fn read_file_range_returns_correct_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let content: String = (1..=20).map(|n| format!("line {:02}\n", n)).collect();
        fs::write(tmp.path().join("f.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "f.txt", "start_line": 5, "end_line": 10});
        let result = execute_tool_inner("read_file_range", &args, tmp.path()).unwrap();
        assert!(result.contains("lines 5-10"), "header must show the served range");
        assert!(result.contains("line 05"));
        assert!(result.contains("line 06"));
        assert!(result.contains("line 07"));
        assert!(result.contains("line 08"));
        assert!(result.contains("line 09"));
        assert!(result.contains("line 10"));
        assert!(!result.contains("line 04"), "line 4 must not appear");
        assert!(!result.contains("line 11"), "line 11 must not appear");
    }

    #[test]
    fn read_file_range_clamps_to_max_range_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let content: String = (1..=500).map(|n| format!("line {}\n", n)).collect();
        fs::write(tmp.path().join("big.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "big.txt", "start_line": 1, "end_line": 500});
        let result = execute_tool_inner("read_file_range", &args, tmp.path()).unwrap();
        assert!(result.contains(&format!("[capped at {} lines", MAX_RANGE_LINES)), "cap notice must appear");
        let content_lines: Vec<&str> = result.lines().skip(1).collect();
        assert_eq!(content_lines.len(), MAX_RANGE_LINES, "exactly MAX_RANGE_LINES content lines");
    }

    #[test]
    fn read_file_range_clamps_to_file_end() {
        let tmp = tempfile::tempdir().unwrap();
        let content: String = (1..=15).map(|n| format!("line {}\n", n)).collect();
        fs::write(tmp.path().join("short.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "short.txt", "start_line": 10, "end_line": 50});
        let result = execute_tool_inner("read_file_range", &args, tmp.path()).unwrap();
        assert!(result.contains("[file has only 15 lines]"), "file-end notice must appear");
        assert!(result.contains("line 10"));
        assert!(result.contains("line 15"));
        assert!(!result.contains("line 16"), "no content beyond file end");
    }

    #[test]
    fn read_file_range_rejects_end_before_start() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("f.txt"), "hello\n").unwrap();
        let args = serde_json::json!({"path": "f.txt", "start_line": 10, "end_line": 5});
        let result = execute_tool_inner("read_file_range", &args, tmp.path());
        assert!(result.is_err(), "must return Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("end_line"), "error must mention end_line");
        assert!(msg.contains("start_line"), "error must mention start_line");
    }

    #[test]
    fn read_file_range_rejects_zero_start() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("f.txt"), "hello\n").unwrap();
        let args = serde_json::json!({"path": "f.txt", "start_line": 0, "end_line": 10});
        let result = execute_tool_inner("read_file_range", &args, tmp.path());
        assert!(result.is_err(), "must return Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("1-based"), "error must mention 1-based");
    }

    #[test]
    fn read_file_range_exact_cap_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let content: String = (1..=MAX_RANGE_LINES).map(|n| format!("line {}\n", n)).collect();
        fs::write(tmp.path().join("cap.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "cap.txt", "start_line": 1, "end_line": MAX_RANGE_LINES});
        let result = execute_tool_inner("read_file_range", &args, tmp.path()).unwrap();
        assert!(!result.contains("[capped"), "no cap notice when exactly at cap boundary");
        let content_lines: Vec<&str> = result.lines().skip(1).collect();
        assert_eq!(content_lines.len(), MAX_RANGE_LINES, "all MAX_RANGE_LINES lines must be present");
    }
}
