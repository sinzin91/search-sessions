# search-sessions

<p align="center">
  <img src="assets/header.png" alt="search-sessions" width="600">
</p>

[![CI](https://github.com/sinzin91/search-sessions/actions/workflows/ci.yml/badge.svg)](https://github.com/sinzin91/search-sessions/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/search-sessions.svg)](https://crates.io/crates/search-sessions)
[![Release](https://img.shields.io/github/v/release/sinzin91/search-sessions)](https://github.com/sinzin91/search-sessions/releases)
[![skills.sh](https://img.shields.io/badge/skills.sh-available-blue)](https://skills.sh)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Search across all your Claude Code and OpenClaw session history. Fast.

## Why?

Claude Code forgets. After a few sessions, you've lost context — that clever regex, the architecture decision, the API you debugged at 2am.

Your session history is still there, buried in `~/.claude/projects/`. But good luck finding anything in 1.6GB of JSONL files.

**search-sessions** fixes that. One binary. No indexing step. No database. Sub-second search across everything.

Each result includes the session UUID — so you can find *and resume* any past conversation:

```
❯ search-sessions "auth refactor"

  [1] auth refactor discussion
      Project:  ~/Projects/myapp
      Date:     2026-01-28 15:30
      Session:  7897c935-2069-4b75-bbad-a3fac62ea59c
      Resume:   cd ~/Projects/myapp && claude -r 7897c935-2069-4b75-bbad-a3fac62ea59c
```

Just copy-paste the `Resume` command to pick up where you left off.

## Quick Start

**For Claude Code users** — paste this into any session:

```
Set up https://github.com/sinzin91/search-sessions as a /search-sessions skill.
```

Claude reads the docs and handles install + setup. Then just ask to search your sessions.

### Manual Install

```bash
# Homebrew (macOS/Linux)
brew install sinzin91/tap/search-sessions

# Cargo (Rust)
cargo install search-sessions
```

See [docs/install.md](docs/install.md) for detailed instructions.

**Optional:** Want your agent to search history when asked "do you remember..."? Paste this:

```
Update your relevant files to use search-sessions when asked to remember or recall information.
```

## Usage

This is a tool meant to be used by your agent.

```bash
# Index search (instant, searches metadata)
search-sessions "kubernetes RBAC"

# Deep search (searches full message content)
search-sessions "docker compose" --deep

# Machine-readable deep hits with exact transcript provenance
search-sessions "not what I asked" --deep --format jsonl

# Filter by project
search-sessions "auth" --project myapp

# Filter by date range
search-sessions "deploy" --since "3 days ago"
search-sessions "auth" --since 2026-02-01 --until 2026-02-15
search-sessions "bug" --date today
search-sessions "refactor" --since "last week"
```

**Multiple accounts.** If you keep accounts in separate config roots with `CLAUDE_CONFIG_DIR` (for example `~/.claude` and `~/.claude-work`), every `~/.claude*/projects` directory and `$CLAUDE_CONFIG_DIR/projects` is searched and the results are merged. For a session from a root that `claude` doesn't read by default, the `Resume` command sets `CLAUDE_CONFIG_DIR` for you.

**`--format jsonl`** prints one JSON object per result. `kind` names the shape: `index`, `deep`, `openclaw` or `obsidian`. Claude Code rows carry `config_root`, the config dir to resume the session in, and `transcript_path` (null if an index lists a transcript that is gone). Deep rows also carry `record_line` and `record_uuid` (null when the record has none). OpenClaw rows carry `sessions_root` instead of `config_root`. When the only matching copy of a session is in a backup root, its `transcript_path` points into the backup while `config_root` names the live one.

**`--project` with deep search** searches every project directory whose name matches (a repo and its worktrees). If nothing matches in a config root, that root is skipped; earlier versions fell back to searching the whole root.

## Speed

| Mode | Time |
|------|------|
| Index search | **18 ms** |
| Deep search (with ripgrep) | **280 ms** |
| Deep search (Rust fallback) | **~1 s** |

**No dependencies required.** Install [ripgrep](https://github.com/BurntSushi/ripgrep) for 3-5x faster deep search.

## Built for Claude, not around it

Other tools give you a separate TUI or CLI to learn. This one works *inside* Claude — just ask:

> "search my sessions for that kubernetes RBAC discussion"

No commands to memorize. No context switching.

## Comparison

| Tool | Speed | Dependencies | Native to Claude |
|------|-------|--------------|------------------|
| **search-sessions** | 280ms | **None** | ✅ Slash command |
| cc-conversation-search | ~500ms | Python + SQLite | ❌ |
| claude-history | ~400ms | Rust | ❌ TUI only |
| aichat claude-code-tools | ~300ms | Python + Tantivy | ❌ |

## OpenClaw Support

Also searches OpenClaw agent sessions with `--openclaw`. See [docs/openclaw.md](docs/openclaw.md).

## Docs

- [Installation](docs/install.md)
- [Claude Code Skill Setup](docs/claude-code-skill.md)
- [OpenClaw Support](docs/openclaw.md)
- [Architecture](docs/architecture.md)
- [Benchmarks](docs/benchmarks.md)
- [Changelog](CHANGELOG.md)

## License

MIT
