use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::agent::MAX_TURNS;
use crate::assets::{Assets, Prompts};
use crate::config::MoebConfig;
use crate::ports::AdapterFactoryPort;
#[cfg(test)]
use crate::ports::AiPort;
use crate::trace::{
    FileContentMode, TraceCommand, TraceConfig, TraceContext, TraceOutcome,
};

const PROMPT_FILE: &str = "spec.prompt";
const README_LINK_PROMPT_FILE: &str = "readme-link.prompt";
const INPUT_TOKEN: &str = "{{input}}";
const README_TOKEN: &str = "{{readme_content}}";
const SPEC_SCHEMA_TOKEN: &str = "{{spec_schema_content}}";
const RUBRICS_TOKEN: &str = "{{rubrics_content}}";
const RUBRICS_PATH: &str = "rubrics/rubrics.index.md";

const REQUIRED_SECTIONS: &[&str] = &[
    "# ",
    "## Raw Requirement",
    "## Description",
    "```mermaid",
    "## Backlinks",
    "## Steps",
    "## Decisions",
    "## Rubric",
];

// ── Validation schema ─────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ValidationSchema {
    frontmatter: FrontmatterSchema,
    body: BodySchema,
}

#[derive(serde::Deserialize)]
struct FrontmatterSchema {
    required: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    optional: Vec<String>,
}

#[derive(serde::Deserialize)]
struct BodySchema {
    required_sections: Vec<String>,
}

fn load_validation_schema(working_dir: &Path) -> Option<ValidationSchema> {
    let path = working_dir.join("spec-schema-validation.json");
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<ValidationSchema>(&content) {
            Ok(schema) => Some(schema),
            Err(e) => {
                eprintln!(
                    "[moeb] warning: spec-schema-validation.json is malformed ({}); \
                     falling back to built-in validation rules.",
                    e
                );
                None
            }
        },
        Err(_) => {
            eprintln!(
                "[moeb] warning: spec-schema-validation.json not found in {:?}; \
                 falling back to built-in validation rules.",
                working_dir
            );
            None
        }
    }
}

pub struct SpecService {
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

impl SpecService {
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

    pub fn run(&self, input: &str, _file_content_mode: FileContentMode) -> Result<()> {
        let working_dir = Path::new(".moeb");
        if !working_dir.exists() {
            bail!(".moeb/ not found. Run `moeb init` first.");
        }
        let cfg = MoebConfig::load().unwrap_or_default();
        let limit = cfg.effective_spec_retry_limit();
        self.run_in(input, working_dir, limit, _file_content_mode)
    }

    pub(crate) fn run_in(
        &self,
        input: &str,
        working_dir: &Path,
        retry_limit: u32,
        file_content_mode: FileContentMode,
    ) -> Result<()> {
        let asset = Prompts::get(PROMPT_FILE)
            .context("Embedded prompt template 'spec.prompt' not found in binary")?;
        let template = std::str::from_utf8(asset.data.as_ref())
            .context("spec.prompt is not valid UTF-8")?;

        let readme_content = fs::read_to_string(working_dir.join("README.md"))
            .with_context(|| {
                format!(
                    "Cannot read {}/README.md. Run `moeb init` first.",
                    working_dir.display()
                )
            })?;

        let spec_schema_asset = Assets::get("spec-schema.yaml")
            .context("Embedded asset 'spec-schema.yaml' not found in binary")?;
        let spec_schema_content = std::str::from_utf8(spec_schema_asset.data.as_ref())
            .context("spec-schema.yaml embedded asset is not valid UTF-8")?
            .to_string();

        let rubrics_content =
            fs::read_to_string(working_dir.join(RUBRICS_PATH)).unwrap_or_else(|_| {
                "(rubrics catalogue not found — rubrics/rubrics.index.md is absent)".to_string()
            });

        let prompt = template
            .replace(INPUT_TOKEN, input)
            .replace(README_TOKEN, &readme_content)
            .replace(SPEC_SCHEMA_TOKEN, &spec_schema_content)
            .replace(RUBRICS_TOKEN, &rubrics_content);

        eprintln!("[moeb] generating specification (up to {} attempt(s))...", retry_limit);

        let cfg = MoebConfig::load().unwrap_or_default();
        let adapter_name = cfg.active_adapter.clone().unwrap_or_default();
        let adapter_cfg = cfg.adapter_config(&adapter_name);
        let model = adapter_cfg.effective_model("unknown");

        let trace_config = TraceConfig {
            command: TraceCommand::Spec,
            spec: format!("spec-{}", sanitize_slug(input)),
            adapter: adapter_name,
            model,
            retention: cfg.effective_run_retention(),
            file_content_mode,
        };
        let trace = Arc::new(TraceContext::new(trace_config));
        let ai = self.factory.build(Arc::clone(&trace))?;

        let schema = load_validation_schema(working_dir);
        let required_sections: Vec<String> = schema
            .as_ref()
            .map(|s| s.body.required_sections.clone())
            .unwrap_or_else(|| REQUIRED_SECTIONS.iter().map(|s| s.to_string()).collect());

        let mut last_err: anyhow::Error = anyhow::anyhow!("no attempts made");
        let mut total_attempts = 0u32;

        let (domain, slug, status, supersedes, body) = 'retry: {
            for attempt in 1..=retry_limit {
                total_attempts = attempt;
                trace.current_attempt.store(attempt, std::sync::atomic::Ordering::SeqCst);

                let state = crate::run_state::new_shared_run_state();
                let tools = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&state)).definitions();
                let executor = crate::tools::RealToolExecutor::new(std::sync::Arc::clone(&state));
                let initial_messages = vec![crate::adapters::Message::User(prompt.clone())];
                let compaction_config = crate::agent::CompactionConfig {
                    enabled: cfg.effective_compaction_enabled(),
                    threshold: cfg.effective_compaction_threshold(),
                    keep_turns: cfg.effective_compaction_keep_turns(),
                };
                let raw = match crate::agent::run_agent_loop_traced(
                    ai.as_ref(),
                    &executor,
                    &tools,
                    working_dir,
                    initial_messages,
                    MAX_TURNS,
                    &trace,
                    attempt,
                    compaction_config,
                    state,
                ) {
                    Ok(r) => r,
                    Err(e) => return Err(e),
                };

