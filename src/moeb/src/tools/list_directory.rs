use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::ToolHandler;

pub struct ListDirectoryTool;

impl ToolHandler for ListDirectoryTool {
    fn name(&self) -> &'static str { "list_directory" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
        let rel = args["path"].as_str().context("list_directory: missing 'path'")?;
        let full = working_dir.join(rel);
        let mut entries: Vec<String> = fs::read_dir(&full)
            .with_context(|| format!("list_directory: cannot read {}", full.display()))?
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() { format!("{}/", name) } else { name }
            })
            .collect();
        entries.sort();
        Ok(entries.join("\n"))
    }
}
