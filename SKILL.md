---
name: search-sessions
description: "Search across Claude Code, OpenClaw, and Obsidian session history. Use when you need to find past conversations, query sessions by date range, filter by project, search Obsidian vaults, or resume previous sessions by UUID."
---

# search-sessions

Fast CLI to search across all Claude Code, OpenClaw, and Obsidian session history. Find past decisions, code snippets, and resume any previous session.

**Announce at start:** "I'm using search-sessions to find that in your session history."

## Installation

```bash
# Homebrew (macOS/Linux)
brew install sinzin91/tap/search-sessions

# Cargo (Rust)
cargo install search-sessions
```

## Usage

### Index Search (default, instant ~18ms)

Searches session metadata — summaries, first prompts, project paths. Start here.

```bash
search-sessions "kubernetes RBAC"
search-sessions "auth migration" --project myapp
```

### Deep Search (full-text, ~280ms with ripgrep)

Searches actual message content. Use `--deep` if index search doesn't find it.

```bash
search-sessions "docker compose" --deep
search-sessions "that regex for parsing" --deep
```

### Date Range Filtering

Narrow results by time period with `--since`, `--until`, or `--date`.

```bash
search-sessions "deploy" --since "3 days ago"
search-sessions "auth" --since 2026-02-01 --until 2026-02-15
search-sessions "bug" --date today
search-sessions "refactor" --since "last week"
```

### OpenClaw Sessions

```bash
search-sessions "security audit" --openclaw
search-sessions "deploy review" --openclaw --agent main
```

### Obsidian Vault Search

Search markdown notes in an Obsidian vault with `--obsidian`.

```bash
search-sessions "meeting notes" --obsidian /path/to/vault
```

### Resume a Session

Results include the session UUID. Resume with:

```bash
claude --resume <session-uuid>
```

## Options

| Flag | Description |
|------|-------------|
| `--deep` | Search full message content (slower) |
| `--openclaw` | Search OpenClaw sessions instead of Claude Code |
| `--obsidian PATH` | Search an Obsidian vault at the given path |
| `--project FILTER` | Filter to projects matching substring |
| `--since DATE` | Show sessions from this date forward (e.g. "3 days ago", "2026-02-01") |
| `--until DATE` | Show sessions up to this date |
| `--date DATE` | Show sessions from a single day (e.g. "today") |
| `--limit N` | Max results (default: 20) |
| `--agent NAME` | OpenClaw agent to search (default: main) |
