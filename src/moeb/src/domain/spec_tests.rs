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
