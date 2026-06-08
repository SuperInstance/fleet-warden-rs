# fleet-warden-rs

Automated disk cleanup daemon for WSL development environments. Scans, reports, and cleans target directories, caches, stale sessions, old toolchains, and large files. Includes anomaly detection (Z-score, MAD, IQR), circuit breaker pattern, adaptive rate limiting, and disk budget tracking.

## What It Does

Fleet Warden keeps development environments from running out of disk space. It scans for cleanable artifacts, tracks disk usage growth rates, auto-cleans when usage exceeds thresholds, and provides anomaly detection on system metrics.

Binary: `fleet-warden`

Core modules:

- **`scanner`** — Full disk scan across 7 categories (target dirs, pip/npm caches, toolchains, sessions, HuggingFace, large files)
- **`cleaner`** — Parallel cleanup with `rayon`, before/after measurement
- **`watcher`** — Daemon mode with periodic checks and auto-clean at 80% threshold
- **`budget`** — Disk usage tracking with growth rate estimation
- **`anomaly`** — Z-score, MAD, and IQR anomaly detection with sliding windows
- **`circuit_breaker`** — Closed → Open → HalfOpen state machine for protecting cleanup operations
- **`throttle`** — Adaptive rate limiting with p50/p95/p99 latency tracking
- **`state`** — Persistent JSON state with cleanup history and budget samples
- **`history`** — JSONL-based cleanup history log

## Quick Start

```bash
cargo install --git https://github.com/SuperInstance/fleet-warden-rs

# Scan and report (dry run)
fleet-warden check

# Clean specific categories
fleet-warden clean --target-dirs --pip-cache --npm-cache

# Clean everything
fleet-warden clean --all

# Clean stale sessions older than 14 days
fleet-warden clean --stale-sessions 14

# Run as daemon (checks every hour)
fleet-warden watch --interval 3600

# Check disk budget and growth rate
fleet-warden budget

# View cleanup history
fleet-warden history --limit 50
```

## CLI Reference

```
fleet-warden check
    Scan and report what can be cleaned (dry run)

fleet-warden clean [OPTIONS]
    --target-dirs        Clean */target/ directories
    --pip-cache          Clean pip cache
    --npm-cache          Clean npm cache
    --stale-sessions N   Clean sessions older than N days
    --old-toolchains     Clean inactive Rust toolchains
    --huggingface        Clean HuggingFace model cache
    --all                Clean everything

fleet-warden watch --interval SECONDS
    Run as daemon with periodic checks

fleet-warden budget
    Show disk budget, usage, growth rate, days until full

fleet-warden history --limit N
    Show last N cleanup entries
```

## Scanner

The scanner traverses configured directories and reports size + file count for each category.

```rust
use fleet_warden::scanner::full_scan;

fn main() -> anyhow::Result<()> {
    let report = full_scan()?;

    println!("Target dirs: {} bytes ({} files)", report.target_dirs_size, report.target_dirs_count);
    println!("Pip cache:   {} bytes", report.pip_cache_size);
    println!("npm cache:   {} bytes", report.npm_cache_size);
    println!("HuggingFace: {} bytes", report.huggingface_size);
    println!("Total cleanable: {} bytes", report.total_cleanable());
    Ok(())
}
```

### ScanReport Fields

```rust
pub struct ScanReport {
    pub target_dirs_count: usize,
    pub target_dirs_size: u64,
    pub pip_cache_size: u64,
    pub pip_cache_count: usize,
    pub npm_cache_size: u64,
    pub npm_cache_count: usize,
    pub old_toolchains_count: usize,
    pub old_toolchains_size: u64,
    pub stale_sessions_count: usize,
    pub stale_sessions_size: u64,
    pub huggingface_count: usize,
    pub huggingface_size: u64,
    pub large_files_count: usize,
    pub large_files_size: u64,
}
```

### Directory Size

```rust
use fleet_warden::scanner::dir_size;
use std::path::Path;

let (size, count) = dir_size(Path::new("/home/user/repos/my-project/target"));
println!("{} bytes across {} files", size, count);
```

### Individual Scanners

