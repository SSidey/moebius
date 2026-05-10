use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::adapters::openai::OpenAiAdapter;
use crate::agent::run_agent_loop;
use crate::config::MoebConfig;

const PROMPT_FILE: &str = "src/prompts/spec.prompt";
const INPUT_TOKEN: &str = "{{input}}";

pub fn run(input: &str) -> Result<()> {
    let template = fs::read_to_string(PROMPT_FILE)
        .with_context(|| format!("Cannot read prompt template '{PROMPT_FILE}'. Ensure moeb init has been run and src/prompts/spec.prompt exists."))?;

    let prompt = template.replace(INPUT_TOKEN, input);

    let config = MoebConfig::load()?;
    let adapter_name = config.active_adapter.as_deref().unwrap_or("");
    if adapter_name.is_empty() {
        anyhow::bail!("No adapter configured. Run `moeb use <adapter>` first.");
    }

    let adapter = resolve_adapter(adapter_name)?;

    let working_dir = Path::new(".moeb");
    if !working_dir.exists() {
        anyhow::bail!(".moeb/ not found. Run `moeb init` first.");
    }

    let result = run_agent_loop(adapter.as_ref(), &prompt, working_dir)?;
    if !result.is_empty() {
        println!("{}", result);
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
