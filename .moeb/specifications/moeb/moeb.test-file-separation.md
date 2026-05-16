---
domain: moeb
slug: test-file-separation
status: active
---

# Test File Separation

## Raw Requirement

> Splitting tests outside of the code files, previously identified as best practice,
> these could be bloating the filesize unnecessarily. The current code structure of the
> rust kernel is written for human readability; given the file sizes for communication
> the code files deserve some refactoring so that we are not communicating overly large
> amounts of data to AI agents reading source files.

## Description

Rust source files carry their `#[cfg(test)]` blocks inline, which is idiomatic for
small unit tests but becomes a significant size penalty as test suites grow. An AI agent
reading `domain/spec.rs` to understand business logic must currently scan past ~196 lines
of test code to do so. Ten source files each carry more than 80 lines of test code inline.

This specification extracts the test block from every qualifying source file (≥ 80 lines
of test code) into a companion `<name>_tests.rs` file in the same directory, using Rust's
`#[path]` attribute to declare the external file as the module body. The companion file
retains full access to private items in the parent module via `use super::*;` or
`super::` paths, exactly as inline tests do today. The `#[cfg(test)]` guard remains on
the `mod` declaration in the source file, so companion test files are never compiled in
production builds.

The ten target files and their companion names are:

| Source file | Companion file(s) |
|-------------|-------------------|
| `domain/spec.rs` | `domain/spec_tests.rs`, `domain/spec_integration_tests.rs` |
| `config.rs` | `config_tests.rs` |
| `commands/adapter_management.rs` | `commands/adapter_management_tests.rs` |
| `adapters/anthropic.rs` | `adapters/anthropic_tests.rs` |
| `trace.rs` | `trace_tests.rs` |
| `tools/mod.rs` | `tools/tool_executor_tests.rs` |
| `agent.rs` | `agent_tests.rs` |
| `commands/use_cmd.rs` | `commands/use_cmd_tests.rs` |
| `adapters/retry.rs` | `adapters/retry_tests.rs` |
| `commands/configure.rs` | `commands/configure_tests.rs` |

Files with fewer than 80 lines of test code (`tools/read_file.rs`,
`tools/read_file_range.rs`, `tools/read_files.rs`, `tools/grep_files.rs`,
`tools/search_files.rs`, `adapters/openai.rs`, `commands/replay.rs`) are left unchanged.
`version_tests.rs` is already a separate file and requires no action.

## Diagram

```mermaid
graph TD
    subgraph Before["Before — tests inline in source file"]
        BF["domain/spec.rs\n────────────────────\npub fn validate_spec()\nfn parse_frontmatter()\n...\n#[cfg(test)] mod tests { ... }\n#[cfg(test)] mod integration_tests { ... }"]
    end

    subgraph After["After — tests in companion file"]
        AF["domain/spec.rs\n────────────────────\npub fn validate_spec()\nfn parse_frontmatter()\n...\n#[cfg(test)] #[path=\"spec_tests.rs\"] mod tests;\n#[cfg(test)] #[path=\"spec_integration_tests.rs\"] mod integration_tests;"]
        TF["domain/spec_tests.rs\n────────────────────\nuse super::*;\n// all tests from mod tests"]
        IF["domain/spec_integration_tests.rs\n────────────────────\nuse super::*;\n// all tests from mod integration_tests"]
    end

    Before -. "this spec" .-> After
    AF --- TF
    AF --- IF
```

## Backlinks

### Parents

| Label | Path | Purpose |
|-------|------|---------|
| Tool Executor Extraction | [specifications/moeb/moeb.tool-executor-extraction.md](specifications/moeb/moeb.tool-executor-extraction.md) | Established the convention of co-locating tests with the code they test; this spec refines that pattern by separating test code into companion files rather than inline blocks |
| README | [README.md](../../README.md) | Root index |

### External

*(none)*

## Steps

### Step 1 — Establish the transformation pattern

The transformation for a source file `foo.rs` with a single test module is:

