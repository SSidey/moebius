use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent::MAX_TURNS;
use crate::assets::Prompts;
use crate::config::MoebConfig;
use crate::ports::AdapterFactoryPort;
#[cfg(test)]
use crate::ports::AiPort;
use crate::trace::{
    FileContentMode, TraceCommand, TraceConfig, TraceContext, TraceOutcome,
};

const PROMPT_FILE: &str = "run.prompt";
const SPEC_TOKEN: &str = "{{spec}}";
const README_TOKEN: &str = "{{readme_content}}";
const SPEC_CONTENT_TOKEN: &str = "{{spec_content}}";
const SPECS_DIR: &str = ".moeb/specifications";
const README_PATH: &str = ".moeb/README.md";

pub struct RunService {
    factory: Arc<dyn AdapterFactoryPort>,
}

#[cfg(test)]
struct FixedAdapterFactory(Arc<dyn AiPort>);

#[cfg(test)]
impl AdapterFactoryPort for FixedAdapterFactory {
    fn build(&self, _trace: Arc<TraceContext>) -> anyhow::Result<Arc<dyn AiPort>> {
        Ok(Arc::clone(&self.0))
    }
}

impl RunService {
    pub fn from_config() -> Self {
        Self {
            factory: Arc::new(crate::adapters::DefaultAdapterFactory),
        }
    }

    #[cfg(test)]
    pub fn new(ai: Arc<dyn AiPort>) -> Self {
        Self {
            factory: Arc::new(FixedAdapterFactory(ai)),
        }
    }

    pub fn run(&self, spec: &str, file_content_mode: FileContentMode) -> Result<()> {
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

        let spec_slug = spec_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| spec.to_string());

        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_name = cfg.active_adapter.clone().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config(&adapter_name);
        let model = adapter_cfg.effective_model("unknown");

        let trace_config = TraceConfig {
            command: TraceCommand::Run,
            spec: spec_slug,
            adapter: adapter_name,
            model,
            retention: cfg.effective_run_retention(),
            file_content_mode,
        };
        let trace = Arc::new(TraceContext::new(trace_config));

        let ai = self.factory.build(Arc::clone(&trace))?;

        let working_dir = Path::new(".");
        let state = crate::run_state::new_shared_run_state();
        let tools = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&state)).definitions();
        let executor = crate::tools::RealToolExecutor::new(std::sync::Arc::clone(&state));
        let initial_messages = vec![crate::adapters::Message::User(prompt)];
        let compaction_config = crate::agent::CompactionConfig {
            enabled: cfg.effective_compaction_enabled(),
            threshold: cfg.effective_compaction_threshold(),
            keep_turns: cfg.effective_compaction_keep_turns(),
        };
        let run_result = crate::agent::run_agent_loop_run_mode(
            ai.as_ref(),
            &executor,
            &tools,
            working_dir,
            initial_messages,
            MAX_TURNS,
            &trace,
            1,
            compaction_config,
            state,
        );

        let (outcome, err_msg) = match &run_result {
            Ok(_) => (TraceOutcome::Success, None),
            Err(e) => (TraceOutcome::Failure, Some(e.to_string())),
        };
        if let Err(e) = trace.finalize(outcome, err_msg) {
            eprintln!("[moeb] warning: trace could not be saved: {}", e);
        }

        let result = run_result?;
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
        service.run("test.spec", crate::trace::FileContentMode::Embed).expect("run should succeed");

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
