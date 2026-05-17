## Baseline run rubric criteria

The following criterion applies to every `moeb run` execution. Include it in your
`verify_rubrics` call along with any criteria in the specification's own `## Rubric`
section and any criteria in the project rubric section below.

| Name | Description | Threshold | Pass Condition |
|------|-------------|-----------|----------------|
| `context-budget` | All implementation files created or modified during this run are ≤ 300 lines; `*_tests.rs` companion files are ≤ 400 lines. If any file exceeds budget, refactor it before passing this criterion. | Zero over-budget files after any required refactoring | Agent checks line counts of every file written; refactors any over-budget file, then marks pass only after all files meet the budget |