```rust
use fleet_warden::scanner::*;

let (count, size) = scan_target_dirs()?;
let (count, size) = scan_pip_cache()?;
let (count, size) = scan_npm_cache()?;
let (count, size) = scan_old_toolchains()?;
let (count, size) = scan_stale_sessions(30)?;  // older than 30 days
let (count, size) = scan_huggingface()?;
let (count, size) = scan_large_files()?;  // >100MB files in ~/repos
```

## Cleaner

Each cleaner measures before/after to report recovered bytes. Cleanup runs in parallel with `rayon`.

```rust
use fleet_warden::cleaner::*;

// Clean all target/ directories under ~/repos (parallel)
clean_target_dirs()?;

// Clean pip cache (tries `pip cache purge`, falls back to rm -rf ~/.cache/pip)
clean_pip_cache()?;

// Clean npm cache (tries `npm cache clean --force`, falls back to rm -rf ~/.npm/_cacache)
clean_npm_cache()?;

// Clean stale OpenClaw sessions older than N days
clean_stale_sessions(30)?;

// Remove inactive Rust toolchains (keeps the active one)
clean_old_toolchains()?;

// Remove entire HuggingFace cache
clean_huggingface()?;
```

### Measuring Recovery

```rust
use fleet_warden::scanner::target_dirs_size;
use fleet_warden::cleaner::clean_target_dirs;

let before = target_dirs_size()?;
clean_target_dirs()?;
let after = target_dirs_size()?;
let recovered = before.saturating_sub(after);
println!("Recovered {} bytes from target dirs", recovered);
```

## Watcher Daemon

Runs periodic checks and auto-cleans when disk usage exceeds 80%.

```rust
use fleet_warden::watcher::run;

// Run daemon with 1-hour intervals
run(3600)?;
```

Daemon behavior:
1. Every interval, check disk usage via `budget::disk_budget()`
2. Log check event to `~/.fleet-warden/log.jsonl`
3. If usage > 80%: auto-clean target dirs
4. If still > 80%: also clean pip and npm caches
5. Save budget samples for growth rate calculation (keeps last 30)

## Disk Budget

Tracks current usage, growth rate, and days until full.

```rust
use fleet_warden::budget::disk_budget;

let budget = disk_budget()?;

println!("Mount: {}", budget.mount_point);
println!("Total: {} bytes", budget.total);
println!("Used:  {} bytes ({:.1}%)", budget.used, budget.used_pct);
println!("Free:  {} bytes", budget.free);

if let Some(rate) = budget.growth_rate {
    println!("Growth rate: {} bytes/day", rate);
    let days = if rate > 0 { budget.free / rate } else { u64::MAX };
    println!("Days until full: {}", days.min(365));
}

println!("Total recovered (all time): {} bytes", budget.total_recovered);
```

### DiskBudget Fields

```rust
pub struct DiskBudget {
    pub mount_point: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub used_pct: f64,
    pub growth_rate: Option<u64>,     // bytes/day
    pub total_recovered: u64,         // cumulative across all cleanups
}
```

## Anomaly Detection

Three methods for detecting anomalies in time-series metrics.

### Z-Score

```rust
use fleet_warden::anomaly::z_score_anomalies;

let data = vec![10.0, 11.0, 12.0, 13.0, 14.0, 100.0];
let outliers = z_score_anomalies(&data, 2.0);
// outliers = [5] — the spike at index 5
```

### MAD (Median Absolute Deviation)

More robust to outliers than Z-score.

```rust
use fleet_warden::anomaly::mad_anomalies;

let data = vec![10.0, 11.0, 12.0, 13.0, 14.0, 50.0];
let outliers = mad_anomalies(&data, 3.0);
// outliers = [5]
```

### IQR (Interquartile Range)

```rust
use fleet_warden::anomaly::{iqr_anomalies, quartiles, median};

let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 100.0];
let outliers = iqr_anomalies(&data, 1.5);

let (q1, q2, q3) = quartiles(&data);
println!("Median: {:.1}, Q1: {:.1}, Q3: {:.1}", q2, q1, q3);
```

### Sliding Window Anomaly Detector