**In `foo.rs`**, replace:
```rust
#[cfg(test)]
mod tests {
    BODY
}
```
with:
```rust
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

**Create `foo_tests.rs`** containing `BODY` verbatim (the contents of the original module
body, without the `mod tests { }` wrapper). All `use super::` and `use crate::` imports
from the original block are preserved unchanged.

For a source file with two test modules, each module gets its own companion file:
```rust
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "foo_integration_tests.rs"]
mod integration_tests;
```

### Step 2 — Extract `domain/spec.rs`

`domain/spec.rs` contains two test modules: `tests` and `integration_tests`.

1. Read `src/moeb/src/domain/spec.rs` in full.
2. Create `src/moeb/src/domain/spec_tests.rs` from the body of `mod tests { ... }`.
3. Create `src/moeb/src/domain/spec_integration_tests.rs` from the body of
   `mod integration_tests { ... }`.
4. Replace both inline blocks in `spec.rs` with `#[path]` declarations as described in
   Step 1.

### Step 3 — Extract `config.rs`

1. Read `src/moeb/src/config.rs` in full.
2. Create `src/moeb/src/config_tests.rs` from the body of its `mod tests { ... }` block.
3. Replace the inline block with the `#[path = "config_tests.rs"]` declaration.

### Step 4 — Extract `commands/adapter_management.rs`

1. Read `src/moeb/src/commands/adapter_management.rs` in full.
2. Create `src/moeb/src/commands/adapter_management_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "adapter_management_tests.rs"]`
   declaration.

### Step 5 — Extract `adapters/anthropic.rs`

1. Read `src/moeb/src/adapters/anthropic.rs` in full.
2. Create `src/moeb/src/adapters/anthropic_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "anthropic_tests.rs"]` declaration.

### Step 6 — Extract `trace.rs`

1. Read `src/moeb/src/trace.rs` in full.
2. Create `src/moeb/src/trace_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "trace_tests.rs"]` declaration.

### Step 7 — Extract `tools/mod.rs`

1. Read `src/moeb/src/tools/mod.rs` in full.
2. Create `src/moeb/src/tools/tool_executor_tests.rs` from its test module body.
3. Replace the inline block with:
   ```rust
   #[cfg(test)]
   #[path = "tool_executor_tests.rs"]
   mod tests;
   ```

### Step 8 — Extract `agent.rs`

1. Read `src/moeb/src/agent.rs` in full.
2. Create `src/moeb/src/agent_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "agent_tests.rs"]` declaration.

### Step 9 — Extract `commands/use_cmd.rs`

1. Read `src/moeb/src/commands/use_cmd.rs` in full.
2. Create `src/moeb/src/commands/use_cmd_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "use_cmd_tests.rs"]` declaration.

### Step 10 — Extract `adapters/retry.rs`

1. Read `src/moeb/src/adapters/retry.rs` in full.
2. Create `src/moeb/src/adapters/retry_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "retry_tests.rs"]` declaration.

### Step 11 — Extract `commands/configure.rs`

1. Read `src/moeb/src/commands/configure.rs` in full.
2. Create `src/moeb/src/commands/configure_tests.rs` from its test module body.
3. Replace the inline block with the `#[path = "configure_tests.rs"]` declaration.

### Step 12 — Verify

Run `cargo build --release` — zero errors. Run `cargo test` — all tests pass. Confirm
that none of the ten modified source files contain a `mod tests {` inline block:

```
grep -rn "^mod tests {" src/moeb/src/domain/spec.rs src/moeb/src/config.rs \
  src/moeb/src/commands/adapter_management.rs src/moeb/src/adapters/anthropic.rs \
  src/moeb/src/trace.rs src/moeb/src/tools/mod.rs src/moeb/src/agent.rs \
  src/moeb/src/commands/use_cmd.rs src/moeb/src/adapters/retry.rs \
  src/moeb/src/commands/configure.rs
```

This grep must return empty. Confirm each companion file exists on disk.

## Decisions

### Decision 1 — Same-directory companion files via `#[path]`, not `tests/` integration directory

