# fleet-warden

> Automated disk cleanup, budget enforcement, and health monitoring for distributed agent environments.

## What This Does

Fleet Warden is a daemon and CLI that prevents disk bloat in development and production fleets. It scans for recoverable waste — Rust `target/` directories, pip and npm caches, stale toolchains, old session files, and HuggingFace model weights — then cleans selectively or on schedule. It tracks cleanup history, monitors disk budgets with growth-rate estimation, and can run as a background watcher that enforces policies automatically. One real-world deployment recovered **54 GB** from a single workstation.

## Why It Matters

Agent fleets generate artifacts at machine speed: model checkpoints, build outputs, cached dependencies, log streams. Without automated hygiene, every node becomes a disk-full incident waiting to happen. In the AGI trajectory, self-managing infrastructure is not optional — it is the immune system of the fleet. Fleet Warden is that immune system: it patrols, diagnoses, and heals storage before humans notice the problem.

## Quick Start

```bash
# Install
git clone https://github.com/SuperInstance/fleet-warden-rs
cd fleet-warden-rs
cargo install --path .

# Dry run — see what can be recovered
fleet-warden check

# Clean everything
fleet-warden clean --all

# Watch mode — patrol every hour
fleet-warden watch --interval 3600

# Check disk budget
eet-warden budget
```

### Programmatic Usage

```rust
use fleet_warden::scanner::{full_scan, ScanReport};
use fleet_warden::cleaner;
use anyhow::Result;

fn main() -> Result<()> {
    let report: ScanReport = full_scan()?;
    println!("Total cleanable: {} bytes", report.total_cleanable());

    if report.target_dirs_size > 1_000_000_000 {
        cleaner::clean_target_dirs()?;
    }
    Ok(())
}
```

## Architecture

| Module | Purpose |
|--------|---------|
| `scanner` | Recursive filesystem scans for targets, caches, toolchains, sessions, and large files |
| `cleaner` | Safe removal with before/after size verification for each category |
| `watcher` | Daemon loop that periodically scans and cleans based on policy thresholds |
| `budget` | Disk usage tracking, growth-rate estimation, and days-until-full prediction |
| `state` | Persistent cleanup ledger with JSON-backed history |
| `history` | Load and query past cleanup events |

## API Tour

### `ScanReport`

The output of a full fleet scan, serializable to JSON.

```rust
#[derive(Debug, Serialize, Clone)]
pub struct ScanReport {
    pub target_dirs_count: usize,
    pub target_dirs_size: u64,
    pub pip_cache_size: u64,
    pub npm_cache_size: u64,
    pub old_toolchains_count: usize,
    pub stale_sessions_count: usize,
    pub huggingface_size: u64,
    pub large_files_count: usize,
}

impl ScanReport {
    pub fn total_cleanable(&self) -> u64;
}
```

### Scanner functions

Each returns a `(count, size)` tuple or populates a `ScanReport`.

```rust
pub fn full_scan() -> Result<ScanReport>
pub fn dir_size(path: &Path) -> (u64, usize)
pub fn target_dirs_size() -> Result<u64>
pub fn pip_cache_size() -> Result<u64>
pub fn stale_sessions_size(days: u64) -> Result<u64>
```

### Cleaner functions

Safe, idempotent cleanup with rayon-parallelized target removal.

```rust
pub fn clean_target_dirs() -> Result<()>
pub fn clean_pip_cache() -> Result<()>
pub fn clean_npm_cache() -> Result<()>
pub fn clean_stale_sessions(days: u64) -> Result<()>
pub fn clean_old_toolchains() -> Result<()>
pub fn clean_huggingface() -> Result<()>
```

### CLI Commands

| Command | Purpose |
|---------|---------|
| `check` | Dry-run scan with formatted table + JSON stderr |
| `clean` | Selective or `--all` cleanup with per-category recovery reports |
| `watch` | Background daemon with configurable interval |
| `budget` | Disk usage, growth rate, and days-until-full estimate |
| `history` | Recent cleanup events with date, category, and bytes recovered |

## Performance

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Full scan | O(filesystem nodes) | Parallelized with `rayon` for target directories |
| Target cleanup | O(target dirs) | Parallel `remove_dir_all` |
| Cache cleanup | O(1) | Shells out to `pip`, `npm`, or removes cache directories |
| Budget query | O(history entries) | Reads JSON ledger from disk |
| Watch loop | O(interval) | Sleeps between scans, configurable granularity |

The scanner uses `symlink_metadata` to avoid following symlinks into dangerous territory. All cleaners verify recovery with before/after size deltas.

## Ecosystem

- **[t-minus](https://github.com/SuperInstance/t-minus-rs)** — Schedule `fleet-warden check` on cron intervals
- **[conservation-law](https://github.com/SuperInstance/conservation-law-rs)** — Model disk growth as a dynamical system and predict inflection points
- **[spectral-fleet](https://github.com/SuperInstance/spectral-fleet-rs)** — Cluster fleet nodes by cleanup patterns to identify outlier resource hogs
- **[categorical-agents](https://github.com/SuperInstance/categorical-agents-rs)** — Compose cleanup policies as monadic state transformations

## Ideas for Improvement

1. **Policy engine** — YAML/JSON rule files defining per-category thresholds and auto-clean triggers.
2. **Fleet-wide aggregation** — Collect scan reports from all nodes and surface fleet-level dashboards.
3. **Dry-run diff mode** — Show exactly which files would be deleted before any removal.
4. **Cloud storage tiering** — Automatically move cold HuggingFace weights to object storage instead of deleting.
5. **Integration with container runtimes** — Scan and clean Docker volumes, BuildKit caches, and overlayfs leftovers.

## License

MIT OR Apache-2.0
