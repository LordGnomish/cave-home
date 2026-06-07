//! Write-key / read-keys keyring and the K3s key-rotation lifecycle.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::secrets_encryption::envelope::KekKeypair;

    fn key(id: &str, seed_tag: u8) -> (KeyId, KekKeypair) {
        (KeyId::new(id).unwrap(), KekKeypair::from_seed([seed_tag; 64]))
    }

    fn ring() -> Keyring {
        let (id, kek) = key("key-1", 1);
        Keyring::new(id, kek)
    }

    // ── KeyId validation ────────────────────────────────────────────────────

    #[test]
    fn key_id_accepts_well_formed() {
        assert!(KeyId::new("aescbc-2026-01").is_ok());
        assert!(KeyId::new("k_1").is_ok());
    }

    #[test]
    fn key_id_rejects_empty() {
        assert!(matches!(KeyId::new(""), Err(KeyringError::InvalidKeyId)));
    }

    #[test]
    fn key_id_rejects_delimiter() {
        // ':' is the transformer field delimiter and must not appear in an id.
        assert!(matches!(KeyId::new("a:b"), Err(KeyringError::InvalidKeyId)));
    }

    #[test]
    fn key_id_rejects_overlong() {
        let long = "a".repeat(64);
        assert!(matches!(KeyId::new(&long), Err(KeyringError::InvalidKeyId)));
    }

    // ── construction ────────────────────────────────────────────────────────

    #[test]
    fn new_keyring_is_steady_with_one_write_key() {
        let r = ring();
        assert_eq!(r.phase(), RotationPhase::Steady);
        assert_eq!(r.len(), 1);
        assert_eq!(r.write_key().id().as_str(), "key-1");
        assert!(r.write_key().is_write());
        assert_eq!(r.write_key().state(), KeyState::WriteRead);
    }

    // ── prepare ─────────────────────────────────────────────────────────────

    #[test]
    fn prepare_appends_read_only_and_keeps_write_key() {
        let mut r = ring();
        let (id, kek) = key("key-2", 2);
        r.prepare(id, kek).unwrap();
        assert_eq!(r.phase(), RotationPhase::Prepared);
        assert_eq!(r.len(), 2);
        // Write key unchanged...
        assert_eq!(r.write_key().id().as_str(), "key-1");
        // ...and the prepared key is read-only.
        let k2 = r.find(&KeyId::new("key-2").unwrap()).unwrap();
        assert_eq!(k2.state(), KeyState::ReadOnly);
        assert!(!k2.is_write());
    }

    #[test]
    fn prepare_rejects_duplicate_id() {
        let mut r = ring();
        let (id, kek) = key("key-1", 9);
        assert!(matches!(r.prepare(id, kek), Err(KeyringError::DuplicateKeyId)));
    }

    #[test]
    fn prepare_rejects_when_not_steady() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        // Second prepare without rotate/prune is illegal.
        let err = r.prepare(KeyId::new("key-3").unwrap(), KekKeypair::from_seed([3; 64]));
        assert!(matches!(err, Err(KeyringError::WrongPhase)));
    }

    // ── rotate ──────────────────────────────────────────────────────────────

    #[test]
    fn rotate_promotes_prepared_key_to_write() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        let new_write = r.rotate().unwrap();
        assert_eq!(new_write.as_str(), "key-2");
        assert_eq!(r.phase(), RotationPhase::Rotated);
        assert_eq!(r.write_key().id().as_str(), "key-2");
        assert_eq!(r.write_key().state(), KeyState::WriteRead);
        // Old write key is now read-only.
        let k1 = r.find(&KeyId::new("key-1").unwrap()).unwrap();
        assert_eq!(k1.state(), KeyState::ReadOnly);
    }

    #[test]
    fn rotate_rejects_when_not_prepared() {
        let mut r = ring();
        assert!(matches!(r.rotate(), Err(KeyringError::NothingToRotate)));
    }

    #[test]
    fn exactly_one_write_key_after_rotate() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        r.rotate().unwrap();
        let writes = r.keys().iter().filter(|k| k.is_write()).count();
        assert_eq!(writes, 1);
        assert!(r.keys()[0].is_write(), "write key is always at index 0");
    }

    // ── prune ───────────────────────────────────────────────────────────────

    #[test]
    fn prune_drops_read_keys_and_returns_to_steady() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        r.rotate().unwrap();
        let pruned = r.prune().unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].as_str(), "key-1");
        assert_eq!(r.phase(), RotationPhase::Steady);
        assert_eq!(r.len(), 1);
        assert_eq!(r.write_key().id().as_str(), "key-2");
        assert!(r.find(&KeyId::new("key-1").unwrap()).is_none());
    }

    #[test]
    fn prune_rejects_when_not_rotated() {
        let mut r = ring();
        assert!(matches!(r.prune(), Err(KeyringError::WrongPhase)));
    }

    // ── routing / read keys ─────────────────────────────────────────────────

    #[test]
    fn find_routes_by_id() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        assert!(r.find(&KeyId::new("key-1").unwrap()).is_some());
        assert!(r.find(&KeyId::new("key-2").unwrap()).is_some());
        assert!(r.find(&KeyId::new("missing").unwrap()).is_none());
    }

    #[test]
    fn read_only_ids_lists_stale_keys() {
        let mut r = ring();
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        r.rotate().unwrap();
        let ro: Vec<_> = r.read_only_ids().iter().map(|i| i.as_str().to_owned()).collect();
        assert_eq!(ro, vec!["key-1".to_owned()]);
    }

    // ── one-shot rotate-keys + full lifecycle ───────────────────────────────

    #[test]
    fn rotate_keys_prepares_and_rotates_in_one_step() {
        let mut r = ring();
        r.rotate_keys(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        assert_eq!(r.phase(), RotationPhase::Rotated);
        assert_eq!(r.write_key().id().as_str(), "key-2");
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn full_lifecycle_prepare_rotate_prune() {
        let mut r = ring();
        assert_eq!(r.phase(), RotationPhase::Steady);
        r.prepare(KeyId::new("key-2").unwrap(), KekKeypair::from_seed([2; 64]))
            .unwrap();
        r.rotate().unwrap();
        r.prune().unwrap();
        assert_eq!(r.phase(), RotationPhase::Steady);
        assert_eq!(r.len(), 1);
        assert_eq!(r.write_key().id().as_str(), "key-2");
        // The invariant holds at the end: exactly one write key, at index 0.
        assert!(r.keys()[0].is_write());
        assert_eq!(r.keys().iter().filter(|k| k.is_write()).count(), 1);
    }
}
