//! Anomaly detection using Z-score, MAD, and IQR methods.

/// Detect anomalies using Z-score method.
/// Returns indices of values whose absolute Z-score exceeds the threshold.
pub fn z_score_anomalies(data: &[f64], threshold: f64) -> Vec<usize> {
    if data.len() < 2 {
        return Vec::new();
    }
    let n = data.len() as f64;
    let mean = data.iter().sum::<f64>() / n;
    let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if std_dev < 1e-10 {
        return Vec::new();
    }
    data.iter()
        .enumerate()
        .filter(|&(_, x)| ((x - mean) / std_dev).abs() > threshold)
        .map(|(i, _)| i)
        .collect()
}

/// Compute the median of a sorted or unsorted slice.
pub fn median(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// Detect anomalies using Median Absolute Deviation (MAD).
/// MAD is more robust to outliers than Z-score.
pub fn mad_anomalies(data: &[f64], threshold: f64) -> Vec<usize> {
    if data.len() < 2 {
        return Vec::new();
    }
    let med = median(data);
    let deviations: Vec<f64> = data.iter().map(|x| (x - med).abs()).collect();
    let mad = median(&deviations);
    let scaled_mad = mad * 1.4826;
    if scaled_mad < 1e-10 {
        return Vec::new();
    }
    data.iter()
        .enumerate()
        .filter(|&(_, x)| ((x - med) / scaled_mad).abs() > threshold)
        .map(|(i, _)| i)
        .collect()
}

/// Compute quartiles (Q1, Q2/median, Q3) from data.
pub fn quartiles(data: &[f64]) -> (f64, f64, f64) {
    if data.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let q2 = median(&sorted);
    let (lower, upper) = if n.is_multiple_of(2) {
        (&sorted[..n / 2], &sorted[n / 2..])
    } else {
        (&sorted[..n / 2], &sorted[n / 2 + 1..])
    };
    let q1 = median(lower);
    let q3 = median(upper);
    (q1, q2, q3)
}

/// Detect anomalies using the Interquartile Range (IQR) method.
pub fn iqr_anomalies(data: &[f64], k: f64) -> Vec<usize> {
    if data.len() < 4 {
        return Vec::new();
    }
    let (q1, _, q3) = quartiles(data);
    let iqr = q3 - q1;
    let lower = q1 - k * iqr;
    let upper = q3 + k * iqr;
    data.iter()
        .enumerate()
        .filter(|&(_, x)| *x < lower || *x > upper)
        .map(|(i, _)| i)
        .collect()
}

/// A sliding-window anomaly detector that combines multiple methods.
pub struct AnomalyDetector {
    pub window: Vec<f64>,
    pub max_window: usize,
    pub z_threshold: f64,
    pub mad_threshold: f64,
    pub iqr_k: f64,
}

impl AnomalyDetector {
    pub fn new(max_window: usize) -> Self {
        Self {
            window: Vec::new(),
            max_window,
            z_threshold: 2.0,
            mad_threshold: 3.0,
            iqr_k: 1.5,
        }
    }

    /// Push a new value into the window.
    pub fn push(&mut self, value: f64) {
        if self.window.len() >= self.max_window {
            self.window.remove(0);
        }
        self.window.push(value);
    }

    /// Check if the latest value is an anomaly using any method.
    pub fn is_anomaly(&self) -> bool {
        if self.window.len() < 4 {
            return false;
        }
        let _last = *self.window.last().unwrap();
        let data = &self.window;
        let last_idx = data.len() - 1;

        let z_outliers = z_score_anomalies(data, self.z_threshold);
        if z_outliers.contains(&last_idx) {
            return true;
        }
        let mad_outliers = mad_anomalies(data, self.mad_threshold);
        if mad_outliers.contains(&last_idx) {
            return true;
        }
        let iqr_outliers = iqr_anomalies(data, self.iqr_k);
        if iqr_outliers.contains(&last_idx) {
            return true;
        }
        false
    }

    /// Get all anomalies in the current window.
    pub fn all_anomalies(&self) -> Vec<usize> {
        let mut result = std::collections::HashSet::new();
        for i in z_score_anomalies(&self.window, self.z_threshold) {
            result.insert(i);
        }
        for i in mad_anomalies(&self.window, self.mad_threshold) {
            result.insert(i);
        }
        for i in iqr_anomalies(&self.window, self.iqr_k) {
            result.insert(i);
        }
        let mut v: Vec<usize> = result.into_iter().collect();
        v.sort();
        v
    }

    /// Clear the window.
    pub fn clear(&mut self) {
        self.window.clear();
    }

    /// Current window size.
    pub fn len(&self) -> usize {
        self.window.len()
    }

    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_z_score_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 100.0];
        let anomalies = z_score_anomalies(&data, 2.0);
        assert!(anomalies.contains(&5));
    }

    #[test]
    fn test_z_score_no_anomalies() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!(z_score_anomalies(&data, 3.0).is_empty());
    }

    #[test]
    fn test_z_score_empty() {
        assert!(z_score_anomalies(&[], 2.0).is_empty());
    }

    #[test]
    fn test_z_score_constant() {
        assert!(z_score_anomalies(&[5.0, 5.0, 5.0, 5.0], 2.0).is_empty());
    }

    #[test]
    fn test_median_odd() {
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_median_even() {
        assert!((median(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_median_empty() {
        assert!((median(&[]) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_mad_anomalies() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 50.0];
        assert!(mad_anomalies(&data, 3.0).contains(&5));
    }

    #[test]
    fn test_mad_no_anomalies() {
        assert!(mad_anomalies(&[10.0, 11.0, 12.0, 13.0, 14.0], 3.0).is_empty());
    }

    #[test]
    fn test_quartiles() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (q1, q2, q3) = quartiles(&data);
        assert!((q2 - 3.0).abs() < 1e-10);
        assert!(q1 < q2);
        assert!(q3 > q2);
    }

    #[test]
    fn test_iqr_anomalies() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 100.0];
        assert!(iqr_anomalies(&data, 1.5).contains(&7));
    }

    #[test]
    fn test_iqr_no_anomalies() {
        assert!(iqr_anomalies(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 1.5).is_empty());
    }

    #[test]
    fn test_detector_push_and_is_anomaly() {
        let mut det = AnomalyDetector::new(20);
        for v in &[1.0, 2.0, 3.0, 4.0, 5.0] {
            det.push(*v);
        }
        assert!(!det.is_anomaly());
        det.push(100.0);
        assert!(det.is_anomaly());
    }

    #[test]
    fn test_detector_window_eviction() {
        let mut det = AnomalyDetector::new(5);
        for v in &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
            det.push(*v);
        }
        assert_eq!(det.len(), 5);
        assert!((det.window[0] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_detector_all_anomalies() {
        let mut det = AnomalyDetector::new(20);
        for v in &[1.0, 2.0, 3.0, 4.0, 5.0, 100.0, 6.0, -50.0] {
            det.push(*v);
        }
        assert!(!det.all_anomalies().is_empty());
    }

    #[test]
    fn test_detector_clear() {
        let mut det = AnomalyDetector::new(10);
        det.push(1.0);
        det.clear();
        assert!(det.is_empty());
    }

    #[test]
    fn test_detector_small_window() {
        let mut det = AnomalyDetector::new(10);
        det.push(1.0);
        det.push(2.0);
        assert!(!det.is_anomaly());
    }

    #[test]
    fn test_z_score_two_sided() {
        let data = vec![-100.0, 10.0, 11.0, 12.0, 12.0, 13.0, 14.0, 14.0, 15.0, 100.0];
        let anomalies = z_score_anomalies(&data, 2.0);
        assert!(anomalies.contains(&0));
        assert!(anomalies.len() >= 1);
    }
}
