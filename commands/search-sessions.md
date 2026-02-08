---
description: "Search across all past Claude Code sessions by metadata or full message content"
usage: '/search-sessions "query" [--deep] [--limit N] [--project FILTER] [--openclaw] [--agent NAME]'
---

# Search Sessions

Search across all past Claude Code and OpenClaw session history.

## Prerequisites

The `search-sessions` binary must be installed and on PATH. Install via:
- `brew install sinzin91/tap/search-sessions`
- `cargo install search-sessions`

## Modes

- **Index search (default)**: Searches session metadata (summary, firstPrompt, projectPath, gitBranch). Near-instant (~18ms).
- **Deep search (`--deep`)**: Searches actual message text. Sub-second (~280ms with ripgrep, ~1s without).

## Options

- `--deep` — Search full message content instead of just metadata
- `--limit N` — Maximum results to show (default: 20)
- `--project FILTER` — Filter to sessions from projects matching this substring
- `--openclaw` — Search OpenClaw sessions instead of Claude Code
- `--agent NAME` — OpenClaw agent to search (default: main)

## Examples

```bash
# Index search (instant)
/search-sessions "kubernetes RBAC"

# Deep search
/search-sessions "auth flow" --deep

# Filter by project
/search-sessions "billing" --project noc0

# Limit results
/search-sessions "docker compose" --deep --limit 5

# OpenClaw sessions
/search-sessions "security audit" --openclaw
```

## Output

Results include session UUID for resuming:

```
[1] Kubernetes RBAC configuration
    Project:  ~/Projects/myapp
    Date:     2026-01-28 15:30
    Session:  7897c935-2069-4b75-bbad-a3fac62ea59c
```

User can resume with: `claude --resume 7897c935-2069-4b75-bbad-a3fac62ea59c`

!search-sessions {{$1}}

Present the results to the user. If index search returned no results, suggest trying `--deep`. If deep search returned no results, suggest refining the query.
