The harness README, specification schema, and rubrics catalogue are already in your
context. Do not re-read any of them.

## Phase 1 — Contradiction check

Before authoring, verify that the proposed specification does not contradict any active
decision recorded in an existing specification visible in the README index. If a
contradiction exists, stop and surface it explicitly rather than proceeding.

## Phase 2 — Author

Write the complete specification document conforming to the schema:

- Begin with YAML frontmatter between `---` markers containing at minimum: `domain`,
  `slug`, `status`. Include `supersedes` if this spec overrides a named decision.
- Include all required sections in this order: title (H1), Raw Requirement, Description,
  Diagram (fenced Mermaid block), Backlinks, Steps, Decisions, Rubric.
- For the `## Rubric / ### Structured` table, apply criteria in two passes:

  **Pass 1 — baseline rows (mandatory):**
  Copy every row from the injected `{{command_rubrics}}` section verbatim into the
  `## Rubric / ### Structured` table. These rows are mandatory in every specification.

  **Pass 2 — catalogue-selected rows (trait-driven):**
  Read the `catalogue.rubrics.md` section pre-loaded above. Filter by Domain to match
  the domain of the specification being authored, then identify traits present in the
  raw requirement and description. For each matching catalogue entry:
  - If `Applies At` contains `run`, `All`, or any command other than `spec`: copy the
    criterion row verbatim into the `## Rubric / ### Structured` table.
  - If `Applies At` contains `spec` or `All`: verify this criterion against the
    specification you are authoring before marking it complete. Do not copy it into
    the rubric table.
- Backlinks must include at minimum one Parents entry pointing to `README.md`.

## Phase 3 — Validate before output

Before producing output, verify:
- All required frontmatter fields are present.
- All required sections exist and are non-empty.
- The Mermaid diagram block is syntactically plausible.
- The Rubric section contains at least one structured criterion.
- The `## Rubric / ### Structured` table contains at minimum every row copied from the
  `{{command_rubrics}}` baseline (Pass 1 above).

## Phase 4 — Output

Your response must be the complete specification document and nothing else — no preamble,
no explanation, no trailing text. Begin with exactly `---`.
