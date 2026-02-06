# search-sessions

Search across all your Claude Code **and OpenClaw** session history. Fast.

## What it does

Claude Code stores session transcripts as JSONL files in `~/.claude/projects/`. OpenClaw stores them in `~/.openclaw/agents/<agent>/sessions/`. This tool searches across **all** of them — metadata and full message content — so you can find past decisions, code snippets, and recover context from any previous session.

## Install

### Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [ripgrep](https://github.com/BurntSushi/ripgrep) (`brew install ripgrep`) — used for deep search

### Build

```bash
git clone <this-repo> ~/Projects/search-sessions
cd ~/Projects/search-sessions
cargo build --release
```

### Set up as a Claude Code skill

Copy or symlink the binary and create the skill file:

```bash
# Create the bin directory if it doesn't exist
mkdir -p ~/.claude/bin/search-sessions/target/release

# Copy the binary
cp target/release/search-sessions ~/.claude/bin/search-sessions/target/release/

# Create the skill file
cat > ~/.claude/commands/search-sessions.md << 'EOF'
---
description: "Search across all past Claude Code sessions by metadata or full message content"
usage: '/search-sessions "query" [--deep] [--limit N] [--project FILTER]'
---

# Search Sessions

Search across all past Claude Code session history.

**Modes:**
- **Index search (default)**: Searches session metadata (summary, firstPrompt, projectPath, gitBranch). Near-instant.
- **Deep search (`--deep`)**: Searches actual user and assistant message text inside JSONL session files via ripgrep. Sub-second.

**Options:**
- `--deep` — Search full message content instead of just metadata
- `--limit N` — Maximum results to show (default: 20)
- `--project FILTER` — Filter to sessions from projects matching this substring

**Examples:**
```
/search-sessions "kubernetes RBAC"
/search-sessions "auth flow" --deep
/search-sessions "billing" --project noc0
/search-sessions "docker compose" --deep --limit 5
```

!~/.claude/bin/search-sessions/target/release/search-sessions {{$1}}

Present the results to the user. If index search returned no results, suggest trying `--deep`. If deep search returned no results, suggest refining the query.
EOF
```

Or just use it directly:

```bash
./target/release/search-sessions "your query here"
```

## Usage

### Index search (default — instant)

Searches session metadata: summaries, first prompts, project paths, and git branches.

```bash
search-sessions "kubernetes"
search-sessions "auth migration"
search-sessions "PR" --project noc0
```

```
============================================================
  INDEX SEARCH: "video"
  1 matches found
============================================================

  [1] Speed up three demo videos 3x with ffmpeg
      Project:  ~
      Date:     2026-01-24 01:20
      Messages: 9
      Matched:  summary
      Prompt:   can you help me make this video 3x faster ~/Downloads/noc0-demo.mov...
      Session:  77662775-dfd2-4a53-9877-4251530316d3

============================================================
  Tip: Use --deep to search inside message content.
============================================================
```

### Deep search (`--deep` — sub-second)

Searches the full text of every user and assistant message across all sessions. Uses ripgrep under the hood.

```bash
search-sessions "ffmpeg" --deep
search-sessions "docker compose" --deep --limit 5
search-sessions "RBAC" --deep --project noc0
```

```
============================================================
  DEEP SEARCH: "ffmpeg"
  5 matches found
============================================================

  [1] [ASST] Speed up three demo videos 3x with ffmpeg
      Project:  ~
      Date:     2026-01-24 01:20
      Snippet:  ...check if the file exists, then create a sped-up copy using ffmpeg.
      Session:  77662775-dfd2-4a53-9877-4251530316d3

  [2] [USER] Configure OpenClaw bot with Anthropic subscription and Ansible
      Project:  ~/Projects/claw-box
      Date:     2026-02-01 19:18
      Snippet:  ...apt packages to install at container startup (e.g. ffmpeg build-essential)...
      Session:  11b92556-235e-4ac5-8790-70fbc3d893f2

============================================================
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--deep` | Search full message content (uses ripgrep) | off (index only) |
| `--openclaw` | Search OpenClaw sessions instead of Claude Code | off |
| `--agent NAME` | OpenClaw agent to search | main |
| `--limit N` | Max results to display | 20 |
| `--project FILTER` | Only search sessions from projects matching this substring | all projects |

## OpenClaw Support

Search across your OpenClaw agent session history with the `--openclaw` flag.

### Setup

OpenClaw stores sessions in `~/.openclaw/agents/<agent>/sessions/*.jsonl`. No additional setup needed — just use the `--openclaw` flag.

### Usage

```bash
# Search OpenClaw sessions (always deep search — no index files)
search-sessions "security audit" --openclaw

# Limit results
search-sessions "email" --openclaw --limit 5

# Search a different agent (default is "main")
search-sessions "query" --openclaw --agent other
```

### Example Output

