// SPDX-License-Identifier: Apache-2.0
//! Low-level cryptographic primitives used by S0 + S2.
//!
//! # Upstream: zwave-js/zwave-js@5ffca2b38393f9eab0bffcdbd65b3020cbeda492:packages/core/src/crypto/index.ts
//!
//! Pure-Rust re-implementation using `aes`, `aes-gcm`, `cmac`, `hkdf`, `sha2`.
//! All inputs/outputs are byte slices / fixed-size arrays — no Node.js
//! `Buffer` adapter shenanigans.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use cmac::{Cmac, Mac};

use crate::error::{ZwaveError, ZwaveResult};

/// AES-128 ECB encrypt one 16-byte block.
///
/// # Upstream: `crypto/index.ts::encryptAES128ECB`
///
/// # Errors
/// Returns [`ZwaveError::Security`] if `key` or `data` are not 16 bytes long.
pub fn aes128_ecb_encrypt(key: &[u8], data: &[u8]) -> ZwaveResult<[u8; 16]> {
    if key.len() != 16 {
        return Err(ZwaveError::Security(format!(
            "AES-128-ECB key must be 16 bytes, got {}",
            key.len()
        )));
    }
    if data.len() != 16 {
        return Err(ZwaveError::Security(format!(
            "AES-128-ECB block must be 16 bytes, got {}",
            data.len()
        )));
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut block = GenericArray::clone_from_slice(data);
    cipher.encrypt_block(&mut block);
    let mut out = [0u8; 16];
    out.copy_from_slice(&block);
    Ok(out)
}

/// CMAC-AES-128 over `data`, keyed with `key`.
///
/// # Upstream: `crypto/index.ts::computeMAC` (S2 path) and S0 MAC.
///
/// # Errors
/// Returns [`ZwaveError::Security`] if the key is not 16 bytes long.
pub fn cmac_aes128(key: &[u8], data: &[u8]) -> ZwaveResult<[u8; 16]> {
    if key.len() != 16 {
        return Err(ZwaveError::Security(format!(
            "CMAC-AES-128 key must be 16 bytes, got {}",
            key.len()
        )));
    }
    let mut mac = <Cmac<Aes128> as Mac>::new_from_slice(key)
        .map_err(|e| ZwaveError::Security(format!("CMAC init failed: {e}")))?;
    mac.update(data);
    let tag = mac.finalize().into_bytes();
    let mut out = [0u8; 16];
    out.copy_from_slice(&tag);
    Ok(out)
}

/// First 8 bytes of CMAC-AES-128 — the S0 MAC truncation used by upstream's
/// `Security CC Message Encapsulation`.
///
/// # Upstream: `Manager.ts::computeMAC` (S0 path).
///
/// # Errors
/// Same as [`cmac_aes128`].
pub fn s0_truncated_mac(key: &[u8], data: &[u8]) -> ZwaveResult<[u8; 8]> {
    let full = cmac_aes128(key, data)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(&full[..8]);
    Ok(out)
}

/// HKDF-SHA256 expand only (S2's CKDF uses a constant info string per key
/// type — see [`crate::security::s2::derive_s2_keys`]).
///
/// # Upstream: HKDF usage in `Manager2.ts`.
///
/// # Errors
/// Returns [`ZwaveError::Security`] if `length > 255 * 32` (HKDF-SHA256 max).
pub fn hkdf_sha256_expand(prk: &[u8], info: &[u8], length: usize) -> ZwaveResult<Vec<u8>> {
    let hk = hkdf::Hkdf::<sha2::Sha256>::from_prk(prk)
        .map_err(|e| ZwaveError::Security(format!("HKDF prk invalid: {e}")))?;
    let mut out = vec![0u8; length];
    hk.expand(info, &mut out)
        .map_err(|e| ZwaveError::Security(format!("HKDF expand failed: {e}")))?;
    Ok(out)
}

/// HKDF-SHA256 extract+expand.
///
/// # Errors
/// Returns [`ZwaveError::Security`] if `length > 255 * 32`.
pub fn hkdf_sha256(salt: &[u8], ikm: &[u8], info: &[u8], length: usize) -> ZwaveResult<Vec<u8>> {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), ikm);
    let mut out = vec![0u8; length];
    hk.expand(info, &mut out)
        .map_err(|e| ZwaveError::Security(format!("HKDF expand failed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer test from NIST SP 800-38A — AES-128 ECB with the FIPS
    /// example key encrypting the FIPS example plaintext block.
    #[test]
    fn aes128_ecb_known_answer() {
        let key = hex16("2b7e151628aed2a6abf7158809cf4f3c");
        let pt = hex16("6bc1bee22e409f96e93d7e117393172a");
        let expected = hex16("3ad77bb40d7a3660a89ecaf32466ef97");
        let ct = aes128_ecb_encrypt(&key, &pt).unwrap();
        assert_eq!(ct, expected);
    }

    /// Known-answer test from NIST SP 800-38B example 1 — CMAC-AES-128 of
    /// the empty string with the SP 800-38A key.
    #[test]
    fn cmac_aes128_known_answer_empty() {
        let key = hex16("2b7e151628aed2a6abf7158809cf4f3c");
        let tag = cmac_aes128(&key, &[]).unwrap();
        let expected = hex16("bb1d6929e95937287fa37d129b756746");
        assert_eq!(tag, expected);
    }

    #[test]
    fn s0_mac_truncates_cmac_to_8_bytes() {
        let key = hex16("2b7e151628aed2a6abf7158809cf4f3c");
        let mac = s0_truncated_mac(&key, &[]).unwrap();
        // First 8 bytes of the CMAC known-answer above.
        let expected = [0xbb, 0x1d, 0x69, 0x29, 0xe9, 0x59, 0x37, 0x28];
        assert_eq!(mac, expected);
    }

    #[test]
    fn aes128_ecb_rejects_wrong_key_length() {
        let err = aes128_ecb_encrypt(&[0u8; 8], &[0u8; 16]).unwrap_err();
        assert!(matches!(err, ZwaveError::Security(_)));
    }

    #[test]
    fn aes128_ecb_rejects_wrong_block_length() {
        let err = aes128_ecb_encrypt(&[0u8; 16], &[0u8; 8]).unwrap_err();
        assert!(matches!(err, ZwaveError::Security(_)));
    }

    #[test]
    fn hkdf_round_trip() {
        let prk = vec![0x42u8; 32];
        let out = hkdf_sha256_expand(&prk, b"cave-home", 32).unwrap();
        assert_eq!(out.len(), 32);
        // Deterministic.
        let out2 = hkdf_sha256_expand(&prk, b"cave-home", 32).unwrap();
        assert_eq!(out, out2);
    }

    fn hex16(s: &str) -> [u8; 16] {
        let bytes: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect();
        let mut out = [0u8; 16];
        out.copy_from_slice(&bytes);
        out
    }
}
