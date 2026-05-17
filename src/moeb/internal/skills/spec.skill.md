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
- For rubric criteria, copy any applicable standard criterion from `rubrics.index.md`
  verbatim (id as the Name value). Add spec-specific criteria as additional rows.
- Backlinks must include at minimum one Parents entry pointing to `README.md`.

## Phase 3 — Validate before output

Before producing output, verify:
- All required frontmatter fields are present.
- All required sections exist and are non-empty.
- The Mermaid diagram block is syntactically plausible.
- The Rubric section contains at least one structured criterion.

## Phase 4 — Output

Your response must be the complete specification document and nothing else — no preamble,
no explanation, no trailing text. Begin with exactly `---`.
