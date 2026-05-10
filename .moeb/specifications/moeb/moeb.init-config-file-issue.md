**Domain**: `moeb`

**Filename**: `moeb.init-config-file-issue.md`

**Path**: `specifications/moeb/moeb.init-config-file-issue.md`

**Registration**: Add this specification to the Specification index under the `moeb` domain in the README.md.

**Content for the Specification File**:

```markdown
# Specification for Moeb Init Configuration File Issue

## Introduction

This specification addresses an issue identified in the `moeb init` command where a `config.toml` file is being incorrectly generated inside the `.moeb/` directory.

## Background

The `moeb init` command is designed to initialize the Moeb project setup, creating necessary configurations and directories. However, it has been observed that a `config.toml` file is being placed in the `.moeb` directory, which contravenes the desired behavior.

## Requirements

### Expected Behavior

- The `moeb init` command should not create a `config.toml` file within the `.moeb/` directory.
- Exactly what files and configurations are meant to be produced should be outlined and verified against the existing harness specifications.

### Resolution Steps

1. Analyze the `moeb init` implementation to establish where the `config.toml` is being generated and why.
2. Modify the implementation so that `config.toml` is not created in `.moeb`.
3. Review and update tests to ensure this behavior is verified.

## Considerations

- Verify this change does not affect any other part of the initialization process.
- Ensure that documentation reflects this correct setup process.

## Impact

Correcting this bug prevents unnecessary file creation, ensuring the `.moeb/` directory only contains files explicitly required by the harness infrastructure.

## References

- Original Specification: [Moeb Kernel](specifications/moeb/moeb.kernel.md)
- Hexagonal Architecture Refactoring: [Moeb Hexagonal Architecture](specifications/moeb/moeb.hex-architecture.md)

```