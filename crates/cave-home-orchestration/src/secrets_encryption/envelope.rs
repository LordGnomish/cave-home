//! PQC envelope crypto: AES-256-GCM data key wrapped by an ML-KEM-768 KEK.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! The on-disk blob is self-describing and fixed-layout:
//!
//! ```text
//! ┌────────┬─────────────┬────────────┬──────────────┬────────────┬───────────┐
//! │ magic  │  kem_ct     │ wrap_nonce │ wrapped_dek  │ data_nonce │  data_ct  │
//! │  8 B   │  1088 B     │   12 B     │    48 B      │    12 B    │   var     │
//! └────────┴─────────────┴────────────┴──────────────┴────────────┴───────────┘
//! ```
//!
//! * `kem_ct` — ML-KEM-768 (FIPS 203) encapsulation ciphertext.
//! * `wrapped_dek` — the 32-byte AES-256 data key, AES-256-GCM-sealed (32 + 16
//!   tag) under an HKDF-SHA256(shared-secret) wrapping key.
//! * `data_ct` — the plaintext, AES-256-GCM-sealed under the data key.
//!
//! Both AEAD layers authenticate the caller-supplied AAD (the transformer binds
//! the key id there), so a blob can only be opened under the key id that wrote
//! it.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key as AesKey, Nonce};
use hkdf::Hkdf;
use ml_kem::array::Array;
use ml_kem::kem::{Decapsulate, KeyExport};
use ml_kem::{B32, Ciphertext, DecapsulationKey768, EncapsulationKey, EncapsulationKey768};
use ml_kem::{MlKem768, Seed};
use sha2::Sha256;
use zeroize::Zeroizing;

/// ML-KEM-768 encapsulation-key (public) encoded length, FIPS 203.
pub const ML_KEM_768_EK_LEN: usize = 1184;
/// ML-KEM-768 ciphertext encoded length, FIPS 203.
pub const ML_KEM_768_CT_LEN: usize = 1088;
/// ML-KEM seed length (d ‖ z, FIPS 203) — the compact private-key form.
pub const KEK_SEED_LEN: usize = 64;
/// AES-256 data-encryption-key length.
pub const DATA_KEY_LEN: usize = 32;
/// AES-GCM nonce length.
pub const NONCE_LEN: usize = 12;
/// ML-KEM encapsulation message length (the `m` in FIPS 203 §7.2).
pub const KEM_MESSAGE_LEN: usize = 32;
/// AES-256-GCM authentication-tag length.
const TAG_LEN: usize = 16;
/// Wrapped-DEK length: the 32-byte DEK plus its GCM tag.
const WRAPPED_DEK_LEN: usize = DATA_KEY_LEN + TAG_LEN;

/// Envelope wire-format magic: "Cave PQ Envelope, v1".
pub const MAGIC: &[u8; 8] = b"CAVEPQE1";

/// Fixed-size header length — everything up to (not including) the data
/// ciphertext. A blob shorter than this is structurally invalid.
pub const HEADER_LEN: usize =
    MAGIC.len() + ML_KEM_768_CT_LEN + NONCE_LEN + WRAPPED_DEK_LEN + NONCE_LEN;

/// HKDF salt — domain-separates this KDF from any other cave-home use.
const HKDF_SALT: &[u8] = b"cave-home/secrets-encryption/mlkem768/v1";
/// HKDF info — labels the derived bytes as a DEK-wrapping key.
const HKDF_INFO: &[u8] = b"dek-wrap";

/// Why an envelope operation failed.
///
/// Authentication failures (wrong KEK, wrong AAD, tampered ciphertext, tampered
/// wrapped-DEK) all surface as [`Self::Aead`] — the AEAD layer is deliberately
/// indistinguishable about *why* it rejected a value, to avoid an oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeError {
    /// The blob did not start with the expected [`MAGIC`].
    BadMagic,
    /// The blob is shorter than the fixed [`HEADER_LEN`].
    Truncated,
    /// A public KEK could not be decoded (wrong length / invalid encoding).
    BadPublicKey,
    /// The embedded KEM ciphertext could not be decoded.
    BadCiphertext,
    /// An AEAD seal/open failed — wrong key, wrong AAD, or a tampered blob.
    Aead,
}

