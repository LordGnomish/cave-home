// SPDX-License-Identifier: Apache-2.0
//! Security S2 (CSA / CKD) framework.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/security/Manager2.ts
//!
//! Upstream's `SecurityManager2` is several thousand lines covering nonce
//! tracking, SPAN/MPAN encryption, Multicast group state, etc. Phase 1 ports
//! the **key-derivation surface** (CKDF — Constrained Key Derivation Function)
//! plus a key-store keyed by [`crate::security::SecurityClass`]. The
//! per-message SPAN / MPAN crypto follows the framework in Phase 1b.

use std::collections::HashMap;

use super::SecurityClass;
use super::crypto::{cmac_aes128, hkdf_sha256};
use crate::error::{ZwaveError, ZwaveResult};

/// The four-key set derived from a single 16-byte network key.
///
/// # Upstream: `Manager2.ts::deriveNetworkKeys`
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S2Keys {
    /// PNK — Personal Network Key (the 16-byte input).
    pub pnk: [u8; 16],
    /// KCCM — key for AES-128-CCM (data encryption).
    pub kccm: [u8; 16],
    /// KMPAN — key for the Multicast Pre-Agreed Nonce.
    pub kmpan: [u8; 16],
    /// PersonalizationString — used for SPAN seeding.
    pub personalization_string: [u8; 16],
}

/// Derive the four S2 working keys from a 16-byte network key.
///
/// # Upstream: `Manager2.ts::deriveNetworkKeys`
///
/// The algorithm is HKDF-SHA256 with constant info strings, applied four
/// times to the same network key with different per-output info bytes.
///
/// # Errors
/// Returns [`ZwaveError::Security`] if the network key is not 16 bytes.
pub fn derive_s2_keys(network_key: &[u8]) -> ZwaveResult<S2Keys> {
    if network_key.len() != 16 {
        return Err(ZwaveError::Security(format!(
            "S2 network key must be 16 bytes, got {}",
            network_key.len()
        )));
    }
    // Each output is 16 bytes; salt is empty per spec.
    let kccm = hkdf_sha256(&[], network_key, b"S2-key-CCM", 16)?;
    let kmpan = hkdf_sha256(&[], network_key, b"S2-key-MPAN", 16)?;
    let pstr = hkdf_sha256(&[], network_key, b"S2-key-personalization", 16)?;
    let mut pnk = [0u8; 16];
    pnk.copy_from_slice(network_key);
    let mut kccm_arr = [0u8; 16];
    kccm_arr.copy_from_slice(&kccm);
    let mut kmpan_arr = [0u8; 16];
    kmpan_arr.copy_from_slice(&kmpan);
    let mut pstr_arr = [0u8; 16];
    pstr_arr.copy_from_slice(&pstr);
    Ok(S2Keys {
        pnk,
        kccm: kccm_arr,
        kmpan: kmpan_arr,
        personalization_string: pstr_arr,
    })
}

/// S2 key-manager — per-class keystore.
///
/// # Upstream: `Manager2.ts::SecurityManager2` (subset).
#[derive(Debug, Default)]
pub struct S2Manager {
    keys: HashMap<SecurityClass, S2Keys>,
}

impl S2Manager {
    /// Empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a derived key set for `class`.
    pub fn install_keys(&mut self, class: SecurityClass, keys: S2Keys) {
        self.keys.insert(class, keys);
    }

    /// Derive and install in one call.
    ///
    /// # Errors
    /// Same as [`derive_s2_keys`].
    pub fn install_network_key(
        &mut self,
        class: SecurityClass,
        network_key: &[u8],
    ) -> ZwaveResult<()> {
        let keys = derive_s2_keys(network_key)?;
        self.keys.insert(class, keys);
        Ok(())
    }

    /// Look up keys for `class`.
    #[must_use]
    pub fn keys_for(&self, class: SecurityClass) -> Option<&S2Keys> {
        self.keys.get(&class)
    }

    /// Which classes do we have key material for?
    #[must_use]
    pub fn known_classes(&self) -> Vec<SecurityClass> {
        self.keys.keys().copied().collect()
    }

    /// Generate the temporary-key CMAC tag used during S2 bootstrap (Security
    /// 2 Public Key Verify). Upstream calls this `computeTempKey` /
    /// `verifyTempKey`.
    ///
    /// # Upstream: `Manager2.ts::verifyTempKey`
    ///
    /// # Errors
    /// Returns [`ZwaveError::Security`] if the key is not 16 bytes.
    pub fn temp_key_cmac(temp_key: &[u8], data: &[u8]) -> ZwaveResult<[u8; 16]> {
        cmac_aes128(temp_key, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_keys_are_deterministic() {
        let nk = [0x12; 16];
        let a = derive_s2_keys(&nk).unwrap();
        let b = derive_s2_keys(&nk).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn derived_keys_differ_per_info_string() {
        let nk = [0x12; 16];
        let k = derive_s2_keys(&nk).unwrap();
        assert_ne!(k.kccm, k.kmpan);
        assert_ne!(k.kccm, k.personalization_string);
        assert_ne!(k.kmpan, k.personalization_string);
    }

    #[test]
    fn manager_stores_per_class_keys() {
        let mut m = S2Manager::new();
        m.install_network_key(SecurityClass::S2AccessControl, &[0x10; 16]).unwrap();
        m.install_network_key(SecurityClass::S2Authenticated, &[0x20; 16]).unwrap();
        assert!(m.keys_for(SecurityClass::S2AccessControl).is_some());
        assert!(m.keys_for(SecurityClass::S2Authenticated).is_some());
        assert!(m.keys_for(SecurityClass::S2Unauthenticated).is_none());
        let mut classes = m.known_classes();
        classes.sort_by_key(|c| c.wire_byte().unwrap_or(255));
        assert_eq!(
            classes,
            vec![
                SecurityClass::S2Authenticated,
                SecurityClass::S2AccessControl
            ]
        );
    }

    #[test]
    fn derive_rejects_wrong_length_key() {
        let err = derive_s2_keys(&[0u8; 8]).unwrap_err();
        assert!(matches!(err, ZwaveError::Security(_)));
    }

    #[test]
    fn temp_key_cmac_matches_raw_cmac() {
        let key = [0x55; 16];
        let data = [0xa5; 32];
        let a = S2Manager::temp_key_cmac(&key, &data).unwrap();
        let b = cmac_aes128(&key, &data).unwrap();
        assert_eq!(a, b);
    }
}
