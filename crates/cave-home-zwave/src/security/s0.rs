// SPDX-License-Identifier: Apache-2.0
//! Security S0 (Legacy) framework.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/security/Manager.ts
//!
//! Re-implementation of upstream's `SecurityManager`. The upstream class owns:
//!
//! 1. The 16-byte network key.
//! 2. The cached auth key (AES-128-ECB of `0x55` × 16 under the network key)
//!    and encryption key (AES-128-ECB of `0xaa` × 16).
//! 3. A short-lived store of generated nonces, keyed by `(issuer, nonceId)`.
//!
//! Phase 1 ports (1) and (2) byte-accurately, plus the nonce *store* surface
//! the inclusion flow needs. Nonce *expiry* (a `setTimer` in upstream) is
//! tracked as an optional last-used timestamp; the driver is expected to call
//! [`S0Manager::expire_older_than`] from its tick loop.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::crypto::aes128_ecb_encrypt;
use crate::error::{ZwaveError, ZwaveResult};

const AUTH_KEY_BASE: [u8; 16] = [0x55; 16];
const ENCRYPTION_KEY_BASE: [u8; 16] = [0xaa; 16];

/// Derive the S0 authentication key from a 16-byte network key.
///
/// # Upstream: `Manager.ts::generateAuthKey`
///
/// # Errors
/// Same as [`aes128_ecb_encrypt`].
pub fn generate_auth_key(network_key: &[u8]) -> ZwaveResult<[u8; 16]> {
    aes128_ecb_encrypt(network_key, &AUTH_KEY_BASE)
}

/// Derive the S0 encryption key from a 16-byte network key.
///
/// # Upstream: `Manager.ts::generateEncryptionKey`
pub fn generate_encryption_key(network_key: &[u8]) -> ZwaveResult<[u8; 16]> {
    aes128_ecb_encrypt(network_key, &ENCRYPTION_KEY_BASE)
}

/// Key under which a nonce is stored.
///
/// # Upstream: `Manager.ts::NonceKey`
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct NonceKey {
    /// Node ID that produced the nonce.
    pub issuer: u8,
    /// Single-byte nonce identifier (`nonce[0]`).
    pub nonce_id: u8,
}

#[derive(Clone, Debug)]
struct NonceEntry {
    nonce: [u8; 8],
    receiver: u8,
    /// When this nonce was stored — used by [`S0Manager::expire_older_than`].
    stored_at: Instant,
    /// Whether the nonce is still in the "free" set (i.e. has not yet been
    /// returned by `getFreeNonce`).
    free: bool,
}

/// S0 security manager (network key + nonce store).
///
/// # Upstream: `Manager.ts::SecurityManager`
#[derive(Debug)]
pub struct S0Manager {
    network_key: [u8; 16],
    auth_key: [u8; 16],
    encryption_key: [u8; 16],
    own_node_id: u8,
    nonces: HashMap<NonceKey, NonceEntry>,
}

impl S0Manager {
    /// Build a manager around the given network key.
    ///
    /// # Errors
    /// Returns [`ZwaveError::Security`] if `network_key` is not 16 bytes.
    pub fn new(network_key: [u8; 16], own_node_id: u8) -> ZwaveResult<Self> {
        let auth_key = generate_auth_key(&network_key)?;
        let encryption_key = generate_encryption_key(&network_key)?;
        Ok(Self {
            network_key,
            auth_key,
            encryption_key,
            own_node_id,
            nonces: HashMap::new(),
        })
    }

    /// Reveal the auth key (mainly for testing — drivers should never need
    /// to inspect it directly).
    #[must_use]
    pub fn auth_key(&self) -> &[u8; 16] {
        &self.auth_key
    }

    /// Reveal the encryption key.
    #[must_use]
    pub fn encryption_key(&self) -> &[u8; 16] {
        &self.encryption_key
    }

    /// Reveal the network key (so the driver can hand it to the controller
    /// during `NetworkKeySet`).
    #[must_use]
    pub fn network_key(&self) -> &[u8; 16] {
        &self.network_key
    }

    /// Store a nonce produced by us.
    pub fn set_nonce(&mut self, nonce: [u8; 8], receiver: u8) {
        let key = NonceKey {
            issuer: self.own_node_id,
            nonce_id: nonce[0],
        };
        self.nonces.insert(
            key,
            NonceEntry {
                nonce,
                receiver,
                stored_at: Instant::now(),
                free: true,
            },
        );
    }