Combines all three methods in a sliding window.

```rust
use fleet_warden::anomaly::AnomalyDetector;

let mut det = AnomalyDetector::new(50); // 50-sample window
det.z_threshold = 2.0;
det.mad_threshold = 3.0;
det.iqr_k = 1.5;

// Feed normal values
for v in &[10.0, 11.0, 12.0, 13.0, 14.0] {
    det.push(*v);
}
assert!(!det.is_anomaly());

// Feed an outlier
det.push(100.0);
assert!(det.is_anomaly());

// Get all anomaly indices in window
let all = det.all_anomalies();
println!("Anomaly indices: {:?}", all);
```

## Circuit Breaker

Protects cleanup operations from cascading failures.

```rust
use fleet_warden::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};

let config = CircuitBreakerConfig {
    failure_threshold: 5,          // trip after 5 failures
    recovery_timeout_secs: 30.0,   // wait 30s before half-open
    success_threshold: 3,          // 3 successes to close
};

let mut cb = CircuitBreaker::new(config);

// Normal operation
assert!(cb.allow_request());
assert_eq!(cb.state, CircuitState::Closed);

// Record failures
for _ in 0..5 {
    cb.record_failure();
}
assert_eq!(cb.state, CircuitState::Open);

// Requests rejected while open
assert!(!cb.allow_request());

// Force trip or reset
cb.trip();   // force open
cb.reset();  // force closed

// Stats
let (successes, failures, rejections) = cb.stats();
```

### Half-Open Recovery

```rust
use fleet_warden::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

let config = CircuitBreakerConfig {
    failure_threshold: 1,
    recovery_timeout_secs: 0.0,  // immediate recovery
    success_threshold: 2,
};

let mut cb = CircuitBreaker::new(config);
cb.record_failure();                    // trips to Open
assert!(cb.allow_request());            // transitions to HalfOpen (timeout=0)
assert_eq!(cb.state, fleet_warden::circuit_breaker::CircuitState::HalfOpen);

cb.record_success();
cb.record_success();                    // 2 successes → Closed
assert_eq!(cb.state, fleet_warden::circuit_breaker::CircuitState::Closed);
```

## Adaptive Throttle

Token-bucket rate limiter that adjusts based on p99 latency.

```rust
use fleet_warden::throttle::AdaptiveThrottle;

let mut throttle = AdaptiveThrottle::new(100.0, 50.0); // 100 req/s, target p99=50ms

// Acquire tokens
if throttle.try_acquire() {
    // do work
}

// Record latency and auto-adapt
throttle.record_and_adapt(120.0);  // p99 too high → rate decreases
throttle.record_and_adapt(5.0);    // p99 low → rate increases

// Manual rate control
throttle.set_rate(50.0);
```

### Latency Tracker

```rust
use fleet_warden::throttle::LatencyTracker;

let mut tracker = LatencyTracker::new(1000); // 1000-sample window
tracker.record(12.0);
tracker.record(15.0);
tracker.record(200.0);

println!("p50: {:.1}ms", tracker.p50());
println!("p95: {:.1}ms", tracker.p95());
println!("p99: {:.1}ms", tracker.p99());
println!("mean: {:.1}ms", tracker.mean());
```

## State Persistence

State stored at `~/.fleet-warden/state.json`:

```rust
use fleet_warden::state::State;

let mut state = State::load()?;
state.record_cleanup("target_dirs", 1_500_000_000); // 1.5GB recovered
state.save()?;
```

### State Fields

```rust
pub struct State {
    pub last_cleanups: HashMap<String, String>,  // category → last cleanup timestamp
    pub total_recovered: u64,                      // cumulative bytes recovered
    pub budget_samples: Vec<BudgetSample>,          // last 30 samples for growth rate
}
```

## Cleanup History

History logged to `~/.fleet-warden/log.jsonl`:

```rust
use fleet_warden::history::load_entries;

let entries = load_entries()?;
for entry in entries.iter().rev().take(10) {
    println!("{} | {} | {} bytes recovered", entry.date, entry.category, entry.recovered);
}
```

## Health Checking and Fleet Monitoring

