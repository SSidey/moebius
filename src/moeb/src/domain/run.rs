use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::assets::Prompts;
use crate::ports::AiPort;

const PROMPT_FILE: &str = "run.prompt";
const SPEC_TOKEN: &str = "{{spec}}";
const README_TOKEN: &str = "{{readme_content}}";
const SPEC_CONTENT_TOKEN: &str = "{{spec_content}}";
const SPECS_DIR: &str = ".moeb/specifications";
const README_PATH: &str = ".moeb/README.md";

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

        let asset = Prompts::get(PROMPT_FILE)
            .context("Embedded prompt template 'run.prompt' not found in binary")?;
        let template = std::str::from_utf8(asset.data.as_ref())
            .context("run.prompt is not valid UTF-8")?;

        let readme_content = fs::read_to_string(README_PATH)
            .with_context(|| format!(
                "Cannot read {}. Run `moeb init` first.",
                README_PATH
            ))?;

        let spec_content = fs::read_to_string(&spec_path)
            .with_context(|| format!("Cannot read {}", spec_path.display()))?;

        let prompt = template
            .replace(SPEC_TOKEN, &rel_path)
            .replace(README_TOKEN, &readme_content)
            .replace(SPEC_CONTENT_TOKEN, &spec_content);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AgentResponse, Message, ToolDef};
    use crate::config::tests::CWD_LOCK;
    use crate::ports::AiPort;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct CapturingStub {
        captured: Mutex<Option<String>>,
    }

    impl AiPort for CapturingStub {
        fn send(&self, messages: &[Message], _tools: &[ToolDef]) -> Result<AgentResponse> {
            let mut captured = self.captured.lock().unwrap();
            if captured.is_none() {
                if let Some(Message::User(text)) = messages.first() {
                    *captured = Some(text.clone());
                }
            }
            Ok(AgentResponse::Text(String::new()))
        }
    }

    fn setup() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_current_dir(dir.path()).expect("set_current_dir");
        (dir, guard)
    }

    #[test]
    fn run_substitutes_readme_and_spec_content() {
        let (_dir, _guard) = setup();

        fs::create_dir_all(".moeb/specifications/moeb").expect("create spec dir");
        fs::write(".moeb/README.md", "readme-body").expect("write README");
        fs::write(
            ".moeb/specifications/moeb/test.spec.md",
            "spec-body",
        )
        .expect("write spec");

        let stub = Arc::new(CapturingStub {
            captured: Mutex::new(None),
        });

        let service = RunService::new(stub.clone() as Arc<dyn AiPort>);
        service.run("test.spec").expect("run should succeed");

        let captured = stub.captured.lock().unwrap();
        let prompt = captured.as_ref().expect("prompt should have been captured");

        assert!(prompt.contains("readme-body"), "prompt must contain README content");
        assert!(prompt.contains("spec-body"), "prompt must contain spec content");
        assert!(
            !prompt.contains("{{readme_content}}"),
            "{{readme_content}} token must be replaced"
        );
        assert!(
            !prompt.contains("{{spec_content}}"),
            "{{spec_content}} token must be replaced"
        );
    }
}
