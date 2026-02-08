---
name: search-sessions
description: Search across Claude Code and OpenClaw session history. Use when you need to find past conversations, decisions, code snippets, or resume previous sessions.
---

# search-sessions

## Overview

Fast CLI to search across all Claude Code and OpenClaw session history. Find past decisions, code snippets, and resume any previous session.

**Announce at start:** "I'm using search-sessions to find that in your session history."

## When to Use

- User asks about something from a previous session
- Need to find past decisions, code, or context
- User wants to resume a previous conversation
- Searching for "that thing we discussed" or similar

## Installation

```bash
# Homebrew (macOS/Linux)
brew install sinzin91/tap/search-sessions

# Cargo (Rust)
cargo install search-sessions

# From source
git clone https://github.com/sinzin91/search-sessions
cd search-sessions && cargo build --release
```

## Usage

### Index Search (default, instant)

Searches session metadata — summaries, first prompts, project paths.

```bash
search-sessions "kubernetes RBAC"
search-sessions "auth migration" --project myapp
```

### Deep Search (full-text)

Searches actual message content. Use `--deep` flag.

```bash
search-sessions "docker compose" --deep
search-sessions "that regex for parsing" --deep
```

### OpenClaw Sessions

Search OpenClaw agent sessions with `--openclaw`.

```bash
search-sessions "security audit" --openclaw
```

### Resume a Session

Results include the session UUID. User can resume with:

```bash
claude --resume <session-uuid>
```

## Output Format

```
============================================================
  INDEX SEARCH: "kubernetes"
  3 matches found
============================================================

  [1] Kubernetes RBAC configuration
      Project:  ~/Projects/myapp
      Date:     2026-01-28 15:30
      Session:  7897c935-2069-4b75-bbad-a3fac62ea59c

  [2] ...
```

## Options

| Flag | Description |
|------|-------------|
| `--deep` | Search full message content (slower) |
| `--openclaw` | Search OpenClaw sessions instead of Claude Code |
| `--project FILTER` | Filter to projects matching substring |
| `--limit N` | Max results (default: 20) |
| `--agent NAME` | OpenClaw agent to search (default: main) |

## Tips

- Start with index search (no flags) — it's instant
- Use `--deep` only if index search doesn't find it
- Include project filter to narrow results
- Present session UUID so user can resume if needed
