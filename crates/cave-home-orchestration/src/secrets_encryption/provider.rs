//! The KMS provider interface and its in-process implementation.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! Upstream Kubernetes models encryption providers behind an `EnvelopeService`
//! (`Encrypt` / `Decrypt` / `Status`) so a provider can be in-process *or* a
//! gRPC KMS plugin. [`KmsProvider`] is that object-safe seam, and
//! [`LocalKmsProvider`] is the in-process implementation over a [`Keyring`] of
//! ML-KEM-768 KEKs. An out-of-process gRPC transport implementing the same
//! trait is ADR-004 phase-1b.

use super::envelope::{self, EnvelopeError, SealRandomness};
use super::keyring::{KeyId, Keyring};

/// The algorithm label reported by [`KmsProvider::status`] — the PQC envelope.
pub const ALGORITHM: &str = "ML-KEM-768+AES-256-GCM";

/// Why a provider operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The object names a key id the provider's keyring does not hold.
    UnknownKey,
    /// The envelope layer failed (tampering, wrong key, malformed blob).
    Envelope(EnvelopeError),
}

impl core::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownKey => f.write_str("kms: no key in keyring for that key id"),
            Self::Envelope(e) => write!(f, "kms: {e}"),
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<EnvelopeError> for ProviderError {
    fn from(e: EnvelopeError) -> Self {
        Self::Envelope(e)
    }
}

/// A sealed object: the envelope blob plus the id of the key that sealed it.
///
/// Mirrors the Kubernetes KMS-v2 `EncryptedObject` (key id + ciphertext); the
/// [`crate::secrets_encryption::transformer`] serializes it into the prefixed
/// stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedObject {
    /// The write key the blob was sealed under — routes the decrypt.
    pub key_id: KeyId,
    /// The PQC envelope blob (see [`crate::secrets_encryption::envelope`]).
    pub blob: Vec<u8>,
}

/// A snapshot of provider health, for `status` reporting (KMS v2 `Status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    /// KMS API version implemented — always `"v2"` (envelope/DEK model).
    pub version: &'static str,
    /// Whether the provider can currently encrypt + decrypt.
    pub healthy: bool,
    /// The id of the current write key.
    pub current_key_id: KeyId,
    /// The envelope algorithm label.
    pub algorithm: &'static str,
}

/// The KMS provider interface — object-safe so it can be in-process or a gRPC
/// plugin behind `&dyn KmsProvider`.
pub trait KmsProvider {
    /// Seal `plaintext` under the current write key.
    ///
    /// # Errors
    /// [`ProviderError::Envelope`] if the envelope layer fails.
    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedObject, ProviderError>;

    /// Open an object previously produced by [`KmsProvider::encrypt`].
    ///
    /// # Errors
    /// [`ProviderError::UnknownKey`] if the keyring lacks `obj.key_id`;
    /// [`ProviderError::Envelope`] on any authentication failure.
    fn decrypt(&self, obj: &EncryptedObject) -> Result<Vec<u8>, ProviderError>;

    /// Report current health, write-key id, and algorithm.
    fn status(&self) -> ProviderStatus;
}

/// In-process KMS provider over a [`Keyring`] of ML-KEM-768 KEKs.
#[derive(Debug)]
pub struct LocalKmsProvider {
    keyring: Keyring,
}

impl LocalKmsProvider {
    /// Wrap a keyring as an in-process provider.
    #[must_use]
    pub const fn new(keyring: Keyring) -> Self {
        Self { keyring }
    }

    /// Borrow the keyring (read path / introspection).
    #[must_use]
    pub const fn keyring(&self) -> &Keyring {
        &self.keyring
    }

    /// Mutably borrow the keyring — to drive the rotation lifecycle.
    pub const fn keyring_mut(&mut self) -> &mut Keyring {
        &mut self.keyring
    }

    /// Consume the provider, returning its keyring.
    #[must_use]
    pub fn into_keyring(self) -> Keyring {
        self.keyring
    }

    /// Seal `plaintext` under the current write key with explicit randomness —
    /// the deterministic seam behind [`KmsProvider::encrypt`].
    ///
    /// The write key's id is bound as the envelope AAD, so a blob can only be
    /// opened under the key id it was tagged with.
    ///
    /// # Errors
    /// [`ProviderError::Envelope`] if the envelope layer fails.
    pub fn encrypt_with(
        &self,
        plaintext: &[u8],
        r: &SealRandomness,
    ) -> Result<EncryptedObject, ProviderError> {
        let write = self.keyring.write_key();
        let key_id = write.id().clone();
        let blob = envelope::seal(plaintext, &write.kek().public(), key_id.as_str().as_bytes(), r)?;
        Ok(EncryptedObject { key_id, blob })
    }
}

impl KmsProvider for LocalKmsProvider {
    fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedObject, ProviderError> {
        self.encrypt_with(plaintext, &SealRandomness::generate())
    }

    fn decrypt(&self, obj: &EncryptedObject) -> Result<Vec<u8>, ProviderError> {
        let key = self.keyring.find(&obj.key_id).ok_or(ProviderError::UnknownKey)?;
        let plaintext = envelope::open(&obj.blob, key.kek(), obj.key_id.as_str().as_bytes())?;
        Ok(plaintext)
    }

    fn status(&self) -> ProviderStatus {
        ProviderStatus {
            version: "v2",
            healthy: true,
            current_key_id: self.keyring.write_key().id().clone(),
            algorithm: ALGORITHM,
        }
    }
}

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
