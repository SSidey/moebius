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

---

## Schema reference

All specification files must conform to the structure defined in [`spec-schema.yaml`](./spec-schema.yaml).