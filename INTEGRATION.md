# INTEGRATION.md — fleet-warden-rs × agent-homeostasis-rs × spectral-fleet-rs

**Fleet warden** monitors disk health and performs automated cleanup. It
connects to homeostatic regulation for treating disk usage as a regulated
parameter and to spectral methods for anomaly detection in usage patterns.

## Synergy Map

```
agent-homeostasis-rs           fleet-warden-rs               spectral-fleet-rs
┌──────────────────┐          ┌──────────────────────┐       ┌─────────────────┐
│ HomeostaticRegul  │◄────────►│ DiskBudget           │◄─────►│ l2_norm         │
│ PidController     │          │ ScanReport           │       │ normalize       │
│ SensorArray       │          │ full_scan            │       │ PowerIteration  │
│ Setpoint          │          │ disk_budget          │       │ SpectralCluster │
└──────────────────┘          │ clean_target_dirs    │       └─────────────────┘
                              │ BudgetSample         │
                              │ State                │
                              └──────────────────────┘
```

## Key Insight

Disk usage is a homeostatic parameter: it has a target (free space),
deviates due to build artifacts and caches, and requires corrective
action (cleanup). Agent-homeostasis provides the PID loop framework;
fleet-warden provides the scanners and cleaners; spectral-fleet detects
anomalous usage patterns that deviate from the fleet norm.

## Example 1: Homeostatic Disk Regulation

Use a PID controller to keep disk usage within a setpoint by triggering
warden cleanups.

```rust
use agent_homeostasis::{PidController, SensorReading, Setpoint};
use fleet_warden::scanner::{full_scan, ScanReport};
use fleet_warden::cleaner::clean_target_dirs;

fn regulate_disk_usage() {
    let mut pid = PidController::new("disk_used", 0.1, 0.01, 0.05, 50.0);
    pid.with_output_limit(10.0);

    let scan: ScanReport = full_scan().unwrap();
    let used_pct = scan.total_cleanable() as f64 / 1e9; // rough GB estimate

    let result = pid.update(used_pct);
    println!("Disk used: {:.1} GB, PID output: {:.2}", used_pct, result.output);

    if result.output > 5.0 {
        println!("Triggering cleanup...");
        clean_target_dirs().unwrap();
    }
}
```

## Example 2: Fleet-Wide Disk Health with Spectral Anomaly Detection

Build an affinity matrix from disk usage across hosts and detect outliers.

```rust
use spectral_fleet::{l2_norm, normalize, dot};
use fleet_warden::budget::disk_budget;

fn spectral_disk_health(hosts: &[&str]) -> Vec<(usize, f64)> {
    let mut usage_vec = vec![0.0; hosts.len()];
    for (i, _host) in hosts.iter().enumerate() {
        let budget = disk_budget().unwrap();
        usage_vec[i] = budget.used_pct;
    }

    // Build Gaussian affinity from usage similarity
    let n = hosts.len();
    let mut affinity = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in i..n {
            let d = usage_vec[i] - usage_vec[j];
            let a = (-d * d / 100.0).exp();
            affinity[i][j] = a;
            affinity[j][i] = a;
        }
    }

    // Power iteration for dominant eigenvector
    let mut vec = vec![1.0; n];
    normalize(&mut vec);
    for _ in 0..100 {
        let mut next = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                next[i] += affinity[i][j] * vec[j];
            }
        }
        normalize(&mut next);
        vec = next;
    }

    let mut ranked: Vec<(usize, f64)> = vec.iter()
        .enumerate()
        .map(|(i, &v)| (i, v.abs()))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    ranked
}
```

## Example 3: Budget History as a State Monad

Track cleanup history using the state monad from categorical-agents.

```rust
use fleet_warden::state::{State, BudgetSample};
use fleet_warden::history::load_entries;

fn audit_cleanup_history() {
    let mut state = State::load().unwrap();
    let entries = load_entries().unwrap();

    let total_recovered: u64 = entries.iter().map(|e| e.recovered).sum();
    println!("Total recovered: {} bytes", total_recovered);

    state.record_cleanup("target_dirs", 1024 * 1024 * 100);
    state.save().unwrap();
}
```

## Cargo.toml Wiring

```toml
[dependencies]
fleet-warden = { git = "https://github.com/SuperInstance/fleet-warden-rs" }
agent-homeostasis = { git = "https://github.com/SuperInstance/agent-homeostasis-rs" }
spectral-fleet = { git = "https://github.com/SuperInstance/spectral-fleet-rs" }
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
