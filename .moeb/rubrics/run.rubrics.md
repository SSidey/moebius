## Project run rubric criteria

The following criteria apply to every `moeb run` execution in this project. Include them
in your `verify_rubrics` call along with the baseline criteria above and any criteria
in the specification's own `## Rubric` section.

| Name | Description | Threshold | Pass Condition |
|------|-------------|-----------|----------------|
| `binary-builds` | `cargo build --release` completes without error | Zero errors | CI build exits 0 |
| `all-tests-pass` | `cargo test` completes without failure | Zero failures | `cargo test` exits 0 |
| `no-test-regression` | All tests present before this change pass without modification to test code | Zero failures | `cargo test` exits 0; no test file edited |
