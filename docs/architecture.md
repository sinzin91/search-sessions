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

**Index search** (Rust-native): 
- Reads all `sessions-index.json` files
- Scores entries with weighted AND-matching (summary 3x, firstPrompt 2x, branch/path 1x)
- Sorts by score then recency

**Deep search** (Rust + ripgrep): 
- Invokes `rg` for SIMD-accelerated string matching across all JSONL files
- Parses matching lines in Rust to extract message text
- Generates snippets and cross-references with index metadata

## Why hybrid?

An earlier pure-Rust deep search (using `rayon` + `BufReader`) clocked in at **1,118ms** — 3x slower than Python+ripgrep. 

Ripgrep is purpose-built for this: SIMD string matching, memory-mapped I/O, and heavily optimized parallel file reading. Rather than reimplement ripgrep, we shell out to it and handle the structured post-processing in Rust.
