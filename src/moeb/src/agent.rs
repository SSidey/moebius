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
        let response = adapter
            .send(&messages, &tools)
            .with_context(|| format!("AI adapter call failed on turn {}", turn + 1))?;

        match response {
            AgentResponse::Text(text) => {
                eprintln!("[moeb] agent finished after {} turn(s)", turn + 1);
                return Ok(text);
            }

            AgentResponse::ToolCalls(calls) => {
                eprintln!("[moeb] turn {}: {} tool call(s)", turn + 1, calls.len());
                for call in &calls {
                    let preview = truncate(&call.arguments, 120);
                    eprintln!("  → {}({})", call.name, preview);
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
        "[moeb] warning: agent loop reached the maximum of {} turns and was halted.",
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
                        out.push_str(&format!("=== {} ===\n{}\n\n", rel, content));
                    }
                    Err(e) => {
                        out.push_str(&format!("=== {} ===\nError: {}\n\n", rel, e));
                    }
                }
            }
            Ok(out)
        }

        other => anyhow::bail!(
            "Unknown tool '{}'. Available: read_file, write_file, list_directory, search_files, grep_files, read_files",
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
        let args = serde_json::json!({"path": "a/", "extension": "rs"}).to_string();
        let result = execute_tool("search_files", &args, tmp.path()).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn execute_tool_grep_files_via_json() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let args = serde_json::json!({"pattern": "struct Baz", "path": "a/"}).to_string();
        let result = execute_tool("grep_files", &args, tmp.path()).unwrap();
        assert!(result.contains("baz.rs"));
        assert!(result.contains("struct Baz"));
    }

    #[test]
    fn read_files_returns_all_contents() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("alpha.txt"), "content-alpha").unwrap();
        fs::write(tmp.path().join("beta.txt"), "content-beta").unwrap();
        let args = serde_json::json!({"paths": ["alpha.txt", "beta.txt"]}).to_string();
        let result = execute_tool("read_files", &args, tmp.path()).unwrap();
        assert!(result.contains("=== alpha.txt ==="));
        assert!(result.contains("content-alpha"));
        assert!(result.contains("=== beta.txt ==="));
        assert!(result.contains("content-beta"));
    }

    #[test]
    fn read_files_reports_error_inline_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("real.txt"), "real-content").unwrap();
        let args = serde_json::json!({"paths": ["real.txt", "nonexistent.txt"]}).to_string();
        let result = execute_tool("read_files", &args, tmp.path()).unwrap();
        assert!(result.contains("real-content"), "valid file content must be present");
        assert!(result.contains("=== nonexistent.txt ==="), "missing file must have a section header");
        assert!(result.contains("Error:"), "missing file must produce an inline error");
    }
}
