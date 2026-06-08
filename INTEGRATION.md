# Integration Guide: fleet-warden-rs

## What This Crate Provides

Automated disk cleanup daemon for WSL development environments. Scans, reports, and cleans developer artifacts that accumulate across Rust/Cargo, Python/pip, Node/npm, HuggingFace, and session directories.

- **`scanner::full_scan()`** — Full disk scan returning `ScanReport` with per-category sizes and counts (target dirs, pip/npm cache, old toolchains, stale sessions, HuggingFace weights, large files).
- **`scanner::ScanReport`** — Serializable report with `total_cleanable()` summing all recoverable space.
- **`scanner::target_dirs_size()`**, **`scanner::pip_cache_size()`**, **`scanner::npm_cache_size()`**, **`scanner::huggingface_size()`** — Individual category size queries.
- **`cleaner::clean_target_dirs()`** — Remove all `*/target/` directories under `~/repos` (parallel via rayon).
- **`cleaner::clean_pip_cache()`** — Purge pip cache via `pip cache purge` or direct `~/.cache/pip` removal.
- **`cleaner::clean_npm_cache()`** — Clean npm cache via `npm cache clean --force` or `~/.npm/_cacache` removal.
- **`cleaner::clean_stale_sessions(days)`** — Remove session files older than N days.
- **`cleaner::clean_old_toolchains()`** — Remove outdated Rust toolchain installations.
- **`cleaner::clean_huggingface()`** — Clean HuggingFace model cache.
- **`budget::disk_budget()`** — Current disk usage with `total`, `used`, `free`, `used_pct`, `growth_rate`, and `total_recovered`.
- **`budget::DiskBudget`** — Serializable struct with mount point and growth estimation.
- **`watcher::run(interval)`** — Daemon mode: periodic scan, auto-clean at >80% usage, budget sampling, JSONL logging.
- **`state::State`** — Persistent cleanup history with `budget_samples`, `total_recovered`, and `last_cleanups`.
- **`state::BudgetSample`** — Timestamped disk usage snapshot for growth-rate calculation.
- **`state::CleanupEntry`** — Individual cleanup log entry with date, category, and recovered bytes.
- **`history::load_entries()`** — Load cleanup history as `Vec<CleanupEntry>`.
- **`anomaly::detect()`** — Detect abnormal disk usage patterns.
- **`circuit_breaker::CircuitBreaker`** — Protect against cascading cleanup failures.
- **`throttle::ThrottleConfig`** — Rate-limit cleanup operations.
- **CLI commands**: `check` (dry-run scan), `clean` (execute with filters), `watch` (daemon), `budget` (usage report), `history` (log viewer).

## How to Add This Crate

```bash
cargo add fleet-warden
```

```rust
use fleet_warden::scanner::full_scan;
use fleet_warden::budget::disk_budget;
use fleet_warden::state::State;
```

## Cross-Repo Connections

### With `conservation-law-rs`: Disk Budget as Conserved Quantity

Treat disk free space as a conserved quantity. Cleanup operations restore the budget, and anomaly detection triggers conservation audits:

```rust
use fleet_warden::budget::disk_budget;
use fleet_warden::state::State;
use conservation_law::conserved::ConservationDetector;

fn disk_conservation_audit() -> bool {
    let budget = disk_budget().unwrap();
    let state = State::load().unwrap();
    
    // Free space should be conserved: total - used = free
    let conserved = budget.total == budget.used + budget.free;
    if !conserved {
        println!("ALERT: disk accounting violation detected");
        return false;
    }
    
    // Check if growth rate violates conservation bounds
    if let Some(rate) = budget.growth_rate {
        let days_until_full = budget.free / rate;
        println!("Conservation horizon: {} days until budget exhausted", days_until_full);
    }
    
    true
}
```

### With `si-cli`: CLI Integration for Fleet Health

The si-cli gateway discovers `fleet-warden` commands via the `FleetBridgeServer` and routes them:

```rust
use fleet_warden::scanner::full_scan;
use fleet_warden::budget::disk_budget;

fn cli_disk_check() {
    let report = full_scan().unwrap();
    println!("Total cleanable: {} bytes", report.total_cleanable());
    
    let budget = disk_budget().unwrap();
    println!("Used: {:.1}%", budget.used_pct);
    
    // JSON output for si-cli scripting
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
```

### With `si-fleet-api`: REST Disk Health Endpoints

Expose disk health via the fleet REST API:

