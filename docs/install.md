# Installation

## Homebrew (macOS/Linux)

```bash
brew install sinzin91/tap/search-sessions
```

## Cargo (Rust)

```bash
cargo install search-sessions
```

## From Source

### Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [ripgrep](https://github.com/BurntSushi/ripgrep) (optional) — 3-5x faster deep search

### Build

```bash
git clone https://github.com/sinzin91/search-sessions
cd search-sessions
cargo build --release
```

The binary will be at `./target/release/search-sessions`.

### Add to PATH

```bash
cp target/release/search-sessions ~/.local/bin/
# or
sudo cp target/release/search-sessions /usr/local/bin/
```

## Optional: Install ripgrep

Deep search works without ripgrep using a pure Rust fallback. For best performance (~3-5x faster), install ripgrep:

```bash
# macOS
brew install ripgrep

# Ubuntu/Debian
sudo apt install ripgrep

# Arch
sudo pacman -S ripgrep
```
