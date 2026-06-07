//! Write-key / read-keys keyring and the K3s key-rotation lifecycle.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! The keyring is an ordered set of [`EncryptionKey`]s with index 0 the single
//! *write key* and every key a *read key*. K3s drives rotation through four
//! `secrets-encrypt` subcommands; this models them as an explicit
//! [`RotationPhase`] state machine so an illegal sequence is rejected rather
//! than silently corrupting the key set:
//!
//! ```text
//!   Steady ──prepare──▶ Prepared ──rotate──▶ Rotated ──prune──▶ Steady
//!      ▲                                                          │
//!      └──────────────────────────────────────────────────────────┘
//! ```
//!
//! * **prepare** appends a new key as read-only (decryptable, not yet writing);
//! * **rotate** promotes that key to the write slot and demotes the old one;
//! * the runtime then re-encrypts every secret under the new write key;
//! * **prune** drops the now-unused read keys, back to Steady.

use super::envelope::KekKeypair;

/// Maximum [`KeyId`] length — Kubernetes resource-name scale.
const MAX_KEY_ID_LEN: usize = 63;

/// What went wrong manipulating the keyring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringError {
    /// A key id was empty, too long, or held a disallowed character.
    InvalidKeyId,
    /// A key with this id is already in the ring.
    DuplicateKeyId,
    /// The operation is illegal in the current [`RotationPhase`].
    WrongPhase,
    /// `rotate` was called with no prepared key to promote.
    NothingToRotate,
}

impl core::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::InvalidKeyId => "keyring: invalid key id",
            Self::DuplicateKeyId => "keyring: duplicate key id",
            Self::WrongPhase => "keyring: operation illegal in current rotation phase",
            Self::NothingToRotate => "keyring: no prepared key to rotate to",
        };
        f.write_str(s)
    }
}

impl std::error::Error for KeyringError {}

/// A key identity — the `<key-id>` field in a transformer prefix.
///
/// Validated to be non-empty, at most [`MAX_KEY_ID_LEN`] bytes, ASCII
/// alphanumeric plus `-`/`_`/`.` — in particular never containing the `:`
/// transformer delimiter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(String);

impl KeyId {
    /// Validate and construct a key id.
    ///
    /// # Errors
    /// [`KeyringError::InvalidKeyId`] if `id` is empty, longer than
    /// [`MAX_KEY_ID_LEN`], or contains a character outside
    /// `[A-Za-z0-9._-]`.
    pub fn new(id: &str) -> Result<Self, KeyringError> {
        let ok = !id.is_empty()
            && id.len() <= MAX_KEY_ID_LEN
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if ok {
            Ok(Self(id.to_owned()))
        } else {
            Err(KeyringError::InvalidKeyId)
        }
    }

    /// The id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for KeyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a key may currently encrypt (write) or only decrypt (read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// The active key: used to encrypt new writes and to decrypt.
    WriteRead,
    /// A retained key: used only to decrypt values written under it.
    ReadOnly,
}

/// One key generation: its id, its ML-KEM-768 KEK, and its current state.
pub struct EncryptionKey {
    id: KeyId,
    kek: KekKeypair,
    state: KeyState,
}

impl EncryptionKey {
    /// Build a key in the [`KeyState::ReadOnly`] state; the keyring promotes it
    /// to write when appropriate.
    #[must_use]
    pub const fn new(id: KeyId, kek: KekKeypair) -> Self {
        Self { id, kek, state: KeyState::ReadOnly }
    }

    /// This key's id.
    #[must_use]
    pub const fn id(&self) -> &KeyId {
        &self.id
    }

    /// This key's KEK key pair.
    #[must_use]
    pub const fn kek(&self) -> &KekKeypair {
        &self.kek
    }

    /// This key's current state.
    #[must_use]
    pub const fn state(&self) -> KeyState {
        self.state
    }

    /// Whether this is the active write key.
    #[must_use]
    pub const fn is_write(&self) -> bool {
        matches!(self.state, KeyState::WriteRead)
    }
}

impl core::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("id", &self.id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// Where the keyring sits in the rotation lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationPhase {
    /// One write key, no pending rotation.
    Steady,
    /// A new key has been prepared (read-only) and awaits `rotate`.
    Prepared,
    /// The write key has changed; stale read keys await re-encrypt + `prune`.
    Rotated,
}

/// An ordered keyring: `keys[0]` is the write key; all entries are read keys.
#[derive(Debug)]
pub struct Keyring {
    keys: Vec<EncryptionKey>,
    phase: RotationPhase,
}

