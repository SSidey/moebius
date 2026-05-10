use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::ports::AiPort;

const PROMPT_FILE: &str = "src/prompts/spec.prompt";
const INPUT_TOKEN: &str = "{{input}}";

pub struct SpecService {
    ai: Arc<dyn AiPort>,
}

impl SpecService {
    pub fn new(ai: Arc<dyn AiPort>) -> Self {
        Self { ai }
    }

    pub fn run(&self, input: &str) -> Result<()> {
        let template = fs::read_to_string(PROMPT_FILE)
            .with_context(|| format!("Cannot read prompt template '{PROMPT_FILE}'. Ensure moeb init has been run and src/prompts/spec.prompt exists."))?;

        let prompt = template.replace(INPUT_TOKEN, input);

        let working_dir = Path::new(".moeb");
        if !working_dir.exists() {
            anyhow::bail!(".moeb/ not found. Run `moeb init` first.");
        }

        let result = crate::agent::run_agent_loop(self.ai.as_ref(), &prompt, working_dir)?;
        if !result.is_empty() {
            println!("{}", result);
        }
        Ok(())
    }
}
