use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::adapters::anthropic::AnthropicAdapter;
use crate::adapters::openai::OpenAiAdapter;
use crate::assets::Prompts;
use crate::config::MoebConfig;
use crate::ports::AiPort;
use crate::trace::{
    FileContentMode, TraceCommand, TraceConfig, TraceContext, TraceOutcome,
};

const PROMPT_FILE: &str = "spec.prompt";
const README_LINK_PROMPT_FILE: &str = "readme-link.prompt";
const INPUT_TOKEN: &str = "{{input}}";

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

type AdapterFactory = Arc<dyn Fn(Arc<TraceContext>) -> Result<Arc<dyn AiPort>> + Send + Sync>;

pub struct SpecService {
    ai_factory: AdapterFactory,
}

impl SpecService {
    /// For production use.
    pub fn from_config() -> Self {
        Self {
            ai_factory: Arc::new(|trace| {
                let cfg = MoebConfig::load().unwrap_or_default();
                let name = cfg.active_adapter.clone().unwrap_or_default();
                build_traced_adapter(&name, trace)
            }),
        }
    }

    /// For tests: wraps a pre-built adapter.
    pub fn new(ai: Arc<dyn AiPort>) -> Self {
        Self {
            ai_factory: Arc::new(move |_trace| Ok(Arc::clone(&ai))),
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

        let prompt = template.replace(INPUT_TOKEN, input);

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
        let ai = (self.ai_factory)(Arc::clone(&trace))?;

        let mut last_err: anyhow::Error = anyhow::anyhow!("no attempts made");
        let mut total_attempts = 0u32;

        let (domain, slug, body) = 'retry: {
            for attempt in 1..=retry_limit {
                total_attempts = attempt;
                trace.current_attempt.store(attempt, std::sync::atomic::Ordering::SeqCst);

                let tools = crate::agent::file_tools();
                let executor = crate::agent::RealToolExecutor::new(
                    Arc::clone(&trace),
                    file_content_mode,
                    attempt,
                );
                let initial_messages = vec![crate::adapters::Message::User(prompt.clone())];
                let raw = match crate::agent::run_agent_loop_traced(
                    ai.as_ref(),
                    &executor,
                    &tools,
                    working_dir,
                    initial_messages,
                    50,
                    &trace,
                    attempt,
                ) {
                    Ok(r) => r,
                    Err(e) => return Err(e),
                };

                if raw.is_empty() {
                    return Err(anyhow::anyhow!("Agent returned an empty response."));
                }

                let result = parse_frontmatter(&raw)
                    .and_then(|(domain, slug, body)| {
                        validate_sections(&body)?;
                        Ok((domain, slug, body))
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
            let _ = trace.finalize(TraceOutcome::Failure, Some(last_err.to_string()));
            bail!("spec generation failed after {} attempt(s). Last error: {}", retry_limit, last_err);
        };

        trace.set_total_attempts(total_attempts);
        let _ = trace.finalize(TraceOutcome::Success, None);

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
        let ai = (self.ai_factory)(Arc::clone(&noop_trace))?;
        let tools = crate::agent::file_tools();
        let executor = crate::agent::RealToolExecutor::new(
            Arc::clone(&noop_trace),
            file_content_mode,
            1,
        );
        let initial_messages = vec![crate::adapters::Message::User(prompt)];
        let _ = crate::agent::run_agent_loop_traced(
            ai.as_ref(),
            &executor,
            &tools,
            working_dir,
            initial_messages,
            50,
            &noop_trace,
            1,
        )?;
        println!("Updated: .moeb/README.md");
        Ok(())
    }
}

fn build_traced_adapter(adapter_name: &str, trace: Arc<TraceContext>) -> Result<Arc<dyn AiPort>> {
    match adapter_name {
        "openai" => Ok(Arc::new(OpenAiAdapter::from_secrets_and_config_with_trace(trace)?)),
        "anthropic" => Ok(Arc::new(AnthropicAdapter::from_secrets_and_config_with_trace(trace)?)),
        "" => anyhow::bail!("No adapter configured. Run `moeb use <adapter>` first."),
        other => anyhow::bail!(
            "Adapter '{}' is not recognised. Run `moeb use <adapter>` to reconfigure.",
            other
        ),
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

fn parse_frontmatter(content: &str) -> Result<(String, String, String)> {
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

    for line in fm_text.lines() {
        if let Some(val) = line.strip_prefix("domain:") {
            domain = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("slug:") {
            slug = Some(val.trim().to_string());
        }
    }

    let domain = domain.context("Frontmatter missing 'domain' field.")?;
    let slug = slug.context("Frontmatter missing 'slug' field.")?;

    if domain.is_empty() {
        bail!("Frontmatter 'domain' field is empty.");
    }
    if slug.is_empty() {
        bail!("Frontmatter 'slug' field is empty.");
    }

    Ok((domain, slug, body.to_string()))
}

// ── Section validation ────────────────────────────────────────────────────────

fn validate_sections(body: &str) -> Result<()> {
    let mut remaining = REQUIRED_SECTIONS.iter().peekable();

    for line in body.lines() {
        let Some(&expected) = remaining.peek() else {
            break;
        };
        if line.trim_start().starts_with(expected) {
            remaining.next();
        }
    }

    if let Some(&missing) = remaining.peek() {
        bail!(
            "Required section missing or out of order: '{}'",
            missing
        );
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_body() -> String {
        [
            "# My Specification",
            "",
            "## Raw Requirement",
            "The requirement text.",
            "",
            "## Description",
            "Technical approach.",
            "",
            "```mermaid",
            "graph TD",
            "  A --> B",
            "```",
            "",
            "## Backlinks",
            "None.",
            "",
            "## Steps",
            "### Step 1",
            "Do something.",
            "",
            "## Decisions",
            "### Decision 1",
            "Chose X.",
            "",
            "## Rubric",
            "### Structured",
            "Criteria here.",
        ]
        .join("\n")
    }

    fn valid_doc(domain: &str, slug: &str) -> String {
        format!(
            "---\ndomain: {}\nslug: {}\n---\n{}",
            domain,
            slug,
            valid_body()
        )
    }

    #[test]
    fn parse_frontmatter_extracts_domain_and_slug() {
        let (domain, slug, _) = parse_frontmatter(&valid_doc("moeb", "my-spec")).unwrap();
        assert_eq!(domain, "moeb");
        assert_eq!(slug, "my-spec");
    }

    #[test]
    fn parse_frontmatter_strips_block_from_body() {
        let (_, _, body) = parse_frontmatter(&valid_doc("moeb", "my-spec")).unwrap();
        assert!(!body.contains("domain:"), "body must not contain frontmatter");
        assert!(body.contains("# My Specification"));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_delimiter() {
        let err = parse_frontmatter("# No frontmatter here").unwrap_err();
        assert!(err.to_string().contains("---"));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_domain() {
        let doc = "---\nslug: my-spec\n---\n# Title\n";
        let err = parse_frontmatter(doc).unwrap_err();
        assert!(err.to_string().contains("domain"));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_slug() {
        let doc = "---\ndomain: moeb\n---\n# Title\n";
        let err = parse_frontmatter(doc).unwrap_err();
        assert!(err.to_string().contains("slug"));
    }

    #[test]
    fn validate_sections_passes_for_valid_body() {
        validate_sections(&valid_body()).unwrap();
    }

    #[test]
    fn validate_sections_rejects_missing_rubric() {
        let body = valid_body().replace("## Rubric\n", "");
        let err = validate_sections(&body).unwrap_err();
        assert!(err.to_string().contains("Rubric"));
    }

    #[test]
    fn validate_sections_rejects_missing_mermaid() {
        let body = valid_body().replace("```mermaid\n", "");
        let err = validate_sections(&body).unwrap_err();
        assert!(
            err.to_string().contains("mermaid"),
            "got: {}",
            err
        );
    }

    #[test]
    fn validate_sections_rejects_wrong_order() {
        let body = valid_body()
            .replace(
                "## Steps\n### Step 1\nDo something.\n\n## Decisions\n### Decision 1\nChose X.",
                "## Decisions\n### Decision 1\nChose X.\n\n## Steps\n### Step 1\nDo something.",
            );
        let err = validate_sections(&body).unwrap_err();
        assert!(
            err.to_string().contains("Steps") || err.to_string().contains("Decisions"),
            "got: {}",
            err
        );
    }
}

// ── Integration tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::adapters::{AgentResponse, Message, ToolCall, ToolDef};
    use crate::ports::AiPort;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn spec_body() -> String {
        [
            "# Token Rotation Spec",
            "",
            "## Raw Requirement",
            "Rotate tokens.",
            "",
            "## Description",
            "Technical approach.",
            "",
            "```mermaid",
            "graph TD",
            "  A --> B",
            "```",
            "",
            "## Backlinks",
            "None.",
            "",
            "## Steps",
            "### Step 1",
            "Do it.",
            "",
            "## Decisions",
            "### Decision 1",
            "Chose X.",
            "",
            "## Rubric",
            "### Structured",
            "Criteria.",
        ]
        .join("\n")
    }

    fn spec_doc(domain: &str, slug: &str) -> String {
        format!("---\ndomain: {}\nslug: {}\n---\n{}", domain, slug, spec_body())
    }

    struct MockAi {
        responses: Mutex<VecDeque<AgentResponse>>,
    }

    impl MockAi {
        fn new(responses: Vec<AgentResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into_iter().collect()),
            })
        }
    }

    impl AiPort for MockAi {
        fn send(&self, _: &[Message], _: &[ToolDef]) -> anyhow::Result<AgentResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockAi: no more queued responses"))
        }
    }

    fn setup_harness(tmp: &tempfile::TempDir) {
        let dir = tmp.path();
        fs::create_dir_all(dir.join("specifications")).unwrap();
        fs::write(dir.join("README.md"), "# Harness\n").unwrap();
        fs::write(dir.join("spec-schema.yaml"), "schema: placeholder\n").unwrap();
    }

    #[test]
    fn run_in_creates_spec_file_at_correct_path() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::Text("Registered.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 1, FileContentMode::Embed).unwrap();

        let spec_path = tmp.path().join("specifications/auth/auth.token-rotation.md");
        assert!(spec_path.exists(), "spec file must be created");
        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.starts_with("# Token Rotation Spec"), "body must not contain frontmatter");
    }

    #[test]
    fn run_in_readme_updated_when_agent_writes_it() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let updated_readme = "# Harness\n\n| Token Rotation | Rotate tokens | [specifications/auth/auth.token-rotation.md](specifications/auth/auth.token-rotation.md) |\n";
        let write_args = serde_json::json!({
            "path": "README.md",
            "content": updated_readme,
        })
        .to_string();

        let ai = MockAi::new(vec![
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::ToolCalls(vec![ToolCall {
                id: "c1".to_string(),
                name: "write_file".to_string(),
                arguments: write_args,
            }]),
            AgentResponse::Text("Done.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 1, FileContentMode::Embed).unwrap();

        let readme = fs::read_to_string(tmp.path().join("README.md")).unwrap();
        assert!(
            readme.contains("specifications/auth/auth.token-rotation.md"),
            "README must contain the spec link"
        );
    }

    #[test]
    fn run_in_retries_on_validation_failure() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text("No frontmatter here, just prose.".to_string()),
            AgentResponse::Text(spec_doc("auth", "token-rotation")),
            AgentResponse::Text("Registered.".to_string()),
        ]);
        SpecService::new(ai).run_in("rotate tokens", tmp.path(), 2, FileContentMode::Embed).unwrap();

        let spec_path = tmp.path().join("specifications/auth/auth.token-rotation.md");
        assert!(spec_path.exists(), "spec file must be created after retry");
    }

    #[test]
    fn run_in_fails_after_exhausting_retries() {
        let tmp = tempfile::tempdir().unwrap();
        setup_harness(&tmp);

        let ai = MockAi::new(vec![
            AgentResponse::Text("No frontmatter — attempt 1.".to_string()),
            AgentResponse::Text("No frontmatter — attempt 2.".to_string()),
        ]);
        let err = SpecService::new(ai)
            .run_in("rotate tokens", tmp.path(), 2, FileContentMode::Embed)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed after 2 attempt(s)"),
            "expected exhaustion message, got: {}",
            msg
        );
    }
}