impl core::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::BadMagic => "envelope: bad magic",
            Self::Truncated => "envelope: truncated blob",
            Self::BadPublicKey => "envelope: malformed KEK public key",
            Self::BadCiphertext => "envelope: malformed KEM ciphertext",
            Self::Aead => "envelope: authentication failed",
        };
        f.write_str(s)
    }
}

impl std::error::Error for EnvelopeError {}

/// An ML-KEM-768 KEK key pair.
///
/// Holds the decapsulation (private) key plus the 64-byte seed it was derived
/// from, so the keyring can persist it compactly. The seed is zeroized on drop;
/// the underlying ML-KEM key zeroizes itself.
pub struct KekKeypair {
    seed: Zeroizing<[u8; KEK_SEED_LEN]>,
    dk: DecapsulationKey768,
}

impl KekKeypair {
    /// Deterministically derive a KEK key pair from a 64-byte ML-KEM seed.
    #[must_use]
    pub fn from_seed(seed: [u8; KEK_SEED_LEN]) -> Self {
        let seed_array: Seed = Array(seed);
        let dk = DecapsulationKey768::from_seed(seed_array);
        Self { seed: Zeroizing::new(seed), dk }
    }

    /// The public encapsulation key — safe to share; secrets seal to it.
    #[must_use]
    pub fn public(&self) -> KekPublic {
        KekPublic { ek: self.dk.encapsulation_key().clone() }
    }

    /// The 64-byte seed, for compact persistence of the private key.
    #[must_use]
    pub fn seed(&self) -> [u8; KEK_SEED_LEN] {
        *self.seed
    }
}

impl core::fmt::Debug for KekKeypair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print key material.
        f.write_str("KekKeypair(<redacted>)")
    }
}

/// The public half of a KEK — an ML-KEM-768 encapsulation key.
#[derive(Clone, Debug)]
pub struct KekPublic {
    ek: EncapsulationKey768,
}

impl KekPublic {
    /// Encode the public key as [`ML_KEM_768_EK_LEN`] bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.ek.to_bytes().as_slice().to_vec()
    }

    /// Decode a public key from its encoded form.
    ///
    /// # Errors
    /// Returns [`EnvelopeError::BadPublicKey`] if `bytes` is the wrong length or
    /// not a valid ML-KEM-768 encapsulation key.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        let array = Array::try_from(bytes).map_err(|_| EnvelopeError::BadPublicKey)?;
        let ek = EncapsulationKey::<MlKem768>::new(&array).map_err(|_| EnvelopeError::BadPublicKey)?;
        Ok(Self { ek })
    }
}

/// All randomness an envelope seal consumes, made explicit so [`seal`] is a pure
/// function — essential for known-answer tests and reproducible behaviour. In
/// production use [`SealRandomness::generate`].
pub struct SealRandomness {
    /// The AES-256 data-encryption key.
    pub data_key: [u8; DATA_KEY_LEN],
    /// The AES-GCM nonce for the data ciphertext.
    pub data_nonce: [u8; NONCE_LEN],
    /// The ML-KEM encapsulation message `m`.
    pub kem_message: [u8; KEM_MESSAGE_LEN],
    /// The AES-GCM nonce for the wrapped DEK.
    pub wrap_nonce: [u8; NONCE_LEN],
}

impl SealRandomness {
    /// Draw fresh randomness from the operating-system CSPRNG.
    #[must_use]
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut rng = rand::rngs::OsRng;
        let mut r = Self {
            data_key: [0u8; DATA_KEY_LEN],
            data_nonce: [0u8; NONCE_LEN],
            kem_message: [0u8; KEM_MESSAGE_LEN],
            wrap_nonce: [0u8; NONCE_LEN],
        };
        rng.fill_bytes(&mut r.data_key);
        rng.fill_bytes(&mut r.data_nonce);
        rng.fill_bytes(&mut r.kem_message);
        rng.fill_bytes(&mut r.wrap_nonce);
        r
    }
}

