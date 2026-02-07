# Benchmarks

## Test dataset

- 514 session files
- 1.6 GB total JSONL data
- macOS

## Results

| Mode | Time | Notes |
|------|------|-------|
| **Index search** | 18 ms | Pure Rust |
| **Deep search (with ripgrep)** | 280 ms | Rust + rg |
| **Deep search (Rust fallback)** | ~1 s | No dependencies |

## Analysis

**Index search** is effectively instant at 18ms. This is the default mode and what most queries use.

**Deep search with ripgrep** is sub-second at 280ms. Ripgrep provides SIMD-accelerated string matching.

**Deep search without ripgrep** falls back to pure Rust file scanning (~1s). Slower, but works everywhere with zero dependencies.

## OpenClaw performance

Tested on 14 sessions, 15 MB of JSONL:

| Query type | Time |
|------------|------|
| Specific queries | ~50 ms |
| Common words | ~130 ms |
