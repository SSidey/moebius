# Declarative Harness

A declarative specification harness for maintaining a consistent, coherent and structured record of requirements and design decisions that govern changes to the solution layer.

---

## Policies

The following policies govern all interaction with this harness. They apply to both human authors and agents operating on this repository.

**No drift.** Every specification must remain consistent with its parent specifications and any decisions they record. A child spec may not introduce behaviour that contradicts an inherited decision. If a proposed change would cause drift, it must not proceed.

**No modification of existing specifications.** Specifications are immutable once authored. An agent must never edit an existing specification file. If a requirement changes, a new specification must be created that supersedes the old one, with a backlink recording the relationship.

**Contradictions require human intervention.** If a proposed specification contradicts an existing decision — whether in a parent spec or elsewhere in the harness — the agent must stop, surface the contradiction explicitly, and wait for a human to resolve it before proceeding.

---

## Repository layers

This repository has two distinct layers with different roles.

**Meta-layer.** `README.md`, `spec-schema.yaml`, and `specifications/` are the harness infrastructure. They govern how changes are made. Agents must not land code changes inside any of these files or directories when implementing a specification.

**Target layer.** `src/` is where all code produced from specifications must be placed — whether new or replacing code that previously existed at the repository root.

**No inferred destinations.** If a specification does not explicitly reference `src/` as the destination for its artifacts, the agent must stop and seek clarification rather than choosing an alternative location.

---

## Specification requirements

All specification files must conform to the following conventions.

**Schema.** Every specification must be authored according to the structure defined in [`spec-schema.yaml`](./spec-schema.yaml). No field defined as required in the schema may be omitted.

**Location.** Specifications are stored under `specifications/<domain>/`. The domain folder name must be a single lowercase word or hyphenated phrase representing the feature or concern area.

**Naming convention.** Specification filenames must follow the pattern `<domain>.<slug>.md` where `<domain>` matches the containing folder name and `<slug>` is a concise kebab-case description of the specification's subject.

> Examples: `specifications/auth/auth.token-rotation.md`, `specifications/payments/payments.refund-flow.md`

**Registration.** Every specification must be registered in the [Specification index](#specification-index) at the time it is first authored. Registration must not be deferred to a later implementation step inside the specification. A specification that is not registered here is not considered part of the harness.

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

### harness

| Name | Description | Path |
|------|-------------|------|
| Declarative Specification Harness | Base harness structure, policies, schema, and naming conventions governing the specification system | [specifications/harness/harness.base-harness.md](specifications/harness/harness.base-harness.md) |
| README Scope Boundary Clarification | Adds an explicit repository-layer statement to README.md distinguishing the harness meta-layer from the src/ target layer | [specifications/harness/harness.readme-scope-boundary.md](specifications/harness/harness.readme-scope-boundary.md) |
| Registration at Creation | Requires that README index registration is atomic with spec file creation, eliminating the deferred-registration pattern | [specifications/harness/harness.registration-at-creation.md](specifications/harness/harness.registration-at-creation.md) |
| Specifications Directory Rename and .moeb/ Path Resolution | Renames harness/ to specifications/ throughout the project and updates moeb run prompt paths to resolve correctly from the project root after moeb init | [specifications/harness/harness.specifications-dir-rename.md](specifications/harness/harness.specifications-dir-rename.md) |

### moeb

| Name | Description | Path |
|------|-------------|------|
| Moeb Kernel | Rust CLI kernel implementing moeb init, moeb use, moeb spec, and moeb run with an AI agent loop and per-project .moeb/ harness directory | [specifications/moeb/moeb.kernel.md](specifications/moeb/moeb.kernel.md) |
| Moeb Hexagonal Architecture | The Moeb kernel must be restructured using hexagonal (also known as ports and adapters) architecture | [specifications/moeb/moeb.hex-architecture.md](specifications/moeb/moeb.hex-architecture.md) |
| Moeb Init Configuration File Issue | Resolves an issue where `moeb init` inappropriately creates `config.toml` in the `.moeb/` directory | [specifications/moeb/moeb.init-config-file-issue.md](specifications/moeb/moeb.init-config-file-issue.md) |
| Ensure Bundling of Prompt Template Files with Moeb Binary | Specifies the bundling process to include prompt template files with the moeb binary. | [specifications/moeb/moeb.prompt-template-bundling.md](specifications/moeb/moeb.prompt-template-bundling.md) |
| Spec Command Output Enforcement and File Persistence | Updates spec.prompt to enforce schema compliance, validates AI output against spec-schema.yaml, and writes the result to the correct .moeb/specifications path | [specifications/moeb/moeb.spec-output-enforcement.md](specifications/moeb/moeb.spec-output-enforcement.md) |
| Moeb run and spec update to ensure linking and automatic files | moeb run must automatically create or update files as required in the specification supplied, moeb spec must cause a link in the README.md to be created|[specifications/moeb/moeb.specification-update-and-readme-linking.md](specifications/moeb/moeb.specification-update-and-readme-linking.md)|

### vcs

| Name | Description | Path |
|------|-------------|------|
| Git Initialisation and Initial Commit | Initialises git source control, establishes the .gitignore, creates the initial commit, and settles the policy that all subsequent commits are user-driven | [specifications/vcs/vcs.git-init.md](specifications/vcs/vcs.git-init.md) |

---

## Schema reference

All specification files must conform to the structure defined in [`spec-schema.yaml`](./spec-schema.yaml).