```rust
use fleet_warden::scanner::full_scan;
use fleet_warden::budget::disk_budget;
use fleet_warden::state::State;
use si_fleet_api::{HttpResponse, HttpRequest};

fn get_disk_health(_req: HttpRequest) -> HttpResponse {
    let report = full_scan().unwrap();
    let budget = disk_budget().unwrap();
    let state = State::load().unwrap_or_default();
    
    HttpResponse::json(json!({
        "used_pct": budget.used_pct,
        "free_bytes": budget.free,
        "total_cleanable": report.total_cleanable(),
        "total_recovered": budget.total_recovered,
        "growth_rate_per_day": budget.growth_rate,
        "categories": {
            "target_dirs": report.target_dirs_size,
            "pip_cache": report.pip_cache_size,
            "npm_cache": report.npm_cache_size,
            "huggingface": report.huggingface_size,
        }
    }))
}

fn post_trigger_cleanup(req: HttpRequest) -> HttpResponse {
    let targets: CleanTargets = req.json().unwrap();
    let mut state = State::load().unwrap();
    let mut total_recovered: u64 = 0;
    
    if targets.target_dirs {
        let before = fleet_warden::scanner::target_dirs_size().unwrap();
        fleet_warden::cleaner::clean_target_dirs().unwrap();
        let after = fleet_warden::scanner::target_dirs_size().unwrap();
        total_recovered += before.saturating_sub(after);
    }
    
    state.record_cleanup("api_triggered", total_recovered);
    state.save().unwrap();
    
    HttpResponse::json(json!({ "recovered": total_recovered }))
}
```

### With Supabase: Persistent Disk Monitoring

Store scan reports and cleanup history in Supabase for fleet-wide disk analytics:

```rust
use fleet_warden::scanner::ScanReport;
use fleet_warden::budget::DiskBudget;
use supabase_rs::SupabaseClient;

async fn persist_scan_report(
    client: &SupabaseClient,
    node_id: &str,
    report: &ScanReport,
    budget: &DiskBudget,
) {
    client.from("disk_scans")
        .insert(json!({
            "node_id": node_id,
            "used_pct": budget.used_pct,
            "free_bytes": budget.free,
            "target_dirs_size": report.target_dirs_size,
            "pip_cache_size": report.pip_cache_size,
            "npm_cache_size": report.npm_cache_size,
            "huggingface_size": report.huggingface_size,
            "total_cleanable": report.total_cleanable(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
        .execute()
        .await
        .unwrap();
}

async fn get_fleet_disk_summary(client: &SupabaseClient) -> Vec<DiskBudget> {
    let rows = client.from("disk_scans")
        .select("*")
        .order("timestamp.desc")
        .limit(100)
        .execute()
        .await
        .unwrap();
    
    rows.into_iter()
        .map(|r| DiskBudget {
            mount_point: r.get("node_id").unwrap().to_string(),
            total: 0,
            used: 0,
            free: r.get("free_bytes").unwrap().parse().unwrap(),
            used_pct: r.get("used_pct").unwrap().parse().unwrap(),
            growth_rate: None,
            total_recovered: 0,
        })
        .collect()
}
```

## Design Patterns

### Pattern: Predictive Cleanup Scheduling

Use budget growth rate to predict when cleanup will be needed:

```rust
use fleet_warden::budget::disk_budget;
use fleet_warden::state::State;

fn predict_cleanup_needed() -> Option<u64> {
    let budget = disk_budget().unwrap();
    let state = State::load().unwrap();
    
    if let Some(&last) = state.budget_samples.last() {
        let growth = budget.used as i64 - last.used_bytes as i64;
        if growth > 0 {
            let remaining = (budget.total - budget.used) as i64;
            let hours = remaining / growth;
            println!("Disk full in ~{} hours", hours);
            return Some(hours.max(0) as u64);
        }
    }
    None
}
```

### Pattern: Tiered Auto-Clean Cascade

Progressively escalate cleanup as disk pressure increases:

```rust
use fleet_warden::budget::disk_budget;
use fleet_warden::cleaner;

fn tiered_cleanup() {
    let budget = disk_budget().unwrap();
    
    if budget.used_pct > 80.0 {
        cleaner::clean_target_dirs().unwrap();
    }
    
    let budget = disk_budget().unwrap();
    if budget.used_pct > 85.0 {
        cleaner::clean_pip_cache().unwrap();
        cleaner::clean_npm_cache().unwrap();
    }
    
    let budget = disk_budget().unwrap();
    if budget.used_pct > 90.0 {
        cleaner::clean_stale_sessions(7).unwrap();
        cleaner::clean_huggingface().unwrap();
    }
}
```

### Pattern: Circuit-Breaker Protected Cleanup

Wrap cleanup in a circuit breaker to prevent cascading failures:

```rust
use fleet_warden::circuit_breaker::CircuitBreaker;
use fleet_warden::cleaner;

fn safe_cleanup() {
    let mut cb = CircuitBreaker::new(3, std::time::Duration::from_secs(60));
    
    if cb.allow() {
        match cleaner::clean_target_dirs() {
            Ok(()) => cb.record_success(),
            Err(e) => {
                cb.record_failure();
                println!("Cleanup failed, circuit breaker tripped: {}", e);
            }
        }
    } else {
        println!("Cleanup blocked by circuit breaker");
    }
}
```
