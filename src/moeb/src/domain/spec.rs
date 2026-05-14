use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
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

                let tools = crate::tools::ToolRegistry::standard().definitions();
                let executor = crate::tools::RealToolExecutor::new();
                let initial_messages = vec![crate::adapters::Message::User(prompt.clone())];
                let raw = match crate::agent::run_agent_loop_traced(
                    ai.as_ref(),
                    &executor,
                    &tools,
                    working_dir,
                    initial_messages,
                    MAX_TURNS,
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
        let tools = crate::tools::ToolRegistry::standard().definitions();
        let executor = crate::tools::RealToolExecutor::new();
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
            "---\ndomain: {}\nslug: {}\nstatus: active\n---\n{}",
            domain,
            slug,
            valid_body()
        )
    }

    #[test]
    fn parse_frontmatter_extracts_domain_and_slug() {
        let (domain, slug, _, _, _) = parse_frontmatter(&valid_doc("moeb", "my-spec")).unwrap();
        assert_eq!(domain, "moeb");
        assert_eq!(slug, "my-spec");
    }

    #[test]
    fn parse_frontmatter_strips_block_from_body() {
        let (_, _, _, _, body) = parse_frontmatter(&valid_doc("moeb", "my-spec")).unwrap();
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
    fn parse_frontmatter_rejects_missing_status() {
        let doc = "---\ndomain: moeb\nslug: my-spec\n---\n# Title\n";
        let err = parse_frontmatter(doc).unwrap_err();
        assert!(err.to_string().contains("status"));
    }

    #[test]
    fn parse_frontmatter_rejects_invalid_status() {
        let doc = "---\ndomain: moeb\nslug: my-spec\nstatus: pending\n---\n# Title\n";
        let err = parse_frontmatter(doc).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pending"), "error must reproduce the invalid value; got: {}", msg);
        assert!(msg.contains("active"), "error must list permitted values; got: {}", msg);
    }

    #[test]
    fn parse_frontmatter_accepts_all_valid_statuses() {
        for s in &["active", "superseded", "draft"] {
            let doc = format!("---\ndomain: moeb\nslug: my-spec\nstatus: {}\n---\n# Title\n", s);
            parse_frontmatter(&doc).unwrap_or_else(|e| panic!("status '{}' should be valid: {}", s, e));
        }
    }

    #[test]
    fn parse_frontmatter_extracts_supersedes_entries() {
        let doc = "---\ndomain: moeb\nslug: my-spec\nstatus: active\nsupersedes:\n  - path: specifications/moeb/moeb.old.md\n    decision: Decision 1 — Old approach\n  - path: specifications/moeb/moeb.other.md\n    decision: Decision 2 — Another one\n---\n# Title\n";
        let (_, _, _, supersedes, _) = parse_frontmatter(doc).unwrap();
        assert_eq!(supersedes.len(), 2);
        assert_eq!(supersedes[0].0, "specifications/moeb/moeb.old.md");
        assert_eq!(supersedes[0].1, "Decision 1 — Old approach");
        assert_eq!(supersedes[1].0, "specifications/moeb/moeb.other.md");
        assert_eq!(supersedes[1].1, "Decision 2 — Another one");
    }

    #[test]
    fn parse_frontmatter_absent_supersedes_gives_empty_vec() {
        let (_, _, _, supersedes, _) = parse_frontmatter(&valid_doc("moeb", "my-spec")).unwrap();
        assert!(supersedes.is_empty(), "no supersedes field should yield an empty vec");
    }

    #[test]
    fn validate_sections_passes_for_valid_body() {
        validate_sections(&valid_body(), REQUIRED_SECTIONS).unwrap();
    }

    #[test]
    fn validate_sections_rejects_missing_rubric() {
        let body = valid_body().replace("## Rubric\n", "");
        let err = validate_sections(&body, REQUIRED_SECTIONS).unwrap_err();
        assert!(err.to_string().contains("Rubric"));
    }

    #[test]
    fn validate_sections_rejects_missing_mermaid() {
        let body = valid_body().replace("```mermaid\n", "");
        let err = validate_sections(&body, REQUIRED_SECTIONS).unwrap_err();
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
        let err = validate_sections(&body, REQUIRED_SECTIONS).unwrap_err();
        assert!(
            err.to_string().contains("Steps") || err.to_string().contains("Decisions"),
            "got: {}",
            err
        );
    }

    #[test]
    fn validate_sections_uses_custom_required_list() {
        // Schema-driven: removing "## Rubric" from the required list should let a spec
        // without that section pass.
        let custom: Vec<String> = REQUIRED_SECTIONS
            .iter()
            .filter(|&&s| s != "## Rubric")
            .map(|s| s.to_string())
            .collect();
        let body = valid_body().replace("## Rubric\n### Structured\nCriteria here.", "");
        validate_sections(&body, &custom).unwrap();
    }

    #[test]
    fn load_validation_schema_returns_none_for_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("spec-schema-validation.json"), "not json").unwrap();
        let result = load_validation_schema(tmp.path());
        assert!(result.is_none(), "malformed JSON must return None");
    }

    #[test]
    fn load_validation_schema_returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_validation_schema(tmp.path());
        assert!(result.is_none(), "absent file must return None");
    }

    #[test]
    fn load_validation_schema_parses_valid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json = r###"{"frontmatter":{"required":["domain","slug","status"],"optional":["supersedes"]},"body":{"required_sections":["# ","## Raw Requirement"]}}"###;
        std::fs::write(tmp.path().join("spec-schema-validation.json"), json).unwrap();
        let schema = load_validation_schema(tmp.path()).expect("must parse valid JSON");
        assert_eq!(schema.frontmatter.required, ["domain", "slug", "status"]);
        assert_eq!(schema.body.required_sections, ["# ", "## Raw Requirement"]);
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
        format!("---\ndomain: {}\nslug: {}\nstatus: active\n---\n{}", domain, slug, spec_body())
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
