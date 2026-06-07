# Integration Guide: fleet-warden

## What This Crate Provides

- **`ScanReport`** — Full disk scan report with per-category sizes and counts (target dirs, pip/npm cache, old toolchains, stale sessions, HuggingFace weights, large files)
- **`DiskBudget`** — Current disk usage with total/used/free, growth rate estimation, and total recovered
- **`Cleaner`** — Automated cleanup for: target/ dirs, pip/npm caches, stale sessions, old Rust toolchains, HuggingFace cache
- **`State`** — Persistent cleanup history with per-category recovery tracking
- **CLI commands**: `check` (scan), `clean` (execute), `watch` (daemon), `budget` (usage), `history` (log)

This crate is the automated disk cleanup daemon for WSL development environments. It scans, reports, and cleans developer artifacts that accumulate in Rust/Cargo, Python/pip, Node/npm, HuggingFace, and session directories.

## How to Add This Crate

```bash
cargo add fleet-warden
```

```rust
use fleet_warden::scanner::full_scan;

let report = full_scan()?;
println!("Total cleanable: {} bytes", report.total_cleanable());
println!("Target dirs: {} items, {} bytes", report.target_dirs_count, report.target_dirs_size);
```

## Integration Points

### conservation-law

- **Why**: conservation-law provides the mathematical framework for resource budgets; fleet-warden enforces those budgets in practice. Every cleanup is a conservation audit: resource deallocation must preserve the system's energy invariant.
- **How**: After cleanup, feed before/after metrics into conservation-law's `ConservationReport` to verify no agent was destabilized by resource reclamation.

```rust
use fleet_warden::scanner::{full_scan, target_dirs_size};
use fleet_warden::cleaner::clean_target_dirs;

// Scan before
let before = full_scan()?;
let target_before = before.target_dirs_size;

// Clean
clean_target_dirs()?;

// Scan after and verify
let after = full_scan()?;
let recovered = target_before.saturating_sub(after.target_dirs_size);
println!("Recovered {} bytes", recovered);
```

### t-minus

- **Why**: t-minus provides scheduling; fleet-warden runs periodic cleanup sweeps that should be scheduled precisely. The `watch` command can be replaced with t-minus cron for more control.
- **How**: Schedule fleet-warden scans via t-minus cron expressions, and use token bucket rate limiting to prevent cleanup storms.

```rust
use fleet_warden::scanner::full_scan;
use fleet_warden::state::State;

// Run on a t-minus schedule
let report = full_scan()?;
if report.total_cleanable() > 10 * 1024 * 1024 * 1024 { // > 10GB
    println!("Threshold exceeded — schedule cleanup via t-minus");
}
```

### entropy-conservation

- **Why**: entropy-conservation enforces the γ + H = C invariant; fleet-warden's cleanup operations produce entropy (resource deallocation is thermodynamically irreversible). The entropy budget gates how much cleanup can happen in a given time window.
- **How**: Before a cleanup sweep, check the entropy budget. After cleanup, report the entropy production.

```rust
use fleet_warden::budget::disk_budget;

let budget = disk_budget()?;
if budget.used_pct > 85.0 {
    println!("WARNING: Disk usage at {:.1}% — cleanup recommended", budget.used_pct);
}
if let Some(rate) = budget.growth_rate {
    let days_until_full = budget.free / rate;
    if days_until_full < 7 {
        println!("CRITICAL: Disk full in ~{} days", days_until_full);
    }
}
```

## For AI Agents

- **Context needed**: Home directory path, disk usage thresholds, cleanup categories to target
- **Key imports**: `fleet_warden::scanner::{full_scan, ScanReport}`, `fleet_warden::budget::{disk_budget, DiskBudget}`, `fleet_warden::state::State`
- **Integration pattern**: Scan → check thresholds → clean selectively → verify recovery → record in state
- **Error handling**: All operations return `anyhow::Result`. Scanner failures (missing directories) are non-fatal — the report shows 0 for unavailable categories.

## For Humans

- **Prerequisites**: Basic familiarity with WSL/Linux filesystem, Cargo/pip/npm caches
- **Learning path**: Run `fleet-warden check` first (dry run), then `fleet-warden clean --target-dirs` (single category), then `fleet-warden clean --all` (everything)
- **Common pitfalls**:
  - `clean --all` is aggressive — it removes ALL target/ directories, requiring full rebuilds
  - The `watch` command runs indefinitely; use Ctrl+C or run as a systemd service
  - Stale session cleanup uses mtime — ensure your system clock is correct
  - Growth rate estimation requires 2+ data points; first run shows "unknown"
