# Declarative Harness

> This harness governs the **moeb** project. For a project overview, installation, and usage see [`.github/README.md`](../.github/README.md).

A declarative specification harness for maintaining a consistent, coherent and structured record of requirements and design decisions that govern changes to the solution layer.

---

## Policies

The following policies govern all interaction with this harness. They apply to both human authors and agents operating on this repository.

**No drift.** Every specification must remain consistent with its parent specifications and any decisions they record. A child spec may not introduce behaviour that contradicts an inherited decision. If a proposed change would cause drift, it must not proceed.

**No modification of existing specifications.** Specifications are immutable once authored. An agent must never edit an existing specification file. If a requirement changes, a new specification must be created that supersedes the old one, with a backlink recording the relationship.

**Contradictions require human intervention.** If a proposed specification contradicts an existing decision — whether in a parent spec or elsewhere in the harness — the agent must stop, surface the contradiction explicitly, and wait for a human to resolve it before proceeding.

**Status discipline.** Every specification carries a `status` field in its frontmatter with one of the values `active`, `superseded`, or `draft`. When a superseding specification is registered in this index, the agent performing that registration must also update the superseded specification's index row to `superseded` in the same authoring action. A specification whose README row reads `superseded` remains immutable on disk and retains its standing as a historical record; it no longer governs the codebase.

---

## Repository layers

This repository has two distinct layers with different roles.

**Meta-layer.** `README.md`, `spec-schema.yaml`, and `specifications/` are the harness infrastructure. They govern how changes are made. Agents must not land code changes inside any of these files or directories when implementing a specification.

**Target layer.** `src/` is where all code produced from specifications must be placed — whether new or replacing code that previously existed at the repository root.

**No inferred destinations.** If a specification does not explicitly reference `src/` as the destination for its artifacts, the agent must stop and seek clarification rather than choosing an alternative location.

---

## Specification requirements

All specification files must conform to the following conventions.

**Schema.** Every specification must be authored according to the structure defined in [`spec-schema.yaml`](./spec-schema.yaml). No field defined as required in the schema may be omitted. A derived machine-readable validation schema is maintained at [`spec-schema-validation.json`](./spec-schema-validation.json); both files must be updated in the same authoring action whenever the schema changes. `spec-schema.yaml` is the source of truth; `spec-schema-validation.json` is derived from it.

**Location.** Specifications are stored under `specifications/<domain>/`. The domain folder name must be a single lowercase word or hyphenated phrase representing the feature or concern area.

**Naming convention.** Specification filenames must follow the pattern `<domain>.<slug>.md` where `<domain>` matches the containing folder name and `<slug>` is a concise kebab-case description of the specification's subject.

> Examples: `specifications/auth/auth.token-rotation.md`, `specifications/payments/payments.refund-flow.md`

