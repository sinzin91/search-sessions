# Benchmarks

## Test dataset

- 514 session files
- 1.6 GB total JSONL data
- macOS

## Results

| Mode | Python version | Rust version | Speedup |
|------|---------------|-------------|---------|
| **Index search** | 100 ms | 18 ms | **5.5x** |
| **Deep search** | 370 ms | 280 ms | **1.3x** |

## Analysis

**Index search** is **5.5x faster** in Rust — effectively instant at 18ms. This is the default mode and what most queries use.

**Deep search** gains are more modest (1.3x) since both versions use ripgrep for the heavy lifting. The Rust advantage comes from eliminating Python's ~80ms startup overhead.

## OpenClaw performance

Tested on 14 sessions, 15 MB of JSONL:

| Query type | Time |
|------------|------|
| Specific queries | ~50 ms |
| Common words | ~130 ms |
