use anyhow::{bail, Context, Result};

pub(super) const REQUIRED_SECTIONS: &[&str] = &[
    "# ",
    "## Raw Requirement",
    "## Description",
    "```mermaid",
    "## Backlinks",
    "## Steps",
    "## Decisions",
    "## Rubric",
];

pub(super) fn sanitize_slug(input: &str) -> String {
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

pub(super) fn parse_frontmatter(content: &str) -> Result<(String, String, String, Vec<(String, String)>, String)> {
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

pub(super) fn validate_sections(body: &str, required: &[impl AsRef<str>]) -> Result<()> {
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
