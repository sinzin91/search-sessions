# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-02-11

### Added

- **Copy-pasteable resume command**: Results now show `Resume: cd <dir> && claude -r <session-id>` for easy session resumption
- **Claude Code plugin**: Native marketplace support with `claude plugin marketplace add sinzin91/search-sessions`
- **SECURITY.md**: Vulnerability reporting via GitHub Security Advisories
- **CONTRIBUTING.md**: Contribution guidelines (agent-friendly!)

### Changed

- Improved README with clearer installation instructions
- Updated example output to show new Resume line

## [0.1.0] - 2026-02-08

### Added

- **Index search**: Fast metadata search across all Claude Code sessions (18ms on 514 sessions)
- **Deep search**: Full-text search of message content with ripgrep (~280ms on 1.6GB)
- **Pure Rust fallback**: Deep search works without ripgrep (~1s, no dependencies required)
- **OpenClaw support**: Search OpenClaw agent sessions with `--openclaw` flag
- **Session resume**: Results include session UUID for `claude --resume`
- **Claude Code skill**: Native `/search-sessions` slash command integration
- **Project filtering**: `--project` flag to scope searches
- **Cross-platform binaries**: Linux (x86_64, aarch64) and macOS (x86_64, aarch64)

### Distribution

- Published to [crates.io](https://crates.io/crates/search-sessions)
- Available via Homebrew: `brew install sinzin91/tap/search-sessions`
- GitHub releases with pre-built binaries

[0.1.1]: https://github.com/sinzin91/search-sessions/releases/tag/v0.1.1
[0.1.0]: https://github.com/sinzin91/search-sessions/releases/tag/v0.1.0
