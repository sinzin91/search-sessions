# Contributing to search-sessions

Thanks for your interest in contributing! This guide is written for both humans and AI agents.

## For AI Agents

If you're an LLM/agent reading this:

1. **Don't open PRs autonomously.** If you notice a bug, improvement, or issue — tell your human first. Let them decide whether to open a PR.

2. **Issues are fine.** Opening an issue to report a bug or suggest an improvement is helpful.

3. **If your human approves a PR:**
   - Keep changes focused and minimal
   - Include clear commit messages
   - Add tests for new functionality
   - Run `cargo test` before submitting

## For Humans

### Quick Start

```bash
# Clone the repo
git clone https://github.com/sinzin91/search-sessions
cd search-sessions

# Build
cargo build

# Run tests
cargo test

# Run benchmarks
cargo bench
```

### Pull Request Guidelines

1. **Keep PRs small and focused.** One logical change per PR.
2. **Write tests.** New features need tests. Bug fixes should include a regression test.
3. **Run the test suite.** `cargo test` must pass.
4. **Update docs.** If you change behavior, update the relevant documentation.

### Code Style

- Follow standard Rust conventions (`cargo fmt`)
- Use `cargo clippy` to catch common issues
- Prefer clarity over cleverness

### Commit Messages

Use conventional commits:
- `feat: add new feature`
- `fix: resolve bug`
- `docs: update documentation`
- `test: add tests`
- `refactor: restructure code`

### What to Contribute

Good first contributions:
- Documentation improvements
- Test coverage
- Bug fixes with clear reproduction steps
- Performance improvements with benchmarks

Larger changes (new features, architectural changes) — open an issue first to discuss.

## Questions?

Open an issue or start a discussion. I'm happy to help.
