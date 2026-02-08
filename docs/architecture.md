# Architecture

## Claude Code session storage

Claude Code stores session data in `~/.claude/projects/`:

```
~/.claude/projects/
├── -Users-you-Projects-foo/
│   ├── sessions-index.json      # Metadata: summaries, dates, branches
│   ├── abc123.jsonl             # Full session transcript
│   └── def456.jsonl
├── -Users-you-Projects-bar/
│   ├── sessions-index.json
│   └── ...
```

- **`sessions-index.json`**: Small JSON files with session metadata — `summary`, `firstPrompt`, `created`, `modified`, `gitBranch`, `projectPath`, `messageCount`
- **`*.jsonl`**: One JSON record per line — user messages, assistant responses, tool calls, file snapshots

## Search architecture

**Index search** (pure Rust): 
- Reads all `sessions-index.json` files
- Scores entries with weighted AND-matching (summary 3x, firstPrompt 2x, branch/path 1x)
- Sorts by score then recency
- **18ms** on 514 sessions

**Deep search** (Rust, optionally with ripgrep): 
- If ripgrep is available: invokes `rg` for SIMD-accelerated matching (~280ms)
- If not: uses pure Rust file scanning fallback (~1s)
- Parses matching lines to extract message text
- Generates snippets and cross-references with index metadata

## Why ripgrep (when available)?

Ripgrep is purpose-built for fast text search: SIMD string matching, memory-mapped I/O, and heavily optimized parallel file reading. 

On 1.6GB of JSONL, ripgrep deep search runs in **280ms** vs **~1s** for the pure Rust fallback. But the fallback means **no external dependencies required** — it just works out of the box.
