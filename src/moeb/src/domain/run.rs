use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ports::AiPort;

const PROMPT_FILE: &str = "src/prompts/run.prompt";
const SPEC_TOKEN: &str = "{{spec}}";
const SPECS_DIR: &str = ".moeb/specifications";

pub struct RunService {
    ai: Arc<dyn AiPort>,
}

impl RunService {
    pub fn new(ai: Arc<dyn AiPort>) -> Self {
        Self { ai }
    }

    pub fn run(&self, spec: &str) -> Result<()> {
        let harness = Path::new(SPECS_DIR);
        if !harness.exists() {
            anyhow::bail!(".moeb/specifications/ not found. Run `moeb init` first.");
        }

        let matches = find_specs(harness, spec)?;

        let spec_path = match matches.len() {
            0 => anyhow::bail!(
                "No specification found matching '{}' under {}.",
                spec,
                SPECS_DIR
            ),
            1 => matches.into_iter().next().unwrap(),
            _ => {
                eprintln!("Multiple specifications match '{}'. Narrow your query:", spec);
                for m in &matches {
                    eprintln!("  {}", m.display());
                }
                anyhow::bail!("Ambiguous specification name.");
            }
        };

        let rel_path = spec_path.to_string_lossy().replace('\\', "/");

        let template = fs::read_to_string(PROMPT_FILE)
            .with_context(|| format!("Cannot read prompt template '{PROMPT_FILE}'. Ensure src/prompts/run.prompt exists."))?;

        let prompt = template.replace(SPEC_TOKEN, &rel_path);

        let working_dir = Path::new(".");
        let result = crate::agent::run_agent_loop(self.ai.as_ref(), &prompt, working_dir)?;
        if !result.is_empty() {
            println!("{}", result);
        }
        Ok(())
    }
}

fn find_specs(harness: &Path, query: &str) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();
    visit_dir(harness, query, &mut matches)?;
    Ok(matches)
}

fn visit_dir(dir: &Path, query: &str, matches: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, query, matches)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.contains(query) {
                matches.push(path);
            }
        }
    }
    Ok(())
}
