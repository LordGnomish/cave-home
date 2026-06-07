//! Observability: encryption op latency, decryption error rate, key age.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

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
