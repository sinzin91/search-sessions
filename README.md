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

Paste this into Claude Code:

```
Clone https://github.com/sinzin91/search-sessions, build it with cargo, 
and set it up as a /search-sessions slash command I can use.
```

That's it. Claude handles the rest.

## Manual Install

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

## Built for Claude, not around it

Other tools give you a separate TUI or CLI to learn. This one works *inside* Claude — just ask your agent:

> "search my sessions for that kubernetes RBAC discussion"

No commands to memorize. No context switching. Claude finds it, shows you the result, and can resume the session if you want.

## Comparison

| Tool | Speed | Setup | Native to Claude |
|------|-------|-------|------------------|
| **search-sessions** | 280ms | `cargo build` | ✅ Slash command |
| cc-conversation-search | ~500ms | Python + SQLite | ❌ Separate CLI |
| claude-history | ~400ms | Rust | ❌ TUI only |
| aichat claude-code-tools | ~300ms | Python + Tantivy | ❌ Separate CLI |
| cc-sessions-cli | ~2s | Python | ❌ Separate CLI |

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
