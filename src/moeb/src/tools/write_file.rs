use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::ToolHandler;

pub struct WriteFileTool;

impl ToolHandler for WriteFileTool {
    fn name(&self) -> &'static str { "write_file" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
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
}
