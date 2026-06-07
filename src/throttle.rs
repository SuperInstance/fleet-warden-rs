//! Adaptive rate limiting with p50/p95/p99 latency tracking.

/// A latency tracker that computes percentiles.
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    samples: Vec<f64>,
    max_samples: usize,
}

impl LatencyTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn record(&mut self, latency_ms: f64) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }
        self.samples.push(latency_ms);
    }

    pub fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = (p / 100.0 * (sorted.len() - 1) as f64).min((sorted.len() - 1) as f64);
        let lower = idx.floor() as usize;
        let upper = idx.ceil() as usize;
        if lower == upper {
            sorted[lower]
        } else {
            let frac = idx - lower as f64;
            sorted[lower] * (1.0 - frac) + sorted[upper] * frac
        }
    }

    pub fn p50(&self) -> f64 { self.percentile(50.0) }
    pub fn p95(&self) -> f64 { self.percentile(95.0) }
    pub fn p99(&self) -> f64 { self.percentile(99.0) }

    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() { return 0.0; }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    pub fn len(&self) -> usize { self.samples.len() }
    pub fn is_empty(&self) -> bool { self.samples.is_empty() }
    pub fn clear(&mut self) { self.samples.clear(); }
}

/// Adaptive throttle that adjusts rate limits based on latency.
#[derive(Debug, Clone)]
pub struct AdaptiveThrottle {
    pub rate_limit: f64,
    pub min_rate: f64,
    pub max_rate: f64,
    pub target_p99_ms: f64,
    pub tracker: LatencyTracker,
    tokens: f64,
    max_tokens: f64,
    last_tick: u64,
}

impl AdaptiveThrottle {
    pub fn new(rate_limit: f64, target_p99_ms: f64) -> Self {
        let max_tokens = rate_limit * 2.0;
        Self {
            rate_limit,
            min_rate: 1.0,
            max_rate: 10000.0,
            target_p99_ms,
            tracker: LatencyTracker::new(1000),
            tokens: max_tokens,
            max_tokens,
            last_tick: 0,
        }
    }

    pub fn try_acquire(&mut self) -> bool {
        self.refill(1);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self, current_tick: u64) {
        if current_tick > self.last_tick {
            let elapsed = current_tick - self.last_tick;
            let new_tokens = self.rate_limit * elapsed as f64;
            self.tokens = (self.tokens + new_tokens).min(self.max_tokens);
            self.last_tick = current_tick;
        }
    }

    pub fn record_and_adapt(&mut self, latency_ms: f64) {
        self.tracker.record(latency_ms);
        if self.tracker.len() < 10 {
            return;
        }
        let p99 = self.tracker.p99();
        if p99 > self.target_p99_ms * 1.5 {
            self.rate_limit = (self.rate_limit * 0.8).max(self.min_rate);
        } else if p99 < self.target_p99_ms * 0.8 {
            self.rate_limit = (self.rate_limit * 1.1).min(self.max_rate);
        }
        self.max_tokens = self.rate_limit * 2.0;
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.rate_limit = rate.clamp(self.min_rate, self.max_rate);
        self.max_tokens = self.rate_limit * 2.0;
    }

    pub fn reset(&mut self) {
        self.tracker.clear();
        self.tokens = self.max_tokens;
        self.last_tick = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_record_and_p50() {
        let mut t = LatencyTracker::new(100);
        for v in &[10.0, 20.0, 30.0, 40.0, 50.0] { t.record(*v); }
        assert!((t.p50() - 30.0).abs() < 1e-10);
    }

    #[test]
    fn test_tracker_p95() {
        let mut t = LatencyTracker::new(100);
        for i in 1..=100 { t.record(i as f64); }
        assert!(t.p95() > 90.0);
    }

    #[test]
    fn test_tracker_p99() {
        let mut t = LatencyTracker::new(100);
        for i in 1..=100 { t.record(i as f64); }
        assert!(t.p99() > 95.0);
    }

    #[test]
    fn test_tracker_empty() {
        let t = LatencyTracker::new(100);
        assert!(t.is_empty());
        assert!((t.p50() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_tracker_mean() {
        let mut t = LatencyTracker::new(100);
        t.record(10.0); t.record(20.0); t.record(30.0);
        assert!((t.mean() - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_tracker_window_eviction() {
        let mut t = LatencyTracker::new(3);
        t.record(1.0); t.record(2.0); t.record(3.0); t.record(4.0);
        assert_eq!(t.len(), 3);
        assert!((t.mean() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_tracker_clear() {
        let mut t = LatencyTracker::new(10);
        t.record(1.0);
        t.clear();
        assert!(t.is_empty());
    }

    #[test]
    fn test_throttle_basic_acquire() {
        let mut th = AdaptiveThrottle::new(10.0, 100.0);
        assert!(th.try_acquire());
    }

    #[test]
    fn test_throttle_refill() {
        let mut th = AdaptiveThrottle::new(10.0, 100.0);
        for _ in 0..100 { th.try_acquire(); }
        assert!(!th.try_acquire());
        th.refill(2);
        assert!(th.try_acquire());
    }

    #[test]
    fn test_throttle_adapt_decrease() {
        let mut th = AdaptiveThrottle::new(100.0, 50.0);
        for _ in 0..20 { th.record_and_adapt(200.0); }
        assert!(th.rate_limit < 100.0);
    }

    #[test]
    fn test_throttle_adapt_increase() {
        let mut th = AdaptiveThrottle::new(100.0, 500.0);
        for _ in 0..20 { th.record_and_adapt(10.0); }
        assert!(th.rate_limit > 100.0);
    }

    #[test]
    fn test_throttle_set_rate() {
        let mut th = AdaptiveThrottle::new(100.0, 100.0);
        th.set_rate(500.0);
        assert!((th.rate_limit - 500.0).abs() < 1e-10);
    }

    #[test]
    fn test_throttle_set_rate_clamped() {
        let mut th = AdaptiveThrottle::new(100.0, 100.0);
        th.set_rate(0.1);
        assert!((th.rate_limit - th.min_rate).abs() < 1e-10);
    }

    #[test]
    fn test_throttle_reset() {
        let mut th = AdaptiveThrottle::new(100.0, 100.0);
        th.record_and_adapt(999.0);
        th.reset();
        assert!(th.tracker.is_empty());
    }

    #[test]
    fn test_percentile_single_value() {
        let mut t = LatencyTracker::new(100);
        t.record(42.0);
        assert!((t.percentile(50.0) - 42.0).abs() < 1e-10);
        assert!((t.percentile(99.0) - 42.0).abs() < 1e-10);
    }

    #[test]
    fn test_throttle_min_rate_enforced() {
        let mut th = AdaptiveThrottle::new(2.0, 10.0);
        for _ in 0..100 { th.record_and_adapt(10000.0); }
        assert!(th.rate_limit >= th.min_rate);
    }
}
