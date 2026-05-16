use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use crate::tools::ToolHandler;

pub struct PatchFileTool;

impl ToolHandler for PatchFileTool {
    fn name(&self) -> &'static str {
        "patch_file"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "patch_file",
            description: "Apply a unified diff to an existing file. Use this instead of \
                write_file when making targeted changes to a small number of lines in a large \
                file — only the changed lines are transmitted rather than the complete file \
                content. The diff must be in unified format with @@ hunk headers (as produced \
                by `git diff` or `diff -u`). The file must have been read via read_file or \
                read_files earlier in this run.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to patch, relative to the working directory."
                    },
                    "diff": {
                        "type": "string",
                        "description": "The unified diff to apply. Must include @@ hunk headers and context lines."
                    }
                },
                "required": ["path", "diff"]
            }),
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("patch_file: missing required argument 'path'"))?;
        let diff = args["diff"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("patch_file: missing required argument 'diff'"))?;

        let abs_path = working_dir.join(path);
        let original = std::fs::read_to_string(&abs_path)
            .with_context(|| format!("patch_file: could not read '{}'", path))?;

        let patch = diffy::Patch::from_str(diff)
            .map_err(|e| anyhow::anyhow!(
                "patch_file: failed to parse diff for '{}': {}. \
                 Ensure the diff uses @@ hunk headers with context lines.",
                path, e
            ))?;

        let patched = diffy::apply(&original, &patch)
            .map_err(|e| anyhow::anyhow!(
                "patch_file: failed to apply diff to '{}': {}. \
                 The diff context lines may not match the current file content — \
                 re-read the file and regenerate the diff.",
                path, e
            ))?;

        let lines_before = original.lines().count();
        let lines_after = patched.lines().count();
        std::fs::write(&abs_path, patched)
            .with_context(|| format!("patch_file: could not write '{}'", path))?;

        Ok(format!(
            "patch_file: applied to '{}' ({} → {} lines).",
            path, lines_before, lines_after
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn patch_file_applies_single_hunk() {
        let dir = temp_dir();
        let file = dir.path().join("target.rs");
        std::fs::write(&file, "fn foo() {\n    let x = 1;\n    x\n}\n").unwrap();

        let tool = PatchFileTool;
        let diff = "@@ -2,2 +2,2 @@\n-    let x = 1;\n+    let x = 42;\n     x\n";
        let args = serde_json::json!({"path": "target.rs", "diff": diff});
        let result = tool.execute(&args, dir.path()).unwrap();

        assert!(result.contains("applied"), "expected success: {}", result);
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("let x = 42;"), "patch must be applied");
        assert!(!content.contains("let x = 1;"), "old line must be removed");
    }

    #[test]
    fn patch_file_returns_error_on_context_mismatch() {
        let dir = temp_dir();
        let file = dir.path().join("target.rs");
        std::fs::write(&file, "fn foo() {}\n").unwrap();

        let tool = PatchFileTool;
        // Syntactically valid diff (correct counts) but context line doesn't match the file
        let diff = "@@ -1,1 +1,1 @@\n-fn bar() {}\n+fn new() {}\n";
        let args = serde_json::json!({"path": "target.rs", "diff": diff});
        let err = tool.execute(&args, dir.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to apply"), "expected apply error: {}", msg);
        // File must be unchanged
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "fn foo() {}\n");
    }

    #[test]
    fn patch_file_returns_error_on_invalid_diff_format() {
        let dir = temp_dir();
        let file = dir.path().join("target.rs");
        std::fs::write(&file, "fn foo() {}\n").unwrap();

        let tool = PatchFileTool;
        // Hunk header count mismatch (says 3 lines but only 2 original lines present) — parse error
        let diff = "@@ -1,3 +1,3 @@\n fn bar() {}\n-fn old() {}\n+fn new() {}\n";
        let args = serde_json::json!({"path": "target.rs", "diff": diff});
        let err = tool.execute(&args, dir.path()).unwrap_err();
        assert!(err.to_string().contains("failed to parse"), "expected parse error: {}", err);
    }

    #[test]
    fn patch_file_returns_error_on_missing_file() {
        let dir = temp_dir();
        let tool = PatchFileTool;
        let diff = "@@ -1,1 +1,1 @@\n-old\n+new\n";
        let args = serde_json::json!({"path": "nonexistent.rs", "diff": diff});
        let err = tool.execute(&args, dir.path()).unwrap_err();
        assert!(err.to_string().contains("could not read"), "expected read error: {}", err);
    }
}
