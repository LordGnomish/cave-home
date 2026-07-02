//! Observability: encryption op latency, decryption error rate, key age.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! The crate is side-effect-free and clockless, so this is a pure in-memory
//! recorder the runtime feeds (timing each envelope op against its own clock)
//! plus a Prometheus-exposition renderer. It covers the three observability
//! signals the mandate calls for: **operation latency** (a histogram),
//! **decryption error rate** (a counter pair), and **key age** (per-key gauges,
//! fed caller-supplied ages since the crate has no clock).

use core::fmt::Write as _;

/// Histogram bucket upper bounds, in microseconds. A final implicit `+Inf`
/// bucket catches everything slower.
const BUCKETS_MICROS: [u64; 8] = [50, 100, 250, 500, 1000, 2500, 5000, 10000];

/// The age of one key, supplied by the caller (the crate has no clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAge {
    /// The key's id.
    pub key_id: String,
    /// Seconds since the key was created.
    pub age_secs: u64,
}

/// In-memory metrics for the encryption subsystem.
#[derive(Debug, Clone, Default)]
pub struct EncryptionMetrics {
    encrypt_ops: u64,
    decrypt_ops: u64,
    decrypt_errors: u64,
    // One slot per finite bucket plus a trailing `+Inf` slot. Non-cumulative;
    // rendered cumulatively.
    bucket_counts: [u64; BUCKETS_MICROS.len() + 1],
    latency_sum: u64,
    latency_count: u64,
}

impl EncryptionMetrics {
    /// Total successful encrypt operations.
    #[must_use]
    pub const fn encrypt_ops(&self) -> u64 {
        self.encrypt_ops
    }

    /// Total decrypt *attempts* (successes plus failures).
    #[must_use]
    pub const fn decrypt_ops(&self) -> u64 {
        self.decrypt_ops
    }

    /// Total decrypt failures.
    #[must_use]
    pub const fn decrypt_errors(&self) -> u64 {
        self.decrypt_errors
    }

    /// Number of latency observations recorded.
    #[must_use]
    pub const fn latency_count(&self) -> u64 {
        self.latency_count
    }

    /// Sum of observed latencies, in microseconds.
    #[must_use]
    pub const fn latency_sum_micros(&self) -> u64 {
        self.latency_sum
    }

    /// Fraction of decrypt attempts that failed, in `[0.0, 1.0]`; `0.0` if there
    /// were no attempts.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn decrypt_error_rate(&self) -> f64 {
        if self.decrypt_ops == 0 {
            0.0
        } else {
            self.decrypt_errors as f64 / self.decrypt_ops as f64
        }
    }

    /// Record a successful encrypt that took `latency_micros`.
    pub fn record_encrypt(&mut self, latency_micros: u64) {
        self.encrypt_ops += 1;
        self.observe(latency_micros);
    }

    /// Record a successful decrypt that took `latency_micros`.
    pub fn record_decrypt_ok(&mut self, latency_micros: u64) {
        self.decrypt_ops += 1;
        self.observe(latency_micros);
    }

    /// Record a failed decrypt attempt (no latency observed).
    pub const fn record_decrypt_error(&mut self) {
        self.decrypt_ops += 1;
        self.decrypt_errors += 1;
    }

    /// File one latency observation into its bucket.
    fn observe(&mut self, latency_micros: u64) {
        let idx = BUCKETS_MICROS
            .iter()
            .position(|&bound| latency_micros <= bound)
            .unwrap_or(BUCKETS_MICROS.len());
        self.bucket_counts[idx] += 1;
        self.latency_sum += latency_micros;
        self.latency_count += 1;
    }

    /// Render the metrics in Prometheus text-exposition format, including a
    /// `key_age_seconds` gauge per supplied [`KeyAge`].
    #[must_use]
    pub fn to_prometheus(&self, key_ages: &[KeyAge]) -> String {
        let mut out = String::new();

        counter(
            &mut out,
            "cave_home_secrets_encrypt_ops_total",
            "Total secret-value encryptions.",
            self.encrypt_ops,
        );
        counter(
            &mut out,
            "cave_home_secrets_decrypt_ops_total",
            "Total secret-value decrypt attempts.",
            self.decrypt_ops,
        );
        counter(
            &mut out,
            "cave_home_secrets_decrypt_errors_total",
            "Total secret-value decrypt failures.",
            self.decrypt_errors,
        );

        // Latency histogram (cumulative bucket counts).
        let metric = "cave_home_secrets_op_latency_micros";
        let _ = writeln!(out, "# HELP {metric} Envelope operation latency (microseconds).");
        let _ = writeln!(out, "# TYPE {metric} histogram");
        let mut cumulative = 0u64;
        for (i, &bound) in BUCKETS_MICROS.iter().enumerate() {
            cumulative += self.bucket_counts[i];
            let _ = writeln!(out, "{metric}_bucket{{le=\"{bound}\"}} {cumulative}");
        }
        cumulative += self.bucket_counts[BUCKETS_MICROS.len()];
        let _ = writeln!(out, "{metric}_bucket{{le=\"+Inf\"}} {cumulative}");
        let _ = writeln!(out, "{metric}_sum {}", self.latency_sum);
        let _ = writeln!(out, "{metric}_count {}", self.latency_count);

        // Per-key age gauges.
        let gauge = "cave_home_secrets_key_age_seconds";
        let _ = writeln!(out, "# HELP {gauge} Age of each encryption key (seconds).");
        let _ = writeln!(out, "# TYPE {gauge} gauge");
        for ka in key_ages {
            let _ = writeln!(out, "{gauge}{{key_id=\"{}\"}} {}", escape_label(&ka.key_id), ka.age_secs);
        }

        out
    }
}

