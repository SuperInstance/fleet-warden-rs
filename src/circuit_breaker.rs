//! Circuit breaker pattern: Closed → Open → HalfOpen state machine.

use std::time::Instant;

/// Circuit breaker state.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Configuration for a circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,
    pub recovery_timeout_secs: f64,
    pub success_threshold: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_secs: 30.0,
            success_threshold: 3,
        }
    }
}

pub struct CircuitBreaker {
    pub config: CircuitBreakerConfig,
    pub state: CircuitState,
    failure_count: usize,
    success_count: usize,
    opened_at: Option<Instant>,
    total_successes: u64,
    total_failures: u64,
    total_rejections: u64,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            opened_at: None,
            total_successes: 0,
            total_failures: 0,
            total_rejections: 0,
        }
    }

    pub fn default_breaker() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(opened) = self.opened_at
                    && opened.elapsed().as_secs_f64() >= self.config.recovery_timeout_secs
                {
                    self.state = CircuitState::HalfOpen;
                    self.success_count = 0;
                    return true;
                }
                self.total_rejections += 1;
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    pub fn record_success(&mut self) {
        self.total_successes += 1;
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                }
            }
            CircuitState::Open => {}
        }
    }

    pub fn record_failure(&mut self) {
        self.total_failures += 1;
        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    self.state = CircuitState::Open;
                    self.opened_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                self.state = CircuitState::Open;
                self.opened_at = Some(Instant::now());
                self.success_count = 0;
            }
            CircuitState::Open => {}
        }
    }

    pub fn trip(&mut self) {
        self.state = CircuitState::Open;
        self.opened_at = Some(Instant::now());
    }

    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.opened_at = None;
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.total_successes, self.total_failures, self.total_rejections)
    }

    pub fn failure_count(&self) -> usize {
        self.failure_count
    }

    pub fn success_count(&self) -> usize {
        self.success_count
    }

    pub fn is_open(&self) -> bool {
        self.state == CircuitState::Open
    }

    pub fn is_closed(&self) -> bool {
        self.state == CircuitState::Closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_closed() {
        let cb = CircuitBreaker::default_breaker();
        assert_eq!(cb.state, CircuitState::Closed);
        assert!(cb.is_closed());
    }

    #[test]
    fn test_closed_allows_requests() {
        let mut cb = CircuitBreaker::default_breaker();
        assert!(cb.allow_request());
    }

    #[test]
    fn test_failures_open_circuit() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state, CircuitState::Open);
    }

    #[test]
    fn test_open_rejects_requests() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 3600.0,
            ..Default::default()
        });
        cb.record_failure();
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_success_resets_failure_count() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_trip_force_opens() {
        let mut cb = CircuitBreaker::default_breaker();
        cb.trip();
        assert_eq!(cb.state, CircuitState::Open);
    }

    #[test]
    fn test_reset_force_closes() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 3600.0,
            ..Default::default()
        });
        cb.record_failure();
        cb.reset();
        assert!(cb.is_closed());
    }

    #[test]
    fn test_stats_tracking() {
        let mut cb = CircuitBreaker::default_breaker();
        cb.record_success();
        cb.record_success();
        cb.record_failure();
        let (s, f, r) = cb.stats();
        assert_eq!((s, f, r), (2, 1, 0));
    }

    #[test]
    fn test_half_open_then_close() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0.0,
            success_threshold: 2,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new(config);
        cb.record_failure();
        assert!(cb.is_open());
        assert!(cb.allow_request());
        assert_eq!(cb.state, CircuitState::HalfOpen);
        cb.record_success();
        cb.record_success();
        assert!(cb.is_closed());
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0.0,
            success_threshold: 3,
            ..Default::default()
        };
        let mut cb = CircuitBreaker::new(config);
        cb.record_failure();
        cb.allow_request();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);
    }

    #[test]
    fn test_rejection_count_increments() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 3600.0,
            ..Default::default()
        });
        cb.record_failure();
        cb.allow_request();
        cb.allow_request();
        let (_, _, r) = cb.stats();
        assert_eq!(r, 2);
    }

    #[test]
    fn test_below_threshold_stays_closed() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        });
        for _ in 0..4 {
            cb.record_failure();
        }
        assert!(cb.is_closed());
    }
}
