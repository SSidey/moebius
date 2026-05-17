# Rubrics

This directory contains the project rubric files for this moeb-governed project. They
supplement the binary-bundled baseline criteria (injected automatically via
`{{command_rubrics}}`) to form the complete set of quality gates applied during `moeb run`
and `moeb spec` invocations.

## Files

### global.rubrics.md

Criteria that apply to every moeb command in this project (layer 3 in the five-layer
model). Add criteria here when they apply regardless of which command is being run.

### run.rubrics.md

Criteria that apply to every `moeb run` execution in this project (layer 4 for run).
Add build, test, and implementation quality gates here.

### spec.rubrics.md

Criteria that apply to every `moeb spec` invocation in this project (layer 4 for spec).
Add specification authoring quality gates here.

### catalogue.rubrics.md

A trait-keyed catalogue of conditionally applicable criteria (layer 5). Entries are not
auto-injected — the spec agent selects them based on traits detected in the specification
being authored.

Columns: `id`, `Name`, `Description`, `Threshold`, `Pass Condition`, `Domain`, `Traits`,
`Applies At`, `Status`. To retire a criterion, set `Status` to `superseded`; do not
delete rows.
