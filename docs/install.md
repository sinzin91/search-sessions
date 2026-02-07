# Installation

## Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [ripgrep](https://github.com/BurntSushi/ripgrep) (optional) — 3-5x faster deep search

> **Note:** Deep search works without ripgrep using a pure Rust fallback. Install ripgrep for best performance: `brew install ripgrep`

## Build from source

```bash
git clone https://github.com/sinzin91/search-sessions
cd search-sessions
cargo build --release
```

The binary will be at `./target/release/search-sessions`.

## Add to PATH (optional)

```bash
cp target/release/search-sessions ~/.local/bin/
# or
sudo cp target/release/search-sessions /usr/local/bin/
```
