# Rubrics Catalogue

A trait-keyed catalogue of conditionally applicable rubric criteria. Entries are selected
by the spec agent based on traits detected in the raw requirement and description of the
specification being authored. They are not auto-injected — selection is agent-driven.

Each entry carries:
- **Domain** — the harness domain this criterion belongs to.
  Used as a coarse pre-filter before trait matching; agents skip entries whose domain
  is unrelated to the specification being authored.
- **Traits** — comma-separated tags that trigger selection within the domain (e.g. `ai-adapter`).
- **Applies At** — comma-separated list of command names (e.g. `spec`, `run`, `spec, run`)
  or the special value `All` (every moeb command). Governs how the entry is applied per
  command: `spec` governs spec authoring quality for this invocation; any other command
  name causes the row to be copied into the spec's `## Rubric` section for that command's
  agent to verify during implementation.

To retire a criterion, set its `status` to `superseded` and add a note identifying the
replacement. Do not delete rows.

## Criteria

| id | Name | Description | Threshold | Pass Condition | Domain | Traits | Applies At | Status |
|----|------|-------------|-----------|----------------|--------|--------|------------|--------|
| `adapter-structural-parity` | Adapter implementations are structurally identical | `AnthropicAdapter::send` and `OpenAiAdapter::send` follow the same retry loop skeleton; only API-specific serialisation differs | Identical structure | Code review of both adapter files side-by-side finds no structural asymmetry | `moeb` | `ai-adapter` | `run` | active |
