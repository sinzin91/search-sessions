# search-sessions

> Find anything from your Claude Code history. Instantly.

![search-sessions demo](assets/demo.gif)

## Why?

Claude Code forgets. After a few sessions, you've lost context — that clever regex, the architecture decision, the API you debugged at 2am.

Your session history is still there, buried in `~/.claude/projects/`. But good luck finding anything in 1.6GB of JSONL files.

**search-sessions** fixes that. One binary. No indexing step. Sub-second search across everything.

Each result includes the session UUID — so you can find *and resume* any past conversation:

```
❯ search-sessions "beads-tracker"

  [1] beads-tracker project exploration
      Project:  ~/Projects/noc0/beads-tracker
      Date:     2026-01-28 15:30
      Session:  7897c935-2069-4b75-bbad-a3fac62ea59c

❯ claude --resume 7897c935-2069-4b75-bbad-a3fac62ea59c
```

## Quick Start

```bash
# Install
git clone https://github.com/sinzin91/search-sessions
cd search-sessions
cargo build --release

# Search
./target/release/search-sessions "kubernetes RBAC"
./target/release/search-sessions "docker compose" --deep
```

## Speed

Tested on 514 sessions, 1.6 GB of JSONL:

| Mode | Time |
|------|------|
| Index search | **18 ms** |
| Deep search | **280 ms** |

Index search hits session metadata (summaries, first prompts). Deep search greps the actual message content via ripgrep.

## Comparison

| Tool | Speed | Setup | Notes |
|------|-------|-------|-------|
| **search-sessions** | 280ms | `cargo build` | Zero config, single binary |
| cc-conversation-search | ~500ms | Python + SQLite | Requires indexing step |
| claude-history | ~400ms | Rust | TUI only, no CLI mode |
| aichat claude-code-tools | ~300ms | Python + Tantivy | Heavier install |
| cc-sessions-cli | ~2s | Python | Basic, no ranking |

## Use as Claude Code Skill

See [docs/claude-code-skill.md](docs/claude-code-skill.md) for `/search-sessions` slash command setup.

## OpenClaw Support

Also searches OpenClaw agent sessions with `--openclaw`. See [docs/openclaw.md](docs/openclaw.md).

## Docs

- [Installation](docs/install.md)
- [Claude Code Skill Setup](docs/claude-code-skill.md)
- [OpenClaw Support](docs/openclaw.md)
- [Architecture](docs/architecture.md)
- [Benchmarks](docs/benchmarks.md)

## License

MIT
