use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde_json::json;
use crate::adapters::ToolDef;
use super::ToolHandler;

pub struct SearchFilesTool;

impl ToolHandler for SearchFilesTool {
    fn name(&self) -> &'static str { "search_files" }

    fn definition(&self) -> ToolDef {
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
        }
    }

    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String> {
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
    fn execute_tool_search_files_via_json() {
        let tmp = tempfile::tempdir().unwrap();
        setup(&tmp);
        let args = serde_json::json!({"path": "a/", "extension": "rs"});
        let result = SearchFilesTool.execute(&args, tmp.path()).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
    }
}
