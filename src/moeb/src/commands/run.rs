use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::openai::OpenAiAdapter;
use crate::agent::run_agent_loop;
use crate::config::MoebConfig;

const PROMPT_FILE: &str = "src/prompts/run.prompt";
const SPEC_TOKEN: &str = "{{spec}}";
const SPECS_DIR: &str = ".moeb/specifications";

pub fn run(spec: &str) -> Result<()> {
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

    // Full project-root-relative path so the agent can resolve it from the working directory
    // e.g. .moeb/specifications/moeb/moeb.kernel.md
    let rel_path = spec_path.to_string_lossy().replace('\\', "/");

    let template = fs::read_to_string(PROMPT_FILE)
        .with_context(|| format!("Cannot read prompt template '{PROMPT_FILE}'. Ensure src/prompts/run.prompt exists."))?;

    let prompt = template.replace(SPEC_TOKEN, &rel_path);

    let config = MoebConfig::load()?;
    let adapter_name = config.active_adapter.as_deref().unwrap_or("");
    if adapter_name.is_empty() {
        anyhow::bail!("No adapter configured. Run `moeb use <adapter>` first.");
    }

    let adapter = resolve_adapter(adapter_name)?;

    // Working directory is the project root so the agent can write to src/
    let working_dir = Path::new(".");
    let result = run_agent_loop(adapter.as_ref(), &prompt, working_dir)?;
    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
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

fn resolve_adapter(name: &str) -> Result<Box<dyn crate::adapters::Adapter>> {
    match name {
        "openai" => Ok(Box::new(OpenAiAdapter::from_secrets()?)),
        other => anyhow::bail!(
            "Adapter '{}' is configured but not recognised. Run `moeb use <adapter>` to reconfigure.",
            other
        ),
    }
}
