use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::ToolHandler;

pub struct GrepFilesTool;

impl ToolHandler for GrepFilesTool {
    fn name(&self) -> &'static str { "grep_files" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
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
    fn execute_tool_grep_files_via_json() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let args = serde_json::json!({"pattern": "struct Baz", "path": "a/"});
        let result = GrepFilesTool.execute(&args, tmp.path()).unwrap();
        assert!(result.contains("baz.rs"));
        assert!(result.contains("struct Baz"));
    }
}
