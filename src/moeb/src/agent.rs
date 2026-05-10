use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;

use crate::adapters::{AgentResponse, Message, ToolDef};
use crate::ports::AiPort;

const MAX_TURNS: usize = 50;

/// Drive an agent loop until the model produces a plain text response or MAX_TURNS is reached.
/// All file tool paths are resolved relative to `working_dir`.
pub fn run_agent_loop(
    adapter: &dyn AiPort,
    initial_prompt: &str,
    working_dir: &Path,
) -> Result<String> {
    let tools = file_tools();
    let mut messages: Vec<Message> = vec![Message::User(initial_prompt.to_string())];

    for turn in 0..MAX_TURNS {
        let response = adapter.send(&messages, &tools)?;

        match response {
            AgentResponse::Text(text) => {
                return Ok(text);
            }

            AgentResponse::ToolCalls(calls) => {
                eprintln!("[moeb] turn {}: {} tool call(s)", turn + 1, calls.len());
                for call in &calls {
                    eprintln!("  → {}({})", call.name, call.arguments);
                }

                messages.push(Message::AssistantToolCalls(calls.clone()));

                for call in &calls {
                    let result = execute_tool(&call.name, &call.arguments, working_dir);
                    let content = match &result {
                        Ok(output) => {
                            eprintln!("  ✓ {}: {} chars", call.name, output.len());
                            output.clone()
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
        "Warning: agent loop reached the maximum of {} turns and was halted.",
        MAX_TURNS
    );
    Ok(String::new())
}

// ── Tool definitions ──────────────────────────────────────────────────────────

fn file_tools() -> Vec<ToolDef> {
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
    ]
}

// ── Tool execution ────────────────────────────────────────────────────────────

fn execute_tool(name: &str, arguments: &str, working_dir: &Path) -> Result<String> {
    let args: serde_json::Value =
        serde_json::from_str(arguments).with_context(|| format!("Invalid JSON arguments: {}", arguments))?;

    match name {
        "read_file" => {
            let rel = args["path"].as_str().context("read_file: missing 'path'")?;
            let full = working_dir.join(rel);
            fs::read_to_string(&full)
                .with_context(|| format!("read_file: cannot read {}", full.display()))
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

        other => anyhow::bail!("Unknown tool '{}'. Available: read_file, write_file, list_directory", other),
    }
}
