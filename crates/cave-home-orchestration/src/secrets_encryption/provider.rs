//! The KMS provider interface and its in-process implementation.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::secrets_encryption::envelope::{KekKeypair, SealRandomness};
    use crate::secrets_encryption::keyring::{KeyId, Keyring};

    fn provider() -> LocalKmsProvider {
        let ring = Keyring::new(KeyId::new("key-1").unwrap(), KekKeypair::from_seed([1; 64]));
        LocalKmsProvider::new(ring)
    }

    fn det_randomness() -> SealRandomness {
        SealRandomness {
            data_key: [0xA1; 32],
            data_nonce: [0xA2; 12],
            kem_message: [0xA3; 32],
            wrap_nonce: [0xA4; 12],
        }
    }

    #[test]
    fn encrypt_then_decrypt_roundtrips() {
        let p = provider();
        let obj = p.encrypt(b"top-secret").unwrap();
        assert_eq!(p.decrypt(&obj).unwrap(), b"top-secret");
    }

    #[test]
    fn encrypt_tags_the_current_write_key() {
        let p = provider();
        let obj = p.encrypt(b"x").unwrap();
        assert_eq!(obj.key_id.as_str(), "key-1");
    }

    #[test]
    fn decrypt_unknown_key_is_rejected() {
        let p = provider();
        let mut obj = p.encrypt(b"x").unwrap();
        obj.key_id = KeyId::new("does-not-exist").unwrap();
        assert!(matches!(p.decrypt(&obj), Err(ProviderError::UnknownKey)));
    }

    #[test]
    fn decrypt_rejects_tampered_blob() {
        let p = provider();
        let mut obj = p.encrypt(b"secret").unwrap();
        let last = obj.blob.len() - 1;
        obj.blob[last] ^= 0x01;
        assert!(matches!(p.decrypt(&obj), Err(ProviderError::Envelope(_))));
    }

    #[test]
    fn status_reports_v2_health_keyid_and_algorithm() {
        let p = provider();
        let s = p.status();
        assert_eq!(s.version, "v2");
        assert!(s.healthy);
        assert_eq!(s.current_key_id.as_str(), "key-1");
        assert_eq!(s.algorithm, "ML-KEM-768+AES-256-GCM");
    }

    #[test]
    fn encrypt_with_is_deterministic() {
        let p = provider();
        let a = p.encrypt_with(b"abc", &det_randomness()).unwrap();
        let b = p.encrypt_with(b"abc", &det_randomness()).unwrap();
        assert_eq!(a.blob, b.blob);
        assert_eq!(a.key_id, b.key_id);
    }

    #[test]
    fn rotation_changes_write_key_but_old_values_still_decrypt() {
        let mut p = provider();
        // Encrypt under key-1.
        let old = p.encrypt(b"written-under-key-1").unwrap();
        assert_eq!(old.key_id.as_str(), "key-1");

        // Rotate to key-2 (prepare + rotate); key-1 retained as a read key.
        p.keyring_mut()
            .rotate_keys(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();

        // New writes are tagged key-2...
        let new = p.encrypt(b"written-under-key-2").unwrap();
        assert_eq!(new.key_id.as_str(), "key-2");

        // ...and both old and new values decrypt.
        assert_eq!(p.decrypt(&old).unwrap(), b"written-under-key-1");
        assert_eq!(p.decrypt(&new).unwrap(), b"written-under-key-2");
    }

    #[test]
    fn usable_as_trait_object() {
        let p = provider();
        let dynp: &dyn KmsProvider = &p;
        let obj = dynp.encrypt(b"via-dyn").unwrap();
        assert_eq!(dynp.decrypt(&obj).unwrap(), b"via-dyn");
        assert_eq!(dynp.status().algorithm, "ML-KEM-768+AES-256-GCM");
    }
}