**Registration.** Every specification must be registered in the [Specification index](#specification-index) at the time it is first authored. Registration must not be deferred to a later implementation step inside the specification. A specification that is not registered here is not considered part of the harness.

**Supersedes.** When a specification overrides a named decision recorded in a parent specification, it must include a `supersedes:` block in its YAML frontmatter identifying each overridden decision by its path and decision title. Prose rationale for the override must still appear in the Decisions section of the new specification. The frontmatter entry is the machine-readable companion to that prose; both are required when a decision is being overridden.

**Rubrics.** A catalogue of named standard rubric criteria is maintained at
[`.moeb/rubrics/rubrics.index.md`](./rubrics/rubrics.index.md). Specifications must
include their own `## Rubric` section. Where a standard named criterion from the
catalogue applies, its row should be copied verbatim into the spec's structured rubric
table with the criterion `id` as the Name value. Criteria specific to the spec are
listed as additional rows. The rubrics index is a mutable harness document and is not
subject to the immutability policy.

---

## How to use this harness

All specifications are authored according to [`spec-schema.yaml`](./spec-schema.yaml) and stored under `specifications/<domain>/`. The `src/` directory contains all artifacts produced from those specifications.

Agents should read this file first to orient, then follow links to the relevant specification before taking action.

---

## Specification index

Organised by domain. Add a new `###` subsection for each domain as it is introduced. Each row represents one specification file registered in the harness.

| Column | Content |
|--------|---------|
| **Name** | The human-readable title of the specification |
| **Description** | A single sentence describing the specification's subject and scope |
| **Path** | The relative path to the specification file from the repo root |
| **Status** | Lifecycle state: `active`, `superseded`, or `draft` |

### harness

| Name | Description | Path | Status |
|------|-------------|------|--------|
| Declarative Specification Harness | Base harness structure, policies, schema, and naming conventions governing the specification system | [specifications/harness/harness.base-harness.md](specifications/harness/harness.base-harness.md) | active |
| README Scope Boundary Clarification | Adds an explicit repository-layer statement to README.md distinguishing the harness meta-layer from the src/ target layer | [specifications/harness/harness.readme-scope-boundary.md](specifications/harness/harness.readme-scope-boundary.md) | active |
| Registration at Creation | Requires that README index registration is atomic with spec file creation, eliminating the deferred-registration pattern | [specifications/harness/harness.registration-at-creation.md](specifications/harness/harness.registration-at-creation.md) | active |
| Specifications Directory Rename and .moeb/ Path Resolution | Renames harness/ to specifications/ throughout the project and updates moeb run prompt paths to resolve correctly from the project root after moeb init | [specifications/harness/harness.specifications-dir-rename.md](specifications/harness/harness.specifications-dir-rename.md) | active |
| Specification Status Field | Adds a required `status` frontmatter field (`active`, `superseded`, `draft`) to every specification and a Status column to the README index, making spec lifecycle state machine-readable and keeping superseded rows marked atomically with the registration of their successor | [specifications/harness/harness.spec-status.md](specifications/harness/harness.spec-status.md) | active |
| Formal Supersedes Field | Adds an optional structured `supersedes` frontmatter block that machine-readably records which named decision in a parent spec is being overridden, complementing the existing prose convention in the Decisions section | [specifications/harness/harness.supersedes-field.md](specifications/harness/harness.supersedes-field.md) | active |
| Schema Split: Documentation and Validation | Splits `spec-schema.yaml` into a human/agent authoring guide and a derived `spec-schema-validation.json` that the kernel loads at validation time, replacing hardcoded section and field lists with schema-driven values | [specifications/harness/harness.schema-split.md](specifications/harness/harness.schema-split.md) | active |
| Rubrics Index | Introduces `.moeb/rubrics/rubrics.index.md` as a mutable catalogue of named standard rubric criteria that specifications reference by id, ensuring consistent wording and preventing silent omissions of common quality gates | [specifications/harness/harness.rubrics-index.md](specifications/harness/harness.rubrics-index.md) | active |

### moeb

| Name | Description | Path | Status |
|------|-------------|------|--------|
| Moeb Kernel | Rust CLI kernel implementing moeb init, moeb use, moeb spec, and moeb run with an AI agent loop and per-project .moeb/ harness directory | [specifications/moeb/moeb.kernel.md](specifications/moeb/moeb.kernel.md) | active |
| Moeb Hexagonal Architecture | The Moeb kernel must be restructured using hexagonal (also known as ports and adapters) architecture | [specifications/moeb/moeb.hex-architecture.md](specifications/moeb/moeb.hex-architecture.md) | active |
| Moeb Init Configuration File Issue | Resolves an issue where `moeb init` inappropriately creates `config.toml` in the `.moeb/` directory | [specifications/moeb/moeb.init-config-file-issue.md](specifications/moeb/moeb.init-config-file-issue.md) | active |
| Moeb Init Retains Schema in Binary and Copies Only README | Update `moeb init` so it no longer materialises `spec-schema.yaml` into the generated `.moeb/` directory during initialisation. | [specifications/moeb/moeb.init-retain-schema-in-binary-copy-readme-only.md](specifications/moeb/moeb.init-retain-schema-in-binary-copy-readme-only.md) | active |
| Ensure Bundling of Prompt Template Files with Moeb Binary | Specifies the bundling process to include prompt template files with the moeb binary. | [specifications/moeb/moeb.prompt-template-bundling.md](specifications/moeb/moeb.prompt-template-bundling.md) | active |
| Binary Release and Semantic Versioning | This specification defines how the moeb project produces and publishes release artifacts for the binary, and how each released binary is versioned using semantic versioning. | [specifications/moeb/moeb.binary-release-and-semver.md](specifications/moeb/moeb.binary-release-and-semver.md) | active |
| Spec Command Output Enforcement and File Persistence | Updates spec.prompt to enforce schema compliance, validates AI output against spec-schema.yaml, and writes the result to the correct .moeb/specifications path | [specifications/moeb/moeb.spec-output-enforcement.md](specifications/moeb/moeb.spec-output-enforcement.md) | active |
| Moeb run and spec update to ensure linking and automatic files | moeb run must automatically create or update files as required in the specification supplied, moeb spec must cause a link in the README.md to be created | [specifications/moeb/moeb.specification-update-and-readme-linking.md](specifications/moeb/moeb.specification-update-and-readme-linking.md) | active |
| AI File Modification Detection | This specification outlines the implementation of an AI-based mechanism for determining necessary file modifications without requiring the entire repository to be processed. This involves integrating specific AI tools capable of analyzing file relevance based on provided requirements. The process should generate a change-diff when files cannot be directly modified, optimizing efficiency in file handling within the `moeb run` process. | [specifications/moeb/moeb.ai-file-modification-detection.md](specifications/moeb/moeb.ai-file-modification-detection.md) | active |
| Spec Validation Retry with User-Visible Error Reporting | When spec validation fails, retry up to a user-configured limit printing each error to the user, and strengthen spec.prompt to reduce frontmatter omission | [specifications/moeb/moeb.spec-retry-on-validation-failure.md](specifications/moeb/moeb.spec-retry-on-validation-failure.md) | active |
| Adapter Configuration, Release, and Listing | Adds moeb adapters (list all adapters and state), moeb adapter <name> config KEY VALUE (set per-adapter model and retries), and moeb adapter <name> release (remove credentials); extends moeb use to print a config summary | [specifications/moeb/moeb.adapter-config-and-listing.md](specifications/moeb/moeb.adapter-config-and-listing.md) | active |
| Anthropic Claude Adapter | Adds an AnthropicAdapter implementing AiPort via the Anthropic Messages API, registers anthropic in all KNOWN_ADAPTERS lists, and integrates with moeb use, moeb adapters, and moeb adapter config/release | [specifications/moeb/moeb.anthropic-adapter.md](specifications/moeb/moeb.anthropic-adapter.md) | active |
| Anthropic Adapter Timeout and Transport Error Retry | Fixes operation-timed-out failures by adding a configurable per-request timeout (TIMEOUT key, default 600 s) and extending the retry loop to cover transport-level errors alongside HTTP 429 and 5xx responses | [specifications/moeb/moeb.anthropic-adapter-timeout-retry.md](specifications/moeb/moeb.anthropic-adapter-timeout-retry.md) | active |
| Kernel Adapter Rate Limiting with Exponential Backoff | Replaces the fixed 1-second retry delay in both adapters with exponential backoff and jitter, adds Retry-After header respect on 429 responses, emits a stderr warning when provider quota is nearly exhausted, and fixes OpenAI transport error retry | [specifications/moeb/moeb.kernel-rate-limiting.md](specifications/moeb/moeb.kernel-rate-limiting.md) | active |
| Use Configured Adapter Without Re-entering Credentials | When an adapter is already configured, `moeb use <adapter>` skips re-prompting for the secret or allows pressing Enter to keep the existing key, then switches the active adapter | [specifications/moeb/moeb.use-configured-adapter.md](specifications/moeb/moeb.use-configured-adapter.md) | active |
| Agent File-Read Optimization | Eliminates the mandatory first-turn API round-trip in `moeb run` by pre-loading README and spec into the initial prompt, and adds a `read_files` batch tool to reduce subsequent round-trips | [specifications/moeb/moeb.agent-read-optimization.md](specifications/moeb/moeb.agent-read-optimization.md) | active |
| Moeb Run Produces No File Writes When Using the Anthropic Adapter | Fixes premature agent loop termination when the Anthropic adapter returns a planning text turn before tool calls, and reinforces run.prompt to prevent preamble narration | [specifications/moeb/moeb.run-anthropic-no-file-writes.md](specifications/moeb/moeb.run-anthropic-no-file-writes.md) | active |
| Trace Capture, Replay, and Kernel Configuration | Adds always-on JSON trace capture for moeb run and moeb spec, a moeb configure command for kernel-level config (RUN_RETENTION, LOG_FILE_CONTENT), and a moeb replay command that deterministically replays a trace using AI and tool stubs | [specifications/moeb/moeb.trace-and-replay.md](specifications/moeb/moeb.trace-and-replay.md) | active |
| Run Stability: Trace Finalize Visibility and File Read Truncation | Replaces silent `let _ = trace.finalize(...)` calls with stderr warnings and adds a 100 KiB per-file cap to `read_file` and `read_files` results to prevent unbounded context growth | [specifications/moeb/moeb.trace-finalize-and-read-cap.md](specifications/moeb/moeb.trace-finalize-and-read-cap.md) | active |
| Targeted File Reads: Line-Range Access Tool | Adds a `read_file_range` tool that returns only a requested line range (capped at 300 lines), and updates `run.prompt` to prefer it over `read_files` for targeted lookups after `grep_files` identifies a line number | [specifications/moeb/moeb.read-file-range.md](specifications/moeb/moeb.read-file-range.md) | active |
| Shared Constants and Type Aliases | Publishes `MAX_TURNS` as a public constant from `agent.rs` and consolidates the `AdapterFactory` type alias into `adapters/mod.rs`, eliminating duplicate definitions in `domain/spec.rs` and `domain/run.rs` | [specifications/moeb/moeb.shared-constants.md](specifications/moeb/moeb.shared-constants.md) | active |
| Adapter Factory Port | Introduces `AdapterFactoryPort` in `ports/` and `DefaultAdapterFactory` in `adapters/`, removing all direct imports of concrete adapter types from the domain layer and correcting the hexagonal architecture violation | [specifications/moeb/moeb.adapter-factory-port.md](specifications/moeb/moeb.adapter-factory-port.md) | active |
| Tool Executor Extraction | Extracts all seven file tools from `agent.rs` into a `tools/` module with a `ToolHandler` trait and `ToolRegistry`, moves `ToolExecutorPort` to `ports/`, and migrates tests to their respective tool files | [specifications/moeb/moeb.tool-executor-extraction.md](specifications/moeb/moeb.tool-executor-extraction.md) | active |
| Content Deduplication for File Reads | Adds a per-run in-memory sha256 cache to `RealToolExecutor` that returns a backreference message instead of re-sending unchanged `read_file` content, and adds `cache_hit: bool` to `ToolCallEvent` | [specifications/moeb/moeb.content-deduplication.md](specifications/moeb/moeb.content-deduplication.md) | active |
| Prompt Caching | Adds `PROMPT_CACHE` kernel config (default true), applies Anthropic ephemeral cache_control to the system prompt, reads cached token counts from both adapters, and emits `CacheUsageEvent` to the trace | [specifications/moeb/moeb.prompt-caching.md](specifications/moeb/moeb.prompt-caching.md) | active |
| Prompt Cache Status Hint in Adapters Output | Extends `moeb adapters` to append a one-line `moeb configure PROMPT_CACHE` hint below the cache status line so users are self-serviced on how to disable or re-enable caching | [specifications/moeb/moeb.prompt-cache-adapters-hint.md](specifications/moeb/moeb.prompt-cache-adapters-hint.md) | active |
| Windows Release Target | Extends the GitHub Actions release workflow to build and publish a Windows (`x86_64-pc-windows-msvc`) binary alongside the existing Linux artifact using a build matrix, with platform-suffixed asset names | [specifications/moeb/moeb.windows-release-target.md](specifications/moeb/moeb.windows-release-target.md) | active |
| Spec Prompt: Static File Pre-load and Redundant-Read Prevention | Pre-loads README.md, spec-schema.yaml, and rubrics/rubrics.index.md into the `moeb spec` initial prompt via template substitution, eliminating the rubrics double-path bug and mandatory first-turn reads, and adds a no-re-read directive covering all three read tools | [specifications/moeb/moeb.spec-prompt-preload.md](specifications/moeb/moeb.spec-prompt-preload.md) | active |
| Moeb Spec: .moeb Root Boundary and src/ Output Location | This specification corrects the repository boundary model used by `moeb spec` and related harness reads. | [specifications/moeb/moeb.moeb-spec-moeb-root-boundary.md](specifications/moeb/moeb.moeb-spec-moeb-root-boundary.md) | active |
| OpenAI Adapter: Direct File Writes and Specification Iteration | Removes the patch-file escape hatch from run.prompt and implements write_file_dispatched loop-continuation tracking in agent.rs so the OpenAI adapter writes files directly and iterates through all specification steps before terminating | [specifications/moeb/moeb.openai-direct-file-writes.md](specifications/moeb/moeb.openai-direct-file-writes.md) | active |
| OpenAI Adapter Rubrics and Non-Regression Preservation | This specification restores the OpenAI adapter change process to a safe, compile-preserving, and regression-aware workflow. The implementation must retain all code and tests needed for successful compilation and runtime behaviour, limit edits strictly to the files and logic required by the OpenAI adapter specification, and execute the relevant rubric checks for the specification before considering the change complete. Existing behaviour outside the scope of the OpenAI adapter change must remain intact. | [specifications/moeb/moeb.openai-adapter-rubrics-and-non-regression.md](specifications/moeb/moeb.openai-adapter-rubrics-and-non-regression.md) | active |
| Run Prompt: Non-Regression and Rubric Verification Enforcement | Adds two additive instructions to run.prompt — a harness constraint bullet that prohibits deleting code not targeted by the active specification, and a rubric verification step that requires the agent to confirm each structured rubric criterion before completing a run | [specifications/moeb/moeb.run-prompt-non-regression-and-rubric-verification.md](specifications/moeb/moeb.run-prompt-non-regression-and-rubric-verification.md) | active |
| Run Prompt: Hard Rules for Minimal-Diff File Writes and Test Preservation | Adds a HARD RULES block to run.prompt with four numbered critical-failure rules: softened test preservation, minimum-diff discipline, byte-for-byte function scope, and pre-write declaration of changes | [specifications/moeb/moeb.run-prompt-hard-rules.md](specifications/moeb/moeb.run-prompt-hard-rules.md) | active |
| Run-Time File Scope Enforcement | Extends `RealToolExecutor` to track which paths have been read via `read_file` or `read_files`; rejects `write_file` calls on existing files that have not been read during the current run, preventing agents from overwriting files they never read | [specifications/moeb/moeb.run-file-scope-enforcement.md](specifications/moeb/moeb.run-file-scope-enforcement.md) | active |
| Ollama Adapter | Add a new local-model AI adapter for Ollama that fits the existing adapter architecture used by the OpenAI and Anthropic providers. The implementation should support configuring and selecting Ollama as an adapter, route chat requests to a locally running Ollama instance, and preserve the existing retry, tracing, and tool-loop behavior expected by the kernel. | [specifications/moeb/moeb.ollama-adapter.md](specifications/moeb/moeb.ollama-adapter.md) | active |

### vcs

| Name | Description | Path | Status |
|------|-------------|------|--------|
| Git Initialisation and Initial Commit | Initialises git source control, establishes the .gitignore, creates the initial commit, and settles the policy that all subsequent commits are user-driven | [specifications/vcs/vcs.git-init.md](specifications/vcs/vcs.git-init.md) | active |

---

## Schema reference

All specification files must conform to the structure defined in [`spec-schema.yaml`](./spec-schema.yaml).
