use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::assets::Prompts;
use crate::ports::AiPort;

const PROMPT_FILE: &str = "spec.prompt";
const INPUT_TOKEN: &str = "{{input}}";

const REQUIRED_SECTIONS: &[&str] = &[
    "# ",           // level-1 title
    "## Raw Requirement",
    "## Description",
    "```mermaid",
    "## Backlinks",
    "## Steps",
    "## Decisions",
    "## Rubric",
];

pub struct SpecService {
    ai: Arc<dyn AiPort>,
}

impl SpecService {
    pub fn new(ai: Arc<dyn AiPort>) -> Self {
        Self { ai }
    }

    pub fn run(&self, input: &str) -> Result<()> {
        let asset = Prompts::get(PROMPT_FILE)
            .context("Embedded prompt template 'spec.prompt' not found in binary")?;
        let template = std::str::from_utf8(asset.data.as_ref())
            .context("spec.prompt is not valid UTF-8")?;

        let prompt = template.replace(INPUT_TOKEN, input);

        let working_dir = Path::new(".moeb");
        if !working_dir.exists() {
            bail!(".moeb/ not found. Run `moeb init` first.");
        }

        let raw = crate::agent::run_agent_loop(self.ai.as_ref(), &prompt, working_dir)?;

        if raw.is_empty() {
            bail!("Agent returned an empty response.");
        }

        let (domain, slug, body) = parse_frontmatter(&raw).with_context(|| {
            format!(
                "Could not parse frontmatter from agent output.\n\nRaw output:\n{}",
                raw
            )
        })?;

        validate_sections(&body).with_context(|| {
            format!("Spec failed schema validation.\n\nRaw output:\n{}", raw)
        })?;

        let spec_dir = Path::new(".moeb/specifications").join(&domain);
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
        Ok(())
    }
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

    // Frontmatter parsing

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

    // Section validation

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
        // Put Decisions before Steps
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