**Rationale:** The test suites in these files call private functions (`parse_frontmatter`,
`compute_delay`, `prune_traces`, etc.) that are inaccessible from Rust's `tests/`
integration directory (which only sees public APIs). The `#[path]` attribute places the
companion file in the same module scope as the source file, preserving `super::` access
to all private items. This was the approach not considered when the tool-executor
extraction spec rejected the `tests/` directory; it offers the size benefit of separation
without the access restriction.

**Alternatives:**

| Option | Reason Rejected |
|--------|-----------------|
| Move tests to `tests/` integration directory | Cannot access private functions; would require making internal helpers `pub(crate)`, which changes the API surface |
| Keep tests fully inline | Status quo; source files remain large, burdening AI agents that need to read only business logic |
| Use `include!()` macro to inline a separate file | Less idiomatic than `#[path]`; file shows as included content in IDEs rather than a proper module |

**Consequences:** Each companion `*_tests.rs` file is a first-class Rust module (visible
in IDEs, navigable via `go-to-definition`) but is excluded from production builds by the
`#[cfg(test)]` guard on its `mod` declaration.

---

### Decision 2 — Threshold of 80 lines; files below threshold are left unchanged

**Rationale:** The goal is reducing the size of source files that agents read to
understand business logic. Files with fewer than 80 lines of test code represent a
smaller proportion of total file size and offer a lower return on the complexity of
separation. Keeping small test blocks inline preserves locality for minor sanity checks.

**Alternatives:**

| Option | Reason Rejected |
|--------|-----------------|
| Extract all test blocks regardless of size | Tools files like `read_file.rs` (24 lines of tests) would gain companion files for negligible size reduction |
| Use a percentage threshold (e.g., > 30% of file is tests) | Harder to reason about; requires calculating proportions rather than counting lines |

**Consequences:** The threshold is a judgement call, not a hard rule. If a currently
below-threshold file grows significantly in future, a follow-up spec may extract it.

---

### Decision 3 — Files with two test modules produce two companion files

**Rationale:** `domain/spec.rs` contains a `tests` module and an `integration_tests`
module. Merging them into a single companion file would require renaming one module or
nesting it under the other, changing test discovery output in `cargo test`. Two companion
files — `spec_tests.rs` and `spec_integration_tests.rs` — preserve module names and
therefore `cargo test domain::spec::tests::` filter patterns unchanged.

**Alternatives:**

| Option | Reason Rejected |
|--------|-----------------|
| Merge both into one companion file as nested sub-modules | Changes `cargo test` filter paths; developers must update any CI test filters |
| Rename `integration_tests` to a sub-module of `tests` | Module rename changes test output and may break CI patterns |

**Consequences:** `domain/spec.rs` acquires two `#[path]` declarations. Any future file
that grows a second test module follows the same two-companion-file pattern.

## Rubric

### Structured

| Name | Description | Threshold | Pass Condition |
|------|-------------|-----------|----------------|
| `binary-builds` | `cargo build --release` exits 0 | Zero errors | CI build exits 0 |
| `all-tests-pass` | `cargo test` exits 0 with identical test count to pre-change | Zero failures, same count | `cargo test` exits 0 |
| `no-inline-test-blocks` | None of the ten modified source files contain an inline `mod tests {` block | Zero matches | `grep` command in Step 12 returns empty |
| `companion-files-exist` | All eleven companion files are present on disk | Eleven files | Each path listed in the table in Description resolves to an existing file |
| `no-test-regression` | Every test that passed before this change passes after, with the same name | Zero regressions | `cargo test` output test names are identical before and after |

### Qualitative

- **Source files are smaller and focused:** After extraction, an agent reading a source file to understand business logic sees only production code. Test code is available in the companion file when explicitly requested.
- **Private access preserved:** No production function should need to change its visibility (`pub`, `pub(crate)`, etc.) as a result of this change. All private helpers accessed by tests remain private.
- **Behaviour-neutral refactor:** This change must not alter any test behaviour, test name, or test output. It is a structural reorganisation only.
