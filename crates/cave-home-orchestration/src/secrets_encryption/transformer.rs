//! The stored-value transformer: the prefix scheme + identity fallback.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! Kubernetes tags every stored value with a provider prefix
//! (`k8s:enc:<provider>:<version>:…`) so the read path can route it back to the
//! provider that wrote it. cave-home uses the same idea with its own scheme:
//!
//! * encrypted: `cave:enc:mlkem768:v1:<key-id>:` ‖ envelope-blob
//! * identity (encryption off): `cave:enc:identity:` ‖ plaintext
//!
//! Because a [`KeyId`](crate::secrets_encryption::keyring::KeyId) can never
//! contain `:`, the read path can split the fixed fields off the front and take
//! the remainder as the raw envelope blob. Both prefixes are always readable, so
//! a cluster can be migrated into or out of encryption without rewriting first —
//! the identity provider is the read fallback.

use super::provider::{EncryptedObject, KmsProvider, ProviderError};

/// Prefix on an encrypted stored value, up to and including the trailing `:`
/// that precedes the `<key-id>`.
pub const ENC_PREFIX: &str = "cave:enc:mlkem768:v1:";
/// Prefix on an identity (passthrough) stored value.
pub const IDENTITY_PREFIX: &str = "cave:enc:identity:";

/// How the transformer encrypts *new* writes. Reads always handle both prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Seal new writes under the provider's current write key.
    Encrypt,
    /// Write plaintext behind the identity prefix (encryption disabled).
    Identity,
}

/// Why a transform failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformError {
    /// The stored value carried no recognised cave-home prefix.
    UnknownPrefix,
    /// The value had the encrypt prefix but was structurally invalid
    /// (no key-id delimiter, or an invalid key id).
    Malformed,
    /// The provider failed to decrypt the routed value.
    Provider(ProviderError),
}

impl core::fmt::Display for TransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownPrefix => f.write_str("transformer: unrecognised storage prefix"),
            Self::Malformed => f.write_str("transformer: malformed encrypted value"),
            Self::Provider(e) => write!(f, "transformer: {e}"),
        }
    }
}

impl std::error::Error for TransformError {}

impl From<ProviderError> for TransformError {
    fn from(e: ProviderError) -> Self {
        Self::Provider(e)
    }
}

/// Wraps a [`KmsProvider`] with the prefix scheme + identity fallback, turning a
/// plaintext value into a self-describing stored value and back.
#[derive(Debug)]
pub struct Transformer<P: KmsProvider> {
    provider: P,
    write_mode: WriteMode,
}

impl<P: KmsProvider> Transformer<P> {
    /// Build a transformer over `provider`, writing per `write_mode`.
    #[must_use]
    pub const fn new(provider: P, write_mode: WriteMode) -> Self {
        Self { provider, write_mode }
    }

    /// The current write mode.
    #[must_use]
    pub const fn write_mode(&self) -> WriteMode {
        self.write_mode
    }

    /// Borrow the underlying provider.
    #[must_use]
    pub const fn provider(&self) -> &P {
        &self.provider
    }

    /// Mutably borrow the provider — e.g. to drive its keyring rotation.
    pub const fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }

    /// Transform a plaintext value into its stored (prefixed) form.
    ///
    /// # Errors
    /// [`TransformError::Provider`] if encryption is enabled and the provider
    /// fails to seal the value.
    pub fn to_storage(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransformError> {
        match self.write_mode {
            WriteMode::Encrypt => {
                let obj = self.provider.encrypt(plaintext)?;
                let mut out =
                    Vec::with_capacity(ENC_PREFIX.len() + obj.key_id.as_str().len() + 1 + obj.blob.len());
                out.extend_from_slice(ENC_PREFIX.as_bytes());
                out.extend_from_slice(obj.key_id.as_str().as_bytes());
                out.push(b':');
                out.extend_from_slice(&obj.blob);
                Ok(out)
            }
            WriteMode::Identity => {
                let mut out = Vec::with_capacity(IDENTITY_PREFIX.len() + plaintext.len());
                out.extend_from_slice(IDENTITY_PREFIX.as_bytes());
                out.extend_from_slice(plaintext);
                Ok(out)
            }
        }
    }

    /// Transform a stored value back into plaintext, routing by its prefix.
    ///
    /// # Errors
    /// - [`TransformError::UnknownPrefix`] for an unrecognised prefix;
    /// - [`TransformError::Malformed`] for a corrupt encrypt prefix;
    /// - [`TransformError::Provider`] for an authentication/decrypt failure.
    pub fn from_storage(&self, stored: &[u8]) -> Result<Vec<u8>, TransformError> {
        if let Some(rest) = stored.strip_prefix(ENC_PREFIX.as_bytes()) {
            let delim = rest
                .iter()
                .position(|&b| b == b':')
                .ok_or(TransformError::Malformed)?;
            let (key_id_bytes, blob) = (&rest[..delim], &rest[delim + 1..]);
            let key_id_str =
                core::str::from_utf8(key_id_bytes).map_err(|_| TransformError::Malformed)?;
            let key_id = super::keyring::KeyId::new(key_id_str)
                .map_err(|_| TransformError::Malformed)?;
            let obj = EncryptedObject { key_id, blob: blob.to_vec() };
            Ok(self.provider.decrypt(&obj)?)
        } else if let Some(plaintext) = stored.strip_prefix(IDENTITY_PREFIX.as_bytes()) {
            Ok(plaintext.to_vec())
        } else {
            Err(TransformError::UnknownPrefix)
        }
    }
}

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
