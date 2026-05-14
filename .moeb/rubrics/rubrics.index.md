# Rubrics Index

A catalogue of named standard rubric criteria. Specifications that apply a criterion
should copy its row verbatim into their `## Rubric / ### Structured` table and use the
criterion `id` as the Name column value.

To retire a criterion, set its `status` to `superseded` and add a note in the
description identifying the criterion that replaces it. Do not delete rows.

## Criteria

| id | Name | Description | Threshold | Pass Condition | Status |
|----|------|-------------|-----------|----------------|--------|
| `binary-builds` | Binary builds cleanly | `cargo build --release` completes without error | Zero errors | CI build exits 0 | active |
| `all-tests-pass` | All unit tests pass | `cargo test` completes without failure | Zero failures | `cargo test` exits 0 | active |
| `no-test-regression` | No existing test regression | All tests present before this change pass without modification to test code | Zero failures | `cargo test` exits 0; no test file edited | active |
| `no-drift` | No contradiction with parent specs | The implementation does not violate any decision recorded in a linked parent specification | Zero contradictions | Manual review of every decision in every parent spec listed in Backlinks | active |
| `spec-schema-compliance` | Spec conforms to schema | All required frontmatter fields and body sections are present and correctly ordered | 100% of required fields and sections | Validation in `domain/spec.rs` exits 0 during `moeb spec` | active |
| `adapter-structural-parity` | Adapter implementations are structurally identical | `AnthropicAdapter::send` and `OpenAiAdapter::send` follow the same retry loop skeleton; only API-specific serialisation differs | Identical structure | Code review of both adapter files side-by-side finds no structural asymmetry | active |
