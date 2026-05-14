# moeb

A declarative specification harness engine for AI-driven development. Write a requirement as a specification, then let `moeb run` drive an AI agent to implement it. Every decision is recorded as an immutable spec; the harness enforces consistency as the project evolves.

This repository is self-hosted — moeb is governed and built by itself using the harness in [`.moeb/`](../.moeb/README.md).

---

## Install

Download the latest binary from the Releases tab and place it on your `$PATH`.

```sh
curl -L https://github.com/OWNER/REPO/releases/latest/download/moeb-VERSION-linux-x86_64 -o moeb
chmod +x moeb
sudo mv moeb /usr/local/bin/
```

## Quickstart

```sh
# Bootstrap a harness in your project
moeb init

# Configure an AI adapter
moeb use anthropic

# Author a new specification
moeb spec "add user authentication"

# Implement it
moeb run .moeb/specifications/auth/auth.user-auth-flow.md
```

## Commands

| Command | Description |
|---------|-------------|
| `moeb init` | Bootstrap a `.moeb/` harness directory in the current project |
| `moeb use <adapter>` | Configure and activate an AI provider (`anthropic` or `openai`) |
| `moeb spec <input>` | Drive an AI agent to produce a conformant specification |
| `moeb run <spec>` | Drive an AI agent to implement the next steps of a specification |
| `moeb adapters` | List all configured adapters and their state |
| `moeb adapter <name> config KEY VALUE` | Set per-adapter configuration (model, retries, timeout) |
| `moeb configure` | Set kernel-level configuration values |
| `moeb replay <trace>` | Replay a previous run deterministically from a saved trace |

## Harness

The [`.moeb/`](../.moeb/README.md) directory is the governance layer: immutability policies, the specification schema, and a full index of every recorded decision. All changes to this project — including to moeb itself — flow through specifications authored and implemented using moeb.
