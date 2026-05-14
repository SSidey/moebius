use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::{ToolHandler, MAX_RANGE_LINES};

pub struct ReadFileRangeTool;

impl ToolHandler for ReadFileRangeTool {
    fn name(&self) -> &'static str { "read_file_range" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
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
        let capped_end = end.min(start.saturating_add(MAX_RANGE_LINES - 1));
        let actual_end = capped_end.min(total_lines);

        let take_count = if actual_end >= start { actual_end - start + 1 } else { 0 };
        let selected: Vec<&str> = raw.lines().skip(start - 1).take(take_count).collect();

        let served_end = if selected.is_empty() { start } else { start + selected.len() - 1 };
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_file_range_returns_correct_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let content: String = (1..=20).map(|n| format!("line {:02}\n", n)).collect();
        fs::write(tmp.path().join("f.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "f.txt", "start_line": 5, "end_line": 10});
        let result = ReadFileRangeTool.execute(&args, tmp.path()).unwrap();
        assert!(result.contains("lines 5-10"), "header must show the served range");
        assert!(result.contains("line 05"));
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
        let result = ReadFileRangeTool.execute(&args, tmp.path()).unwrap();
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
        let result = ReadFileRangeTool.execute(&args, tmp.path()).unwrap();
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
        let result = ReadFileRangeTool.execute(&args, tmp.path());
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
        let result = ReadFileRangeTool.execute(&args, tmp.path());
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
        let result = ReadFileRangeTool.execute(&args, tmp.path()).unwrap();
        assert!(!result.contains("[capped"), "no cap notice when exactly at cap boundary");
        let content_lines: Vec<&str> = result.lines().skip(1).collect();
        assert_eq!(content_lines.len(), MAX_RANGE_LINES, "all MAX_RANGE_LINES lines must be present");
    }
}
