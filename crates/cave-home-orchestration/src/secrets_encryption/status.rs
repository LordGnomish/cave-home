//! The encryption status view-model — the cavectl / Portal data contract.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::secrets_encryption::envelope::KekKeypair;
    use crate::secrets_encryption::keyring::{KeyId, Keyring, RotationPhase};
    use crate::secrets_encryption::provider::LocalKmsProvider;

    fn provider() -> LocalKmsProvider {
        let ring = Keyring::new(KeyId::new("key-1").unwrap(), KekKeypair::from_seed([1; 64]));
        LocalKmsProvider::new(ring)
    }

    #[test]
    fn status_of_steady_encrypted_provider() {
        let p = provider();
        let s = EncryptionStatus::from_provider(&p, true);
        assert!(s.enabled);
        assert_eq!(s.algorithm, "ML-KEM-768+AES-256-GCM");
        assert_eq!(s.write_key_id, "key-1");
        assert_eq!(s.key_count, 1);
        assert_eq!(s.read_only_key_count, 0);
        assert_eq!(s.rotation_phase, RotationPhase::Steady);
    }

    #[test]
    fn status_after_rotation() {
        let mut p = provider();
        p.keyring_mut()
            .rotate_keys(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        let s = EncryptionStatus::from_provider(&p, true);
        assert_eq!(s.write_key_id, "key-2");
        assert_eq!(s.key_count, 2);
        assert_eq!(s.read_only_key_count, 1);
        assert_eq!(s.rotation_phase, RotationPhase::Rotated);
    }

    #[test]
    fn disabled_status() {
        let p = provider();
        let s = EncryptionStatus::from_provider(&p, false);
        assert!(!s.enabled);
    }

    #[test]
    fn summary_line_mentions_key_and_algorithm() {
        let p = provider();
        let s = EncryptionStatus::from_provider(&p, true);
        let line = s.summary_line();
        assert!(line.contains("key-1"));
        assert!(line.contains("ML-KEM-768"));
    }
}
