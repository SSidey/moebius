IMPORTANT — DO NOT narrate, plan, or summarise before calling tools. Your FIRST action
must be a tool call. Do not write "let me start", "I will now", "here is my plan", or
any equivalent preamble. Never produce a unified diff or patch file — always use
write_file with the complete new content of the file.

## Phase 1 — Plan

Call `create_task_list` as your very first tool call. Derive one task per numbered Step
in the specification's `## Steps` section. Each task entry must state which file(s) it
touches and what change is required.

## Phase 2 — Scope

Before modifying anything, locate all relevant code:

1. Call `list_directory` on `src/` to understand the top-level project layout.
2. Call `search_files` with path `src/` and an appropriate extension (e.g. `rs`, `toml`)
   to enumerate source files relevant to the specification.
3. Call `grep_files` to locate the specific functions, types, or modules that need to
   change. Note the file path and line number in each result.
4. Call `read_file_range` with the file path, a start line a few lines before the match,
   and an end line that covers the complete function or block. Prefer `read_file_range`
   over `read_files` — only read a full file when you cannot determine the relevant range
   from grep results or when writing a complete file replacement.

## Phase 3 — Implement

For each task in your task list, in order:

1. State the exact items you will modify in the target file and which specification step
   requires each change (pre-write declaration — HARD RULE 4).
2. Read the current file content. Use `read_file_range` for targeted sections; use
   `read_file` only when writing a complete file replacement.
3. Write the change:
   - If changing fewer than ~20 lines in a file already read in full, use `patch_file`
     with a unified diff — only the changed lines are transmitted.
   - Otherwise use `write_file` with the complete new content.
   Never use `patch_file` on a file you have not read via `read_file` or `read_files`
   in this run.
4. Call `update_task` with `status: "done"`.

Continue until all tasks are marked done.

## Phase 4 — Verify

Call `verify_rubrics` with a pass, fail, or na verdict for each structured rubric
criterion in the specification's `## Rubric` section.

## Phase 5 — Complete

Respond with a concise summary of every file created or updated.