impl Keyring {
    /// Create a keyring with a single write key.
    #[must_use]
    pub fn new(write_id: KeyId, write_kek: KekKeypair) -> Self {
        let key = EncryptionKey { id: write_id, kek: write_kek, state: KeyState::WriteRead };
        Self { keys: vec![key], phase: RotationPhase::Steady }
    }

    /// The current rotation phase.
    #[must_use]
    pub const fn phase(&self) -> RotationPhase {
        self.phase
    }

    /// The active write key (`keys[0]`).
    #[must_use]
    pub fn write_key(&self) -> &EncryptionKey {
        // Invariant: the ring is never empty and index 0 is the write key.
        &self.keys[0]
    }

    /// All keys, in order (write key first). Suitable for the read path.
    #[must_use]
    pub fn keys(&self) -> &[EncryptionKey] {
        &self.keys
    }

    /// Number of keys in the ring.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Always `false` — a keyring always has at least the write key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Find a key by id (write or read), for routing a decrypt.
    #[must_use]
    pub fn find(&self, id: &KeyId) -> Option<&EncryptionKey> {
        self.keys.iter().find(|k| k.id() == id)
    }

    /// The ids of every read-only (stale) key, in order.
    #[must_use]
    pub fn read_only_ids(&self) -> Vec<&KeyId> {
        self.keys
            .iter()
            .filter(|k| !k.is_write())
            .map(EncryptionKey::id)
            .collect()
    }

    /// **prepare** — append a new key as read-only, awaiting `rotate`.
    ///
    /// # Errors
    /// - [`KeyringError::WrongPhase`] unless the ring is [`RotationPhase::Steady`];
    /// - [`KeyringError::DuplicateKeyId`] if `id` is already present.
    pub fn prepare(&mut self, id: KeyId, kek: KekKeypair) -> Result<(), KeyringError> {
        if self.phase != RotationPhase::Steady {
            return Err(KeyringError::WrongPhase);
        }
        if self.find(&id).is_some() {
            return Err(KeyringError::DuplicateKeyId);
        }
        self.keys.push(EncryptionKey::new(id, kek));
        self.phase = RotationPhase::Prepared;
        Ok(())
    }

    /// **rotate** — promote the prepared key to the write slot, demoting the
    /// previous write key to read-only. Returns the new write key's id.
    ///
    /// # Errors
    /// [`KeyringError::NothingToRotate`] unless the ring is
    /// [`RotationPhase::Prepared`].
    pub fn rotate(&mut self) -> Result<&KeyId, KeyringError> {
        if self.phase != RotationPhase::Prepared {
            return Err(KeyringError::NothingToRotate);
        }
        // The prepared key is the last one appended; move it to the front.
        let mut prepared = self.keys.remove(self.keys.len() - 1);
        prepared.state = KeyState::WriteRead;
        for key in &mut self.keys {
            key.state = KeyState::ReadOnly;
        }
        self.keys.insert(0, prepared);
        self.phase = RotationPhase::Rotated;
        Ok(self.keys[0].id())
    }

    /// **prune** — drop every read-only key, leaving only the write key. Returns
    /// the pruned ids. Run after the cluster has re-encrypted all secrets.
    ///
    /// # Errors
    /// [`KeyringError::WrongPhase`] unless the ring is [`RotationPhase::Rotated`].
    pub fn prune(&mut self) -> Result<Vec<KeyId>, KeyringError> {
        if self.phase != RotationPhase::Rotated {
            return Err(KeyringError::WrongPhase);
        }
        let mut pruned = Vec::new();
        let mut kept = Vec::with_capacity(1);
        for key in self.keys.drain(..) {
            if key.is_write() {
                kept.push(key);
            } else {
                pruned.push(key.id);
            }
        }
        self.keys = kept;
        self.phase = RotationPhase::Steady;
        Ok(pruned)
    }

    /// **rotate-keys** — the one-shot `prepare` + `rotate`. Leaves the ring in
    /// [`RotationPhase::Rotated`]; the runtime then re-encrypts and `prune`s.
    ///
    /// # Errors
    /// As [`Keyring::prepare`] (the ring must be [`RotationPhase::Steady`] and
    /// `id` unused).
    pub fn rotate_keys(&mut self, id: KeyId, kek: KekKeypair) -> Result<(), KeyringError> {
        self.prepare(id, kek)?;
        self.rotate()?;
        Ok(())
    }
}

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
