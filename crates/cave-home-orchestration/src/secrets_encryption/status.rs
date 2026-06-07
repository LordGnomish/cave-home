//! The encryption status view-model — the cavectl / Portal data contract.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! [`EncryptionStatus`] is the structured snapshot that `cavectl orchestration
//! secrets encryption status` renders as text and the Portal "Security >
//! Encryption" page binds to (key versions, algorithm, rotation phase). It is
//! pure data derived from a provider's keyring; it carries no clock, so key
//! *ages* are an observability concern (see [`super::metrics`]) computed from
//! caller-supplied timestamps.

use super::keyring::RotationPhase;
use super::provider::{ALGORITHM, LocalKmsProvider};

/// A structured snapshot of the encryption subsystem for status reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionStatus {
    /// Whether new writes of the governed resources are encrypted.
    pub enabled: bool,
    /// The envelope algorithm label.
    pub algorithm: &'static str,
    /// The id of the current write key.
    pub write_key_id: String,
    /// Total keys in the ring (write + retained read keys).
    pub key_count: usize,
    /// Retained read-only (stale) keys awaiting prune.
    pub read_only_key_count: usize,
    /// Where the keyring sits in the rotation lifecycle.
    pub rotation_phase: RotationPhase,
}

impl EncryptionStatus {
    /// Build a status snapshot from a provider's keyring. `enabled` reflects the
    /// configured write mode (whether the transformer encrypts new writes).
    #[must_use]
    pub fn from_provider(provider: &LocalKmsProvider, enabled: bool) -> Self {
        let ring = provider.keyring();
        Self {
            enabled,
            algorithm: ALGORITHM,
            write_key_id: ring.write_key().id().as_str().to_owned(),
            key_count: ring.len(),
            read_only_key_count: ring.read_only_ids().len(),
            rotation_phase: ring.phase(),
        }
    }

    /// A one-line human summary for the CLI.
    #[must_use]
    pub fn summary_line(&self) -> String {
        let state = if self.enabled { "on" } else { "off" };
        format!(
            "encryption {state}; algorithm {}; write key {}; {} key(s); phase {:?}",
            self.algorithm, self.write_key_id, self.key_count, self.rotation_phase
        )
    }
}

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
