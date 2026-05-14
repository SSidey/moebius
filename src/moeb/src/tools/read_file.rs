use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::{ToolHandler, MAX_READ_BYTES, truncate_to_byte_limit};

pub struct ReadFileTool;

impl ToolHandler for ReadFileTool {
    fn name(&self) -> &'static str { "read_file" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
        let rel = args["path"].as_str().context("read_file: missing 'path'")?;
        let full = working_dir.join(rel);
        let content = fs::read_to_string(&full)
            .with_context(|| format!("read_file: cannot read {}", full.display()))?;
        Ok(truncate_to_byte_limit(content, MAX_READ_BYTES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_file_truncates_large_file() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES + 1000);
        fs::write(tmp.path().join("big.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "big.txt"});
        let result = ReadFileTool.execute(&args, tmp.path()).unwrap();
        assert!(result.contains("[... truncated:"));
        assert!(result.len() < MAX_READ_BYTES + 200);
    }

    #[test]
    fn read_file_does_not_truncate_exact_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES);
        fs::write(tmp.path().join("exact.txt"), &content).unwrap();
        let args = serde_json::json!({"path": "exact.txt"});
        let result = ReadFileTool.execute(&args, tmp.path()).unwrap();
        assert!(!result.contains("[... truncated:"));
    }
}