    /// Store a nonce produced by a peer node.
    pub fn set_peer_nonce(&mut self, issuer: u8, nonce: [u8; 8], receiver: u8) {
        let key = NonceKey {
            issuer,
            nonce_id: nonce[0],
        };
        self.nonces.insert(
            key,
            NonceEntry {
                nonce,
                receiver,
                stored_at: Instant::now(),
                free: false,
            },
        );
    }

    /// Look a nonce up. Mirrors upstream's `getNonce`.
    #[must_use]
    pub fn get_nonce(&self, key: NonceKey) -> Option<[u8; 8]> {
        self.nonces.get(&key).map(|e| e.nonce)
    }

    /// Whether a nonce exists. Mirrors upstream's `hasNonce`.
    #[must_use]
    pub fn has_nonce(&self, key: NonceKey) -> bool {
        self.nonces.contains_key(&key)
    }

    /// Consume a free nonce issued by `node_id`. Mirrors upstream's
    /// `getFreeNonce`.
    pub fn take_free_nonce(&mut self, node_id: u8) -> Option<[u8; 8]> {
        let key = self
            .nonces
            .iter()
            .find(|(k, e)| k.issuer == node_id && e.free)
            .map(|(k, _)| *k)?;
        let entry = self.nonces.get_mut(&key)?;
        entry.free = false;
        Some(entry.nonce)
    }

    /// Drop nonces older than `max_age`. Drivers should call this from a
    /// timer.
    ///
    /// # Upstream: `Manager.ts::expireNonce` (called from the `setTimer`).
    pub fn expire_older_than(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.nonces.retain(|_, e| now.duration_since(e.stored_at) < max_age);
    }

    /// Forget every nonce ever issued *for* `receiver`. Mirrors upstream's
    /// `deleteAllNoncesForReceiver`.
    pub fn delete_all_nonces_for_receiver(&mut self, receiver: u8) {
        self.nonces.retain(|_, e| e.receiver != receiver);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The auth key for an all-zero network key is the AES-128-ECB of
    /// 0x55…55 under the zero key — a deterministic value the upstream JS
    /// test suite also pins.
    #[test]
    fn auth_key_is_deterministic_function_of_network_key() {
        let nk = [0u8; 16];
        let auth = generate_auth_key(&nk).unwrap();
        let auth2 = generate_auth_key(&nk).unwrap();
        assert_eq!(auth, auth2);
        // Encrypting the auth-key base under all-zeros gives a non-trivial
        // tag; just confirm it isn't the input back.
        assert_ne!(auth, [0x55; 16]);
    }

    #[test]
    fn encryption_key_differs_from_auth_key() {
        let nk = [0x42; 16];
        let auth = generate_auth_key(&nk).unwrap();
        let enc = generate_encryption_key(&nk).unwrap();
        assert_ne!(auth, enc);
    }

    #[test]
    fn nonce_store_round_trips() {
        let mut mgr = S0Manager::new([0u8; 16], 1).unwrap();
        let nonce = [0xab, 0xcd, 0xef, 0x01, 0x02, 0x03, 0x04, 0x05];
        mgr.set_nonce(nonce, 12);
        let key = NonceKey {
            issuer: 1,
            nonce_id: 0xab,
        };
        assert!(mgr.has_nonce(key));
        assert_eq!(mgr.get_nonce(key), Some(nonce));
    }

    #[test]
    fn take_free_nonce_returns_first_free_for_issuer() {
        let mut mgr = S0Manager::new([0u8; 16], 1).unwrap();
        mgr.set_peer_nonce(
            7,
            [0x11, 0, 0, 0, 0, 0, 0, 0],
            1,
        );
        // Peer-set nonces are not free — drivers reserve them for the
        // specific incoming message.
        assert_eq!(mgr.take_free_nonce(7), None);

        // Our own nonces *are* free.
        mgr.set_nonce([0x22, 0, 0, 0, 0, 0, 0, 0], 9);
        assert_eq!(
            mgr.take_free_nonce(1).map(|n| n[0]),
            Some(0x22)
        );
        // And only-once.
        assert_eq!(mgr.take_free_nonce(1), None);
    }

    #[test]
    fn delete_all_nonces_for_receiver_drops_matching() {
        let mut mgr = S0Manager::new([0u8; 16], 1).unwrap();
        mgr.set_nonce([0x10, 0, 0, 0, 0, 0, 0, 0], 5);
        mgr.set_nonce([0x20, 0, 0, 0, 0, 0, 0, 0], 6);
        mgr.delete_all_nonces_for_receiver(5);
        assert!(!mgr.has_nonce(NonceKey {
            issuer: 1,
            nonce_id: 0x10
        }));
        assert!(mgr.has_nonce(NonceKey {
            issuer: 1,
            nonce_id: 0x20
        }));
    }
}
