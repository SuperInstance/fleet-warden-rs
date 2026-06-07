# fleet-warden-rs

Automated disk cleanup daemon for WSL development environments. Scans build artifacts, caches, stale sessions, and old toolchains. Cleans on schedule. Tracks budget trajectories.

## The Problem

WSL dev environments accumulate garbage: `target/` directories from 30 Rust projects (each 2–5 GB), pip/npm caches, stale OpenClaw session files, old Rust toolchains, HuggingFace model weights. Disk fills up silently. You find out when `cargo build` fails.

Fleet Warden watches, reports, and cleans — automatically or on demand.

## Commands

```
fleet-warden check              # Scan and report (dry run)
fleet-warden clean --all        # Clean everything
fleet-warden clean --target-dirs --pip-cache
fleet-warden watch --interval 3600  # Daemon mode
fleet-warden budget             # Disk budget and growth rate
fleet-warden history --limit 20     # Cleanup history
```

## Architecture

```
fleet-warden-rs
├── main.rs            — CLI entry point (clap)
├── scanner.rs         — Parallel disk scanning (rayon)
├── cleaner.rs         — Parallel cleanup operations
├── watcher.rs         — Daemon loop with auto-clean thresholds
├── budget.rs          — Disk budget tracking and growth rate
├── state.rs           — Persistent state (JSON)
├── history.rs         — Cleanup history log
├── anomaly.rs         — Z-score, MAD, IQR anomaly detection
├── circuit_breaker.rs — Closed → Open → HalfOpen state machine
└── throttle.rs        — Adaptive rate limiter with p50/p95/p99 tracking
```

## Scan: What's Eating Your Disk

The `check` command runs a full parallel scan of seven categories:

```rust
use fleet_warden::scanner::full_scan;

fn main() -> anyhow::Result<()> {
    let report = full_scan()?;

    println!("Target directories: {} items, {} bytes", 
        report.target_dirs_count, report.target_dirs_size);
    println!("Pip cache:          {} items, {} bytes",
        report.pip_cache_count, report.pip_cache_size);
    println!("npm cache:          {} items, {} bytes",
        report.npm_cache_count, report.npm_cache_size);
    println!("Old toolchains:     {} items, {} bytes",
        report.old_toolchains_count, report.old_toolchains_size);
    println!("Stale sessions:     {} items, {} bytes",
        report.stale_sessions_count, report.stale_sessions_size);
    println!("HuggingFace:        {} items, {} bytes",
        report.huggingface_count, report.huggingface_size);
    println!("Large files (>100M):{} items, {} bytes",
        report.large_files_count, report.large_files_size);
    println!("TOTAL CLEANABLE:    {} bytes", report.total_cleanable());

    Ok(())
}
```

### Individual Scan Functions

Each category has its own scan function if you need granularity:

```rust
use fleet_warden::scanner::{
    scan_target_dirs, scan_pip_cache, scan_npm_cache,
    scan_old_toolchains, scan_stale_sessions,
    scan_huggingface, scan_large_files,
};

fn check_caches() -> anyhow::Result<()> {
    let (count, size) = scan_pip_cache()?;
    println!("Pip cache: {} files, {} bytes", count, size);

    let (count, size) = scan_npm_cache()?;
    println!("npm cache: {} files, {} bytes", count, size);

    // Sessions older than 30 days
    let (count, size) = scan_stale_sessions(30)?;
    println!("Stale sessions: {} files, {} bytes", count, size);

    Ok(())
}
```

### Custom Directory Scanning

```rust
use fleet_warden::scanner::dir_size;
use std::path::Path;

fn custom_scan() {
    let path = Path::new("/home/user/repos/my-project/target");
    let (size, count) = dir_size(path);
    println!("{}: {} bytes in {} files", path.display(), size, count);
}
```

## Clean: Reclaim Space

Cleanup operations measure before and after to report exact recovery:

```rust
use fleet_warden::cleaner;
use fleet_warden::scanner;

fn clean_all() -> anyhow::Result<()> {
    // Target directories (~/repos/**/target/)
    let before = scanner::target_dirs_size()?;
    cleaner::clean_target_dirs()?;
    let after = scanner::target_dirs_size()?;
    println!("Target dirs: recovered {} bytes", before.saturating_sub(after));

    // Pip cache
    cleaner::clean_pip_cache()?;

    // npm cache
    cleaner::clean_npm_cache()?;

    // Stale sessions older than 14 days
    cleaner::clean_stale_sessions(14)?;

    // Old Rust toolchains (keeps active)
    cleaner::clean_old_toolchains()?;

    // HuggingFace model cache
    cleaner::clean_huggingface()?;

    Ok(())
}
```

### Selective Cleanup