Use the scanner + anomaly detector together for fleet health monitoring:

```rust
use fleet_warden::scanner::full_scan;
use fleet_warden::anomaly::AnomalyDetector;

fn fleet_health_check() -> String {
    let report = match full_scan() {
        Ok(r) => r,
        Err(e) => return format!("Scan failed: {}", e),
    };

    let total = report.total_cleanable();
    let budget = fleet_warden::budget::disk_budget().unwrap();

    let mut det = AnomalyDetector::new(20);
    // ... feed historical usage data ...

    if budget.used_pct > 90.0 {
        "CRITICAL: disk >90%".to_string()
    } else if budget.used_pct > 80.0 {
        "WARNING: disk >80%, auto-clean recommended".to_string()
    } else if total > 5_000_000_000 {
        format!("OK but {} cleanable — consider `fleet-warden clean --all`", total)
    } else {
        "HEALTHY".to_string()
    }
}
```

## Conservation Enforcement

Fleet Warden enforces conservation laws by tracking total recovered bytes and growth rates. The `DiskBudget` struct provides `total_recovered` and `growth_rate` — when growth rate exceeds recovery rate, the system is losing ground and auto-clean triggers.

```rust
use fleet_warden::budget::disk_budget;

let budget = disk_budget()?;
let net_growth = budget.growth_rate.unwrap_or(0) as i64
    - budget.total_recovered as i64 / 30;  // approximate daily recovery

if net_growth > 0 {
    println!("WARNING: disk growing faster than cleanup can recover");
}
```

## Alert System

Combine anomaly detection with the circuit breaker for an alert pipeline:

```rust
use fleet_warden::anomaly::AnomalyDetector;
use fleet_warden::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use fleet_warden::budget::disk_budget;

fn run_alerts() {
    let mut usage_tracker = AnomalyDetector::new(24); // 24-hour window
    let mut alert_cb = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 3,
        recovery_timeout_secs: 300.0,
        success_threshold: 2,
    });

    loop {
        if let Ok(budget) = disk_budget() {
            usage_tracker.push(budget.used_pct);

            if usage_tracker.is_anomaly() {
                if alert_cb.allow_request() {
                    println!("ALERT: disk usage anomaly detected ({:.1}%)", budget.used_pct);
                    alert_cb.record_success();
                } else {
                    println!("Alert circuit open — suppressing duplicate alerts");
                }
            }

            if budget.used_pct > 80.0 {
                alert_cb.record_failure();
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
```

## Supabase Integration

Fleet Warden can report cleanup metrics to Supabase for fleet-wide monitoring:

```sql
CREATE TABLE fleet_warden_events (
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    host TEXT NOT NULL,
    event TEXT NOT NULL,
    category TEXT,
    recovered_bytes BIGINT,
    disk_used_pct FLOAT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

```rust
// POST cleanup event to si-fleet-api
async fn report_cleanup(category: &str, recovered: u64) {
    let client = reqwest::Client::new();
    client.post("https://api.superinstance.ai/v1/fleet/warden/event")
        .json(&serde_json::json!({
            "host": hostname(),
            "event": "cleanup",
            "category": category,
            "recovered_bytes": recovered,
        }))
        .send()
        .await
        .ok();
}
```

## Architecture

```
src/
├── main.rs              — CLI (check, clean, watch, budget, history)
├── scanner.rs           — full_scan, scan_target_dirs, scan_*, dir_size
├── cleaner.rs           — clean_target_dirs, clean_pip_cache, clean_*, rayon parallel
├── watcher.rs           — daemon loop, auto-clean at 80%, budget sampling
├── budget.rs            — disk_budget, growth rate estimation
├── anomaly.rs           — z_score_anomalies, mad_anomalies, iqr_anomalies, AnomalyDetector
├── circuit_breaker.rs   — CircuitBreaker (Closed → Open → HalfOpen)
├── throttle.rs          — AdaptiveThrottle, LatencyTracker (p50/p95/p99)
├── state.rs             — State (load/save JSON), BudgetSample, CleanupEntry
└── history.rs           — load_entries from JSONL
```

## License

MIT