/// Append a single Prometheus counter (HELP + TYPE + value).
fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} counter");
    let _ = writeln!(out, "{name} {value}");
}

/// Escape a Prometheus label value (`\`, `"`, newline).
fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::float_cmp)]

    use super::*;

    #[test]
    fn counters_start_at_zero() {
        let m = EncryptionMetrics::default();
        assert_eq!(m.encrypt_ops(), 0);
        assert_eq!(m.decrypt_ops(), 0);
        assert_eq!(m.decrypt_errors(), 0);
        assert_eq!(m.decrypt_error_rate(), 0.0);
    }

    #[test]
    fn records_encrypt_and_decrypt() {
        let mut m = EncryptionMetrics::default();
        m.record_encrypt(40);
        m.record_decrypt_ok(120);
        m.record_decrypt_ok(900);
        m.record_decrypt_error();
        assert_eq!(m.encrypt_ops(), 1);
        assert_eq!(m.decrypt_ops(), 3); // two ok + one error = three attempts
        assert_eq!(m.decrypt_errors(), 1);
        // one error out of three attempts.
        assert!((m.decrypt_error_rate() - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn latency_histogram_count_and_sum() {
        let mut m = EncryptionMetrics::default();
        m.record_encrypt(40);
        m.record_encrypt(300);
        m.record_decrypt_ok(70);
        assert_eq!(m.latency_count(), 3);
        assert_eq!(m.latency_sum_micros(), 410);
    }

    #[test]
    fn prometheus_exposition_has_all_series() {
        let mut m = EncryptionMetrics::default();
        m.record_encrypt(40);
        m.record_decrypt_ok(120);
        m.record_decrypt_error();
        let ages = vec![
            KeyAge { key_id: "key-1".to_owned(), age_secs: 3600 },
            KeyAge { key_id: "key-2".to_owned(), age_secs: 10 },
        ];
        let text = m.to_prometheus(&ages);

        assert!(text.contains("cave_home_secrets_encrypt_ops_total 1"));
        assert!(text.contains("cave_home_secrets_decrypt_ops_total 2"));
        assert!(text.contains("cave_home_secrets_decrypt_errors_total 1"));
        // histogram pieces
        assert!(text.contains("cave_home_secrets_op_latency_micros_bucket{le=\"+Inf\"}"));
        assert!(text.contains("cave_home_secrets_op_latency_micros_count"));
        assert!(text.contains("cave_home_secrets_op_latency_micros_sum"));
        // key-age gauges, labelled by key id
        assert!(text.contains("cave_home_secrets_key_age_seconds{key_id=\"key-1\"} 3600"));
        assert!(text.contains("cave_home_secrets_key_age_seconds{key_id=\"key-2\"} 10"));
        // HELP/TYPE metadata present
        assert!(text.contains("# TYPE cave_home_secrets_op_latency_micros histogram"));
    }

    #[test]
    fn histogram_buckets_are_cumulative() {
        let mut m = EncryptionMetrics::default();
        m.record_encrypt(40); // falls in the first finite bucket
        let text = m.to_prometheus(&[]);
        // every cumulative bucket at or above 40us must include that observation,
        // so the +Inf bucket count equals the total count (1).
        assert!(text.contains("cave_home_secrets_op_latency_micros_bucket{le=\"+Inf\"} 1"));
    }
}