```bash
# Only clean Rust build artifacts
fleet-warden clean --target-dirs

# Clean caches but keep build artifacts
fleet-warden clean --pip-cache --npm-cache

# Nuke old sessions (90 days)
fleet-warden clean --stale-sessions 90

# Nuclear option
fleet-warden clean --all
```

## Watch: The Daemon Cycle

The watcher runs a continuous loop. Every N seconds it:

1. Reads the disk budget
2. Logs a timestamped check to `~/.fleet-warden/log.jsonl`
3. If disk usage > 80%: auto-cleans target dirs
4. If still > 80%: also cleans pip and npm caches
5. Saves a budget sample for growth rate calculation

```rust
use fleet_warden::watcher;

fn main() -> anyhow::Result<()> {
    // Run the watcher with 1-hour intervals
    watcher::run(3600)?;
    Ok(())
}
```

### Auto-Clean Thresholds

The daemon uses a two-stage escalation:

```
Disk at 75% → log only
Disk at 80% → clean target/ directories
Disk at 80%+ after clean → also clean caches
Disk at 90%+ → (future: alert via webhook)
```

### Watch Log Format

Each cycle writes JSON entries to `~/.fleet-warden/log.jsonl`:

```json
{"timestamp":"2025-06-07T15:30:00Z","event":"watch_check","disk_used_pct":72.3}
{"timestamp":"2025-06-07T16:30:00Z","event":"auto_clean","category":"target_dirs","recovered":4294967296,"disk_used_pct_before":82.1}
```

## Budget: Disk Trajectory

The `budget` command shows current usage, growth rate, and days until full:

```rust
use fleet_warden::budget::disk_budget;

fn check_budget() -> anyhow::Result<()> {
    let budget = disk_budget()?;

    println!("Mount:         {}", budget.mount_point);
    println!("Total:         {} bytes", budget.total);
    println!("Used:          {} bytes ({:.1}%)", budget.used, budget.used_pct);
    println!("Free:          {} bytes", budget.free);

    if let Some(rate) = budget.growth_rate {
        println!("Growth rate:   {} bytes/day", rate);
        let days = if rate > 0 { budget.free / rate } else { u64::MAX };
        println!("Days to full:  {}", days.min(365));
    }

    println!("Total recovered (all time): {} bytes", budget.total_recovered);

    Ok(())
}
```

### Growth Rate Calculation

The growth rate is computed from the last 30 budget samples stored in state:

```
growth_rate = (used_last - used_first) / (days between samples)
```

Requires at least 2 data points (2 watch cycles). The more data points, the more accurate the trajectory.

## Budget Trajectory Over Time

After running the watcher for a few days, the budget samples tell a story:

```
Day 0:  60% used, 40% free, rate unknown
Day 1:  63% used, 37% free, rate = +3%/day
Day 2:  65% used, 35% free, rate = +2.5%/day
Day 3:  82% used → AUTO-CLEAN target dirs → 68% used
Day 3:  68% used, 32% free, rate recalculated
Day 4:  70% used, 30% free, rate = +2%/day
Day 7:  76% used, 24% free, rate = +1.3%/day
```

The growth rate drops after cleanup because build artifacts grow faster than caches.

## State Management

Fleet Warden persists state to `~/.fleet-warden/state.json`:

```rust
use fleet_warden::state::State;

fn state_example() -> anyhow::Result<()> {
    let mut state = State::load()?;

    // Record a cleanup
    state.record_cleanup("target_dirs", 4_294_967_296);
    state.record_cleanup("pip_cache", 500_000_000);

    println!("Total recovered: {} bytes", state.total_recovered);
    println!("Last cleanups: {:?}", state.last_cleanups);

    state.save()?;
    Ok(())
}
```

### State Structure

```json
{
  "last_cleanups": {
    "target_dirs": "2025-06-07T15:30:00Z",
    "pip_cache": "2025-06-07T15:30:01Z"
  },
  "total_recovered": 4794967296,
  "budget_samples": [
    {"timestamp": "2025-06-07T14:00:00Z", "used_bytes": 50000000000, "total_bytes": 100000000000},
    {"timestamp": "2025-06-07T15:00:00Z", "used_bytes": 45705032704, "total_bytes": 100000000000}
  ]
}
```

## Anomaly Detection

The `anomaly` module provides three methods for detecting anomalous disk usage patterns:

```rust
use fleet_warden::anomaly::{AnomalyDetector, z_score_anomalies, mad_anomalies, iqr_anomalies};

fn detect_anomalies() {
    // Historical disk usage percentages
    let usage = vec![60.0, 62.0, 61.0, 63.0, 59.0, 64.0, 95.0, 63.0, 62.0];

    // Z-score method
    let z_outliers = z_score_anomalies(&usage, 2.0);
    println!("Z-score anomalies at indices: {:?}", z_outliers); // [6]

    // MAD method (robust to outliers)
    let mad_outliers = mad_anomalies(&usage, 3.0);
    println!("MAD anomalies at indices: {:?}", mad_outliers); // [6]

    // IQR method
    let iqr_outliers = iqr_anomalies(&usage, 1.5);
    println!("IQR anomalies at indices: {:?}", iqr_outliers); // [6]

    // Sliding-window detector (combines all methods)
    let mut det = AnomalyDetector::new(20);
    for &v in &[60.0, 62.0, 61.0, 63.0, 59.0, 64.0] {
        det.push(v);
        println!("Value {} → anomaly: {}", v, det.is_anomaly());
    }
    det.push(95.0);
    println!("Value 95 → anomaly: {}", det.is_anomaly()); // true
}
```

## Circuit Breaker

The circuit breaker protects cleanup operations from running on a degraded system:

```rust
use fleet_warden::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};

fn circuit_breaker() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,      // trip after 3 failures
        recovery_timeout_secs: 30.0, // wait 30s before retry
        success_threshold: 2,      // 2 successes to close
    };
    let mut cb = CircuitBreaker::new(config);

    // Normal operation
    assert!(cb.allow_request());
    cb.record_success();
    cb.record_success();
    assert_eq!(cb.state, CircuitState::Closed);

    // Failures trip the breaker
    cb.record_failure();
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state, CircuitState::Open);
    assert!(!cb.allow_request()); // rejected

    // Force trip or reset
    cb.reset();
    assert_eq!(cb.state, CircuitState::Closed);

    // Stats
    let (successes, failures, rejections) = cb.stats();
    println!("S/F/R: {}/{}/{}", successes, failures, rejections);
}
```

## Adaptive Throttle

The throttle adjusts cleanup rate based on observed latency:

```rust
use fleet_warden::throttle::{AdaptiveThrottle, LatencyTracker};

fn throttle() {
    let mut throttle = AdaptiveThrottle::new(10.0, 100.0); // 10 ops/sec, target p99=100ms

    // Acquire tokens
    assert!(throttle.try_acquire());

    // Record latency — throttle adapts
    throttle.record_and_adapt(50.0);  // fast → rate increases
    throttle.record_and_adapt(50.0);
    println!("Rate after fast ops: {:.1}/s", throttle.rate_limit); // >10

    // Slow operations cause rate decrease
    for _ in 0..20 {
        throttle.record_and_adapt(500.0); // slow
    }
    println!("Rate after slow ops: {:.1}/s", throttle.rate_limit); // <10

    // Latency tracking
    let mut tracker = LatencyTracker::new(100);
    for i in 1..=50 {
        tracker.record(i as f64);
    }
    println!("p50={:.1}ms p95={:.1}ms p99={:.1}ms",
        tracker.p50(), tracker.p95(), tracker.p99());
}
```

## Cleanup History

```rust
use fleet_warden::history::load_entries;

fn show_history() -> anyhow::Result<()> {
    let entries = load_entries()?;
    for entry in entries.iter().rev().take(10) {
        println!("{}: {} → {} bytes recovered",
            entry.date, entry.category, entry.recovered);
    }
    Ok(())
}
```

## Typical Session

```
$ fleet-warden check
🔍 Fleet Warden — Disk Scan Report

────────────────────────────────────────────────────────────────
  Category                             Size       Items
────────────────────────────────────────────────────────────────
  Target directories (*/target/)      12.4 GB       34521
  Pip cache                            1.8 GB        1203
  npm cache                            0.9 GB         842
  Old Rust toolchains                  3.2 GB        8910
  Stale sessions (>30 days)            0.2 GB          45
  HuggingFace weights                 28.1 GB         120
  Large files (>100MB)                 4.7 GB          12
────────────────────────────────────────────────────────────────
  TOTAL CLEANABLE                     51.3 GB

$ fleet-warden clean --target-dirs --pip-cache --npm-cache
🧹 Fleet Warden — Cleaning Up

  ✓ Target dirs: recovered 12.4 GB
  ✓ Pip cache: recovered 1.8 GB
  ✓ npm cache: recovered 0.9 GB

✨ Total recovered: 15.1 GB

$ fleet-warden budget
📊 Fleet Warden — Disk Budget

  Mount:      /dev/sdb
  Total:      250.0 GB
  Used:       184.2 GB (73.7%)
  Free:       65.8 GB

  Growth rate: 1.2 GB/day (estimated)
  Days until full: 54

  Total recovered (all time): 127.3 GB
```

## Building and Running

```bash
cargo build --release
./target/release/fleet-warden check
./target/release/fleet-warden clean --all
./target/release/fleet-warden watch --interval 1800
```

## License

MIT