                if raw.is_empty() {
                    return Err(anyhow::anyhow!("Agent returned an empty response."));
                }

                let result = parse_frontmatter(&raw)
                    .and_then(|(domain, slug, status, supersedes, body)| {
                        validate_sections(&body, &required_sections)?;
                        Ok((domain, slug, status, supersedes, body))
                    });

                match result {
                    Ok(parsed) => break 'retry parsed,
                    Err(e) => {
                        eprintln!("[moeb] spec attempt {}/{} failed: {}", attempt, retry_limit, e);
                        last_err = e;
                    }
                }
            }
            trace.set_total_attempts(total_attempts);
            if let Err(e) = trace.finalize(TraceOutcome::Failure, Some(last_err.to_string())) {
                eprintln!("[moeb] warning: trace could not be saved: {}", e);
            }
            bail!("spec generation failed after {} attempt(s). Last error: {}", retry_limit, last_err);
        };

        trace.set_total_attempts(total_attempts);
        if let Err(e) = trace.finalize(TraceOutcome::Success, None) {
            eprintln!("[moeb] warning: trace could not be saved: {}", e);
        }

        if status == "draft" {
            eprintln!("[moeb] note: spec has status 'draft' and is not yet considered governing.");
        }
        for (path, decision) in &supersedes {
            eprintln!("[moeb] spec supersedes: {} — {}", path, decision);
        }

        // Forward-compatibility guard: warn if the JSON schema requires a field the kernel
        // has no dedicated parser for yet.
        if let Some(ref s) = schema {
            let known_fields: std::collections::HashSet<&str> =
                ["domain", "slug", "status", "supersedes"].iter().copied().collect();
            for required_field in &s.frontmatter.required {
                if !known_fields.contains(required_field.as_str()) {
                    eprintln!(
                        "[moeb] warning: schema requires frontmatter field '{}' \
                         but the kernel has no parser for it yet.",
                        required_field
                    );
                }
            }
        }

        let spec_dir = working_dir.join("specifications").join(&domain);
        fs::create_dir_all(&spec_dir).with_context(|| {
            format!("Failed to create directory {}", spec_dir.display())
        })?;

        let filename = format!("{}.{}.md", domain, slug);
        let path = spec_dir.join(&filename);
        fs::write(&path, &body)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        println!(
            "Created: .moeb/specifications/{}/{}",
            domain, filename
        );

        self.link_readme(&domain, &filename, working_dir, file_content_mode)?;

        Ok(())
    }

    fn link_readme(
        &self,
        domain: &str,
        filename: &str,
        working_dir: &Path,
        file_content_mode: FileContentMode,
    ) -> Result<()> {
        let asset = Prompts::get(README_LINK_PROMPT_FILE)
            .context("Embedded prompt template 'readme-link.prompt' not found in binary")?;
        let template = std::str::from_utf8(asset.data.as_ref())
            .context("readme-link.prompt is not valid UTF-8")?;

        let spec_path = format!("specifications/{}/{}", domain, filename);
        let prompt = template
            .replace("{{spec_path}}", &spec_path)
            .replace("{{domain}}", domain);

        eprintln!("[moeb] linking specification in README...");

        let noop_trace = Arc::new(TraceContext::new(TraceConfig {
            command: TraceCommand::Spec,
            spec: String::new(),
            adapter: String::new(),
            model: String::new(),
            retention: 0,
            file_content_mode,
        }));
        let ai = self.factory.build(Arc::clone(&noop_trace))?;
        let link_state = crate::run_state::new_shared_run_state();
        let tools = crate::tools::ToolRegistry::standard(std::sync::Arc::clone(&link_state)).definitions();
        let executor = crate::tools::RealToolExecutor::new(std::sync::Arc::clone(&link_state));
        let initial_messages = vec![crate::adapters::Message::User(prompt)];
        let _ = crate::agent::run_agent_loop_traced(
            ai.as_ref(),
            &executor,
            &tools,
            working_dir,
            initial_messages,
            MAX_TURNS,
            &noop_trace,
            1,
            crate::agent::CompactionConfig::default(),
            link_state,
        )?;
        println!("Updated: .moeb/README.md");
        Ok(())
    }
}

