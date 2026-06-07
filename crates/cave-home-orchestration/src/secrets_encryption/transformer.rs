//! The stored-value transformer: the prefix scheme + identity fallback.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::secrets_encryption::envelope::KekKeypair;
    use crate::secrets_encryption::keyring::{KeyId, Keyring};
    use crate::secrets_encryption::provider::LocalKmsProvider;

    fn provider() -> LocalKmsProvider {
        let ring = Keyring::new(KeyId::new("key-1").unwrap(), KekKeypair::from_seed([1; 64]));
        LocalKmsProvider::new(ring)
    }

    fn enc() -> Transformer<LocalKmsProvider> {
        Transformer::new(provider(), WriteMode::Encrypt)
    }

    fn identity() -> Transformer<LocalKmsProvider> {
        Transformer::new(provider(), WriteMode::Identity)
    }

    #[test]
    fn encrypt_roundtrips_through_storage() {
        let t = enc();
        let stored = t.to_storage(b"my-secret").unwrap();
        assert_eq!(t.from_storage(&stored).unwrap(), b"my-secret");
    }

    #[test]
    fn encrypted_value_carries_ascii_prefix_with_key_id() {
        let t = enc();
        let stored = t.to_storage(b"x").unwrap();
        let want = b"cave:enc:mlkem768:v1:key-1:";
        assert_eq!(&stored[..want.len()], want);
        assert_eq!(ENC_PREFIX, "cave:enc:mlkem768:v1:");
    }

    #[test]
    fn identity_value_carries_identity_prefix_and_passes_through() {
        let t = identity();
        let stored = t.to_storage(b"plain").unwrap();
        assert_eq!(&stored[..IDENTITY_PREFIX.len()], IDENTITY_PREFIX.as_bytes());
        assert_eq!(&stored[IDENTITY_PREFIX.len()..], b"plain");
        assert_eq!(t.from_storage(&stored).unwrap(), b"plain");
    }

    #[test]
    fn encrypt_transformer_reads_identity_values() {
        // Disabled→enabled migration: an Encrypt-mode reader still reads
        // identity-written values.
        let written = identity().to_storage(b"legacy").unwrap();
        assert_eq!(enc().from_storage(&written).unwrap(), b"legacy");
    }

    #[test]
    fn identity_transformer_reads_encrypted_values() {
        // Enabled→disabled migration: an Identity-mode reader still decrypts
        // previously-encrypted values (the keyring is retained).
        let written = enc().to_storage(b"sensitive").unwrap();
        assert_eq!(identity().from_storage(&written).unwrap(), b"sensitive");
    }

    #[test]
    fn from_storage_rejects_unknown_prefix() {
        let t = enc();
        assert!(matches!(
            t.from_storage(b"k8s:enc:aescbc:v1:key:..."),
            Err(TransformError::UnknownPrefix)
        ));
    }

    #[test]
    fn from_storage_rejects_malformed_missing_key_id_delimiter() {
        let t = enc();
        // Has the encrypt prefix but no ':' terminating the key id.
        let mut bad = ENC_PREFIX.as_bytes().to_vec();
        bad.extend_from_slice(b"key-1-no-delimiter-then-blob");
        assert!(matches!(t.from_storage(&bad), Err(TransformError::Malformed)));
    }

    #[test]
    fn from_storage_rejects_tampered_ciphertext() {
        let t = enc();
        let mut stored = t.to_storage(b"secret").unwrap();
        let last = stored.len() - 1;
        stored[last] ^= 0x01;
        assert!(matches!(t.from_storage(&stored), Err(TransformError::Provider(_))));
    }

    #[test]
    fn rotation_updates_prefix_key_id_but_old_values_still_read() {
        let mut t = enc();
        let old = t.to_storage(b"under-1").unwrap();
        t.provider_mut()
            .keyring_mut()
            .rotate_keys(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        let new = t.to_storage(b"under-2").unwrap();
        assert_eq!(&new[..b"cave:enc:mlkem768:v1:key-2:".len()], b"cave:enc:mlkem768:v1:key-2:");
        assert_eq!(t.from_storage(&old).unwrap(), b"under-1");
        assert_eq!(t.from_storage(&new).unwrap(), b"under-2");
    }
}
