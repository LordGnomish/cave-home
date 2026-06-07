//! PQC envelope crypto: AES-256-GCM data key wrapped by an ML-KEM-768 KEK.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests for the envelope are written first; the implementation that
// makes them pass lands in the paired `feat` commit. Until then this file is
// test-only and the crate's `cargo test` build fails to resolve these symbols.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A deterministic, non-zero 64-byte ML-KEM seed for a test KEK.
    fn kek(tag: u8) -> KekKeypair {
        let mut seed = [0u8; KEK_SEED_LEN];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = tag ^ (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        KekKeypair::from_seed(seed)
    }

    /// Fixed sealing randomness so `seal` is a pure function in tests.
    fn fixed_randomness() -> SealRandomness {
        SealRandomness {
            data_key: [0x11; DATA_KEY_LEN],
            data_nonce: [0x22; NONCE_LEN],
            kem_message: [0x33; KEM_MESSAGE_LEN],
            wrap_nonce: [0x44; NONCE_LEN],
        }
    }

    #[test]
    fn seal_then_open_roundtrips() {
        let kp = kek(1);
        let pt = b"the-spice-must-flow";
        let blob = seal(pt, &kp.public(), b"v1", &fixed_randomness()).unwrap();
        let recovered = open(&blob, &kp, b"v1").unwrap();
        assert_eq!(recovered, pt);
    }

    #[test]
    fn empty_plaintext_roundtrips() {
        let kp = kek(2);
        let blob = seal(b"", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        assert_eq!(open(&blob, &kp, b"v1").unwrap(), b"");
    }

    #[test]
    fn blob_carries_pqc_magic_and_minimum_size() {
        let kp = kek(3);
        let blob = seal(b"x", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        assert_eq!(&blob[..MAGIC.len()], MAGIC, "blob must start with PQC magic");
        assert!(blob.len() >= HEADER_LEN, "blob shorter than fixed header");
    }

    #[test]
    fn seal_is_deterministic_given_randomness() {
        let kp = kek(4);
        let r = fixed_randomness();
        let a = seal(b"abc", &kp.public(), b"v1", &r).unwrap();
        let b = seal(b"abc", &kp.public(), b"v1", &r).unwrap();
        assert_eq!(a, b, "same plaintext+kek+aad+randomness ⇒ identical blob");
    }

    #[test]
    fn distinct_data_keys_yield_distinct_ciphertext() {
        let kp = kek(5);
        let mut r2 = fixed_randomness();
        r2.data_key = [0x99; DATA_KEY_LEN];
        let a = seal(b"abc", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        let b = seal(b"abc", &kp.public(), b"v1", &r2).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn open_rejects_wrong_kek() {
        let kp = kek(6);
        let other = kek(7);
        let blob = seal(b"secret", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        assert!(matches!(
            open(&blob, &other, b"v1"),
            Err(EnvelopeError::Aead)
        ));
    }

    #[test]
    fn open_rejects_wrong_aad() {
        let kp = kek(8);
        let blob = seal(b"secret", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        assert!(matches!(open(&blob, &kp, b"v2"), Err(EnvelopeError::Aead)));
    }

    #[test]
    fn open_rejects_bad_magic() {
        let kp = kek(9);
        let mut blob = seal(b"secret", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        blob[0] ^= 0xFF;
        assert!(matches!(open(&blob, &kp, b"v1"), Err(EnvelopeError::BadMagic)));
    }

    #[test]
    fn open_rejects_truncated() {
        let kp = kek(10);
        let blob = seal(b"secret", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        let short = &blob[..HEADER_LEN - 1];
        assert!(matches!(open(short, &kp, b"v1"), Err(EnvelopeError::Truncated)));
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let kp = kek(11);
        let mut blob = seal(b"secret-data", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(matches!(open(&blob, &kp, b"v1"), Err(EnvelopeError::Aead)));
    }

    #[test]
    fn open_rejects_tampered_wrapped_dek() {
        let kp = kek(12);
        let mut blob = seal(b"secret-data", &kp.public(), b"v1", &fixed_randomness()).unwrap();
        // Flip a byte inside the wrapped-DEK region (after magic+kem_ct+wrap_nonce).
        let off = MAGIC.len() + ML_KEM_768_CT_LEN + NONCE_LEN;
        blob[off] ^= 0x01;
        assert!(matches!(open(&blob, &kp, b"v1"), Err(EnvelopeError::Aead)));
    }

    #[test]
    fn public_key_serialization_roundtrips() {
        let kp = kek(13);
        let bytes = kp.public().to_bytes();
        assert_eq!(bytes.len(), ML_KEM_768_EK_LEN);
        let pk = KekPublic::from_bytes(&bytes).unwrap();
        // A public key recovered from bytes must seal blobs the original key opens.
        let blob = seal(b"hi", &pk, b"v1", &fixed_randomness()).unwrap();
        assert_eq!(open(&blob, &kp, b"v1").unwrap(), b"hi");
    }

    #[test]
    fn from_bytes_rejects_short_public_key() {
        assert!(matches!(
            KekPublic::from_bytes(&[0u8; 10]),
            Err(EnvelopeError::BadPublicKey)
        ));
    }

    #[test]
    fn seed_roundtrips_the_keypair() {
        let seed = [0x5A; KEK_SEED_LEN];
        let a = KekKeypair::from_seed(seed);
        let b = KekKeypair::from_seed(a.seed());
        assert_eq!(a.public().to_bytes(), b.public().to_bytes());
        assert_eq!(a.seed(), seed);
    }

    #[test]
    fn generate_randomness_is_full_length() {
        let r = SealRandomness::generate();
        // Overwhelmingly improbable that OS entropy is all-zero across 88 bytes.
        let all_zero = r.data_key == [0; DATA_KEY_LEN]
            && r.kem_message == [0; KEM_MESSAGE_LEN]
            && r.data_nonce == [0; NONCE_LEN]
            && r.wrap_nonce == [0; NONCE_LEN];
        assert!(!all_zero);
    }
}