fn sanitize_slug(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
        .chars()
        .take(40)
        .collect()
}

// ── Frontmatter parsing ───────────────────────────────────────────────────────

fn parse_frontmatter(content: &str) -> Result<(String, String, String, Vec<(String, String)>, String)> {
    let content = content.trim_start();

    if !content.starts_with("---") {
        bail!("Response does not begin with a '---' frontmatter block.");
    }

    let after_open = content.trim_start_matches("---").trim_start_matches('\n');
    let close = after_open
        .find("\n---")
        .context("Frontmatter block is not closed with '---'.")?;

    let fm_text = &after_open[..close];
    let body = after_open[close..].trim_start_matches("\n---").trim_start_matches('\n');

    let mut domain = None;
    let mut slug = None;
    let mut status: Option<String> = None;
    let mut supersedes: Vec<(String, String)> = Vec::new();
    let mut in_supersedes = false;
    let mut current_path: Option<String> = None;
    let mut current_decision: Option<String> = None;

    for line in fm_text.lines() {
        if line.trim_start() == "supersedes:" {
            in_supersedes = true;
            continue;
        }
        if in_supersedes {
            if !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('-') {
                // Flush any incomplete pair with a warning before leaving the block.
                if current_path.is_some() || current_decision.is_some() {
                    eprintln!(
                        "[moeb] warning: incomplete supersedes entry (path={:?}, decision={:?}) skipped.",
                        current_path, current_decision
                    );
                    current_path = None;
                    current_decision = None;
                }
                in_supersedes = false;
                // Fall through: this line may carry another top-level key.
                // Re-process it as a scalar field below.
            } else if let Some(val) = line.trim_start_matches([' ', '-']).strip_prefix("path:") {
                // Flush any complete pair before starting a new one.
                match (current_path.take(), current_decision.take()) {
                    (Some(p), Some(d)) => supersedes.push((p, d)),
                    (Some(_), None) | (None, Some(_)) => {
                        eprintln!(
                            "[moeb] warning: incomplete supersedes entry skipped (missing path or decision)."
                        );
                    }
                    (None, None) => {}
                }
                current_path = Some(val.trim().to_string());
                continue;
            } else if let Some(val) = line.trim_start().strip_prefix("decision:") {
                current_decision = Some(val.trim().trim_matches('"').to_string());
                continue;
            } else {
                continue;
            }
        }
        if let Some(val) = line.strip_prefix("domain:") {
            domain = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("slug:") {
            slug = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("status:") {
            status = Some(val.trim().to_string());
        }
    }
    // Flush the last supersedes pair.
    match (current_path, current_decision) {
        (Some(p), Some(d)) => supersedes.push((p, d)),
        (Some(_), None) | (None, Some(_)) => {
            eprintln!(
                "[moeb] warning: incomplete supersedes entry skipped (missing path or decision)."
            );
        }
        (None, None) => {}
    }

    let domain = domain.context("Frontmatter missing 'domain' field.")?;
    let slug = slug.context("Frontmatter missing 'slug' field.")?;
    let status = status.context("Frontmatter missing 'status' field.")?;

    if domain.is_empty() {
        bail!("Frontmatter 'domain' field is empty.");
    }
    if slug.is_empty() {
        bail!("Frontmatter 'slug' field is empty.");
    }

    const VALID_STATUSES: &[&str] = &["active", "superseded", "draft"];
    if !VALID_STATUSES.contains(&status.as_str()) {
        bail!(
            "Frontmatter 'status' field has invalid value '{}'. \
             Must be one of: active, superseded, draft.",
            status
        );
    }

    Ok((domain, slug, status, supersedes, body.to_string()))
}

// ── Section validation ────────────────────────────────────────────────────────

fn validate_sections(body: &str, required: &[impl AsRef<str>]) -> Result<()> {
    let mut remaining = required.iter().peekable();

    for line in body.lines() {
        let Some(expected) = remaining.peek() else {
            break;
        };
        if line.trim_start().starts_with(expected.as_ref()) {
            remaining.next();
        }
    }

    if let Some(missing) = remaining.peek() {
        bail!(
            "Required section missing or out of order: '{}'",
            missing.as_ref()
        );
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "spec_integration_tests.rs"]
mod integration_tests;
