use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::{ToolHandler, MAX_READ_BYTES, truncate_to_byte_limit};

pub struct ReadFilesTool;

impl ToolHandler for ReadFilesTool {
    fn name(&self) -> &'static str { "read_files" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn read_files_returns_all_contents() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("alpha.txt"), "content-alpha").unwrap();
        fs::write(tmp.path().join("beta.txt"), "content-beta").unwrap();
        let args = serde_json::json!({"paths": ["alpha.txt", "beta.txt"]});
        let result = ReadFilesTool.execute(&args, tmp.path()).unwrap();
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
        let result = ReadFilesTool.execute(&args, tmp.path()).unwrap();
        assert!(result.contains("real-content"), "valid file content must be present");
        assert!(result.contains("=== nonexistent.txt ==="), "missing file must have a section header");
        assert!(result.contains("Error:"), "missing file must produce an inline error");
    }

    #[test]
    fn read_files_truncates_each_file_independently() {
        let tmp = tempfile::tempdir().unwrap();
        let content = "x".repeat(MAX_READ_BYTES + 500);
        fs::write(tmp.path().join("file1.txt"), &content).unwrap();
        fs::write(tmp.path().join("file2.txt"), &content).unwrap();
        let args = serde_json::json!({"paths": ["file1.txt", "file2.txt"]});
        let result = ReadFilesTool.execute(&args, tmp.path()).unwrap();
        let count = result.matches("[... truncated:").count();
        assert_eq!(count, 2, "both files must be independently truncated");
        assert!(result.contains("=== file1.txt ==="));
        assert!(result.contains("=== file2.txt ==="));
    }
}