```
============================================================
  DEEP SEARCH (OPENCLAW): "security audit"
  17 matches found
============================================================

  [1] [USER] (no summary)
      Project:  ~/.openclaw/workspace
      Date:     2026-02-03 17:00
      Snippet:  ...daily-security-audit] Perform your daily security audit. Review: 1) All credentials and access you have...
      Session:  329ca9d8-a90c-4c34-add7-d680c8c67937

  [2] [ASST] (no summary)
      Project:  ~/.openclaw/workspace
      Date:     2026-02-03 17:01
      Snippet:  🔐 **Daily Security Audit Complete** **2 findings:** 1. ⚠️ **fastmail.sh is malformed**...
      Session:  329ca9d8-a90c-4c34-add7-d680c8c67937

============================================================
```

### OpenClaw Session Format

OpenClaw sessions differ from Claude Code:

| Aspect | Claude Code | OpenClaw |
|--------|-------------|----------|
| Path | `~/.claude/projects/<project>/` | `~/.openclaw/agents/<agent>/sessions/` |
| Index | `sessions-index.json` per project | None (deep search only) |
| Message type | `"type": "user"` or `"assistant"` | `"type": "message"` with nested `role` |

### Performance

Tested on 14 sessions, 15 MB of JSONL data:

- Specific queries: **~50 ms**
- Common words: **~130 ms**

### Use Cases

1. **Recover context after compaction** — Find what you discussed before context was truncated
2. **Cross-session memory** — Search decisions, code snippets, or ideas from any session
3. **Audit trail** — "Did I ever mention that API key?" or "What did the security audit find?"
4. **Find past conversations** — "What did we say about X?" across your entire history

## How it works

### Claude Code session storage

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

### Search architecture

**Index search** (Rust-native): Reads all `sessions-index.json` files, scores entries with weighted AND-matching (summary 3x, firstPrompt 2x, branch/path 1x), sorts by score then recency.

**Deep search** (Rust + ripgrep): Invokes `rg` for SIMD-accelerated string matching across all JSONL files, then parses matching lines in Rust to extract message text, generate snippets, and cross-reference with index metadata.

## Benchmarks

Tested on a real dataset: 514 session files, 1.6 GB total JSONL data, macOS.

| Mode | Python version | Rust version | Speedup |
|------|---------------|-------------|---------|
| **Index search** | 100 ms | 18 ms | **5.5x** |
| **Deep search** | 370 ms | 280 ms | **1.3x** |

Index search (the default mode) is **5.5x faster** — effectively instant at 18ms. Deep search gains are more modest since both versions use ripgrep for the heavy lifting; the Rust advantage comes from eliminating Python's ~80ms startup overhead.

### Why hybrid?

An earlier pure-Rust deep search (using `rayon` + `BufReader`) clocked in at **1,118ms** — 3x slower than Python+ripgrep. Ripgrep is purpose-built for this: SIMD string matching, memory-mapped I/O, and heavily optimized parallel file reading. Rather than reimplement ripgrep, we shell out to it and handle the structured post-processing in Rust.

## Comparison with alternatives

| Tool | Language | Search Method | Speed (deep) | Dependencies | Claude Code Skill | Notes |
|------|----------|---------------|-------------|-------------|-------------------|-------|
| **search-sessions** (this) | Rust + rg | Weighted index + ripgrep | **280 ms** | rg only | Yes (zero-config) | Hybrid architecture; instant index, sub-second deep |
| [cc-conversation-search](https://github.com/nicobailey/cc-conversation-search) | Python | SQLite FTS5 | ~500 ms | Python, SQLite | No | Full-text index; requires initial indexing step |
| [claude-history](https://github.com/4t2/claude-history) | Rust | Fuzzy matching (TUI) | ~400 ms | None | No | Interactive TUI with fuzzy finder; no CLI batch mode |
| [aichat claude-code-tools](https://github.com/aichat/claude-code-tools) | Python + Rust | Tantivy FTS | ~300 ms | Python, Tantivy | No | Powerful full-text search; heavier install |
| [cc-sessions-cli](https://github.com/AiRSpace/cc-sessions-cli) | Python | Simple grep | ~2 s | Python | No | Basic; no ranking or snippets |
| [claude-conversation-extractor](https://github.com/nicobailey/claude-conversation-extractor) | Python | Export only | N/A | Python | No | Export/convert tool, not a search tool |

### Why this one?

- **Zero config**: No indexing step, no database, no Python runtime. Build the binary, point the skill at it, done.
- **Instant default mode**: Index search completes in 18 ms — fast enough that Claude Code can call it mid-conversation without the user noticing.
- **Sub-second deep search**: Ripgrep does the heavy lifting with SIMD-accelerated matching across 1.6 GB of JSONL in 280 ms.
- **Native Claude Code skill**: Designed from the start as a `/search-sessions` slash command, not a standalone app retrofitted with a wrapper.
- **No external runtime**: Single static binary. No Python, no Node, no Docker.

## Python fallback

A standalone Python version is included as `search-sessions.py`. It has identical functionality and output format, requires only the Python standard library (plus `rg` for deep search).

```bash
python3 search-sessions.py "your query"
python3 search-sessions.py "your query" --deep
```

## License

MIT