/// Seal `plaintext` to `kek`, binding `aad` (the transformer's key id) into both
/// AEAD layers. `r` supplies every random input.
///
/// # Errors
/// Returns [`EnvelopeError::Aead`] if any AEAD/KDF step fails (in practice only
/// on pathological inputs — the happy path never fails for valid keys).
pub fn seal(
    plaintext: &[u8],
    kek: &KekPublic,
    aad: &[u8],
    r: &SealRandomness,
) -> Result<Vec<u8>, EnvelopeError> {
    // 1. ML-KEM encapsulate (deterministic in the supplied message `m`).
    let m: B32 = Array(r.kem_message);
    let (kem_ct, shared) = kek.ek.encapsulate_deterministic(&m);

    // 2. HKDF-derive the DEK-wrapping key from the KEM shared secret.
    let wrap_key = derive_wrap_key(shared.as_slice())?;

    // 3. Wrap the DEK, then 4. encrypt the data — both under `aad`.
    let dek = Zeroizing::new(r.data_key);
    let wrapped_dek = aes_seal(&wrap_key[..], &r.wrap_nonce, &dek[..], aad)?;
    let data_ct = aes_seal(&dek[..], &r.data_nonce, plaintext, aad)?;

    // 5. Assemble the fixed-layout blob.
    let mut blob = Vec::with_capacity(HEADER_LEN + data_ct.len());
    blob.extend_from_slice(MAGIC);
    blob.extend_from_slice(kem_ct.as_ref());
    blob.extend_from_slice(&r.wrap_nonce);
    blob.extend_from_slice(&wrapped_dek);
    blob.extend_from_slice(&r.data_nonce);
    blob.extend_from_slice(&data_ct);
    Ok(blob)
}

/// Open a blob sealed by [`seal`], using `kek`'s private half and the same
/// `aad`.
///
/// # Errors
/// - [`EnvelopeError::Truncated`] / [`EnvelopeError::BadMagic`] for a
///   structurally invalid blob;
/// - [`EnvelopeError::BadCiphertext`] if the KEM ciphertext won't decode;
/// - [`EnvelopeError::Aead`] for a wrong key, wrong AAD, or tampered blob.
pub fn open(blob: &[u8], kek: &KekKeypair, aad: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    if blob.len() < HEADER_LEN {
        return Err(EnvelopeError::Truncated);
    }
    let (magic, rest) = blob.split_at(MAGIC.len());
    if magic != MAGIC.as_slice() {
        return Err(EnvelopeError::BadMagic);
    }
    let (kem_ct_bytes, rest) = rest.split_at(ML_KEM_768_CT_LEN);
    let (wrap_nonce, rest) = rest.split_at(NONCE_LEN);
    let (wrapped_dek, rest) = rest.split_at(WRAPPED_DEK_LEN);
    let (data_nonce, data_ct) = rest.split_at(NONCE_LEN);

    // 1. Rebuild the KEM ciphertext and decapsulate to the shared secret.
    let kem_ct: Ciphertext<MlKem768> =
        Array::try_from(kem_ct_bytes).map_err(|_| EnvelopeError::BadCiphertext)?;
    let shared = kek.dk.decapsulate(&kem_ct);

    // 2. Derive the wrapping key and unwrap the DEK.
    let wrap_key = derive_wrap_key(shared.as_slice())?;
    let dek = Zeroizing::new(aes_open(&wrap_key[..], wrap_nonce, wrapped_dek, aad)?);

    // 3. Decrypt the data under the recovered DEK.
    aes_open(&dek[..], data_nonce, data_ct, aad)
}

/// HKDF-SHA256 a 32-byte AES-256 wrapping key from the KEM shared secret.
fn derive_wrap_key(shared: &[u8]) -> Result<Zeroizing<[u8; DATA_KEY_LEN]>, EnvelopeError> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), shared);
    let mut out = Zeroizing::new([0u8; DATA_KEY_LEN]);
    hk.expand(HKDF_INFO, &mut out[..])
        .map_err(|_| EnvelopeError::Aead)?;
    Ok(out)
}

/// AES-256-GCM seal. `key` is 32 bytes, `nonce` is 12 bytes (both guaranteed by
/// the call sites).
fn aes_seal(key: &[u8], nonce: &[u8], msg: &[u8], aad: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let cipher = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(key));
    cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg, aad })
        .map_err(|_| EnvelopeError::Aead)
}

/// AES-256-GCM open. Returns [`EnvelopeError::Aead`] on any authentication
/// failure.
fn aes_open(key: &[u8], nonce: &[u8], ct: &[u8], aad: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let cipher = Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| EnvelopeError::Aead)
}

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
        assert!(matches!(open(&blob, &other, b"v1"), Err(EnvelopeError::Aead)));
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
