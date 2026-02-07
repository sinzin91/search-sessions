# Claude Code Skill Setup

Set up `/search-sessions` as a slash command in Claude Code.

## Install the binary

```bash
mkdir -p ~/.claude/bin/search-sessions/target/release
cp target/release/search-sessions ~/.claude/bin/search-sessions/target/release/
```

## Create the skill file

```bash
cat > ~/.claude/commands/search-sessions.md << 'EOF'
---
description: "Search across all past Claude Code sessions by metadata or full message content"
usage: '/search-sessions "query" [--deep] [--limit N] [--project FILTER]'
---

# Search Sessions

Search across all past Claude Code session history.

**Modes:**
- **Index search (default)**: Searches session metadata (summary, firstPrompt, projectPath, gitBranch). Near-instant.
- **Deep search (`--deep`)**: Searches actual message text via ripgrep. Sub-second.

**Options:**
- `--deep` — Search full message content
- `--limit N` — Maximum results (default: 20)
- `--project FILTER` — Filter to projects matching substring

**Examples:**
```
/search-sessions "kubernetes RBAC"
/search-sessions "auth flow" --deep
/search-sessions "billing" --project noc0
```

!~/.claude/bin/search-sessions/target/release/search-sessions {{$1}}

Present the results to the user. If index search returned no results, suggest trying `--deep`.
EOF
```

## Usage

```
/search-sessions "your query"
```
