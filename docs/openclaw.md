# OpenClaw Support

Search across OpenClaw agent session history with the `--openclaw` flag.

## How it works

OpenClaw stores sessions in `~/.openclaw/agents/<agent>/sessions/*.jsonl`. No additional setup needed — just use the `--openclaw` flag.

## Usage

```bash
# Search OpenClaw sessions (always deep search — no index files)
search-sessions "security audit" --openclaw

# Limit results
search-sessions "email" --openclaw --limit 5

# Search a different agent (default is "main")
search-sessions "query" --openclaw --agent other
```

## Example Output

```
============================================================
  DEEP SEARCH (OPENCLAW): "security audit"
  17 matches found
============================================================

  [1] [USER] (no summary)
      Project:  ~/.openclaw/workspace
      Date:     2026-02-03 17:00
      Snippet:  ...daily-security-audit] Perform your daily security audit...
      Session:  329ca9d8-a90c-4c34-add7-d680c8c67937

============================================================
```

## OpenClaw vs Claude Code

| Aspect | Claude Code | OpenClaw |
|--------|-------------|----------|
| Path | `~/.claude/projects/<project>/` | `~/.openclaw/agents/<agent>/sessions/` |
| Index | `sessions-index.json` per project | None (deep search only) |
| Message format | `"type": "user"/"assistant"` | `"type": "message"` with nested `role` |

## Performance

Tested on 14 sessions, 15 MB of JSONL:

- Specific queries: **~50 ms**
- Common words: **~130 ms**
