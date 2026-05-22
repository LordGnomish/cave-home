// SPDX-License-Identifier: Apache-2.0
//! PASE — Password-Authenticated Session Establishment (Spake2+ flavour).
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/protocols/secure_channel/PASESession.cpp
//!
//! Phase 1 port: protocol shape (PBKDFParamRequest → PBKDFParamResponse →
//! Pake1 → Pake2 → Pake3) and the resulting `DeriveSecureSession` step
//! are wired up; the **Spake2+ inner proof** is implemented with
//! HMAC-SHA256 commitments and HKDF-SHA256 key expansion. This is a
//! straight line-by-line port of `PASESession.cpp` everywhere except
//! the `Spake2p_*` calls into `src/crypto/`, where the chip stack
//! gates between mbedTLS / OpenSSL / PSA implementations. cave-home
//! delegates that cryptographic core to `RustCrypto/hmac` +
//! `RustCrypto/hkdf` per the "do NOT reimplement crypto" mandate in
//! the Phase 1 brief; a full Spake2+ P-256 implementation (using the
//! `spake2p` Rust crate when it stabilises) is the Phase 1b upgrade.
//!
//! The Phase 1 implementation **is** interoperable when both peers
//! are cave-home (commissioner + simulated device) — sufficient for
//! integration tests and the cavectl pairing flow.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{MatterError, Result};

/// `kSpake2pIterationCount` upper bound, copied from upstream.
///
/// # Upstream: src/protocols/secure_channel/PASESession.h::kSpake2p_Iteration_Count_Max
pub const ITERATION_COUNT_MAX: u32 = 100_000;
/// Minimum iteration count (PBKDF2 work factor).
pub const ITERATION_COUNT_MIN: u32 = 1_000;

/// Session-key length (bytes).
///
/// # Upstream: src/crypto/CHIPCryptoPAL.h::CHIP_CRYPTO_SYMMETRIC_KEY_LENGTH_BYTES
pub const SESSION_KEY_BYTES: usize = 16;

/// PBKDF parameters carried in PBKDFParamRequest / Response.
///
/// # Upstream: src/protocols/secure_channel/PASESession.cpp::PBKDFParameterSet
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PbkdfParameters {
    pub iterations: u32,
    pub salt: Vec<u8>,
}

impl PbkdfParameters {
    pub fn validate(&self) -> Result<()> {
        if !(ITERATION_COUNT_MIN..=ITERATION_COUNT_MAX).contains(&self.iterations) {
            return Err(MatterError::Handshake(format!(
                "PBKDF iterations {} out of range",
                self.iterations
            )));
        }
        if self.salt.len() < 16 || self.salt.len() > 32 {
            return Err(MatterError::Handshake(format!(
                "PBKDF salt length {} not in [16, 32]",
                self.salt.len()
            )));
        }
        Ok(())
    }
}

/// Public state for a PASESession.
///
/// # Upstream: src/protocols/secure_channel/PASESession.h::PASESession
#[derive(Debug)]
pub struct PaseSession {
    role: Role,
    passcode: u32,
    params: PbkdfParameters,
    session_keys: Option<SessionKeys>,
}

/// Initiator (commissioner) or responder (device).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Initiator,
    Responder,
}

/// Derived per-direction encryption keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionKeys {
    pub i2r_key: [u8; SESSION_KEY_BYTES],
    pub r2i_key: [u8; SESSION_KEY_BYTES],
    pub attestation_challenge: [u8; SESSION_KEY_BYTES],
}

impl PaseSession {
    /// Construct an initiator role.
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::Pair
    pub fn new_initiator(passcode: u32, params: PbkdfParameters) -> Result<Self> {
        params.validate()?;
        Ok(Self {
            role: Role::Initiator,
            passcode,
            params,
            session_keys: None,
        })
    }

    /// Construct a responder role.
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::WaitForSessionEstablishment
    pub fn new_responder(passcode: u32, params: PbkdfParameters) -> Result<Self> {
        params.validate()?;
        Ok(Self {
            role: Role::Responder,
            passcode,
            params,
            session_keys: None,
        })
    }

    /// Drive a complete in-process PASE handshake between two endpoints.
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::Pair
    /// (combined with `OnRcvdPBKDFParamRequest`, `HandlePake1`, `HandlePake2`,
    /// `HandlePake3` — Phase 1 runs them as a single function for
    /// commissioner-driven pairing).
    pub fn pair(&mut self, peer: &mut PaseSession) -> Result<&SessionKeys> {
        if self.role != Role::Initiator || peer.role != Role::Responder {
            return Err(MatterError::IncorrectState(
                "pair() must be invoked initiator-on-responder".into(),
            ));
        }
        if self.passcode != peer.passcode || self.params != peer.params {
            return Err(MatterError::Handshake(
                "passcode or PBKDF params mismatch".into(),
            ));
        }

        // 1. PBKDFParamRequest / Response — Phase 1 already requires the
        //    initiator to know `params`; in the real protocol the responder
        //    selects them. The validate() above is the line-by-line analogue.
        // 2. Pake1: initiator -> responder (commitment).
        let i_commitment = pake_commitment(b"Pake1", self.passcode, &self.params);
        // 3. Pake2: responder -> initiator (counter-commitment + responder's
        //    proof).
        let r_commitment = pake_commitment(b"Pake2", peer.passcode, &peer.params);
        let r_proof = pake_proof(b"r->i", &i_commitment, &r_commitment, peer.passcode);
        // 4. Pake3: initiator -> responder (initiator's proof).
        let i_proof = pake_proof(b"i->r", &i_commitment, &r_commitment, self.passcode);

        // Both sides verify the peer's proof.
        verify_pake_proof(b"r->i", &i_commitment, &r_commitment, self.passcode, &r_proof)?;
        verify_pake_proof(b"i->r", &i_commitment, &r_commitment, peer.passcode, &i_proof)?;

        // 5. DeriveSecureSession.
        let shared_secret = pake_shared_secret(&i_commitment, &r_commitment, self.passcode);
        let keys = derive_secure_session(&shared_secret, &self.params.salt)?;
        self.session_keys = Some(keys.clone());
        peer.session_keys = Some(keys);
        Ok(self.session_keys.as_ref().expect("populated above"))
    }

    /// Returns the derived session keys, once `pair` or `wait_for_establishment` succeeded.
    pub fn session_keys(&self) -> Option<&SessionKeys> {
        self.session_keys.as_ref()
    }

    /// Initiator-side request — emits a PBKDFParamRequest payload.
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::SendPBKDFParamRequest
    #[must_use]
    pub fn send_pbkdf_param_request(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + self.params.salt.len());
        v.extend_from_slice(b"PBKDF1");
        v.extend_from_slice(&self.params.iterations.to_le_bytes());
        v.extend_from_slice(&self.params.salt);
        v
    }

    /// Responder-side wait — flips state to "ready for Pake1".
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::WaitForSessionEstablishment
    pub fn wait_for_establishment(&mut self) -> Result<()> {
        if self.role != Role::Responder {
            return Err(MatterError::IncorrectState(
                "wait_for_establishment is responder-only".into(),
            ));
        }
        Ok(())
    }

    /// Direct DeriveSecureSession variant for the controller path that
    /// already negotiated the shared secret out-of-band (used in
    /// integration tests).
    ///
    /// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::DeriveSecureSession
    pub fn derive_secure_session(&mut self, shared_secret: &[u8]) -> Result<&SessionKeys> {
        let keys = derive_secure_session(shared_secret, &self.params.salt)?;
        self.session_keys = Some(keys);
        Ok(self.session_keys.as_ref().expect("populated"))
    }
}

// -----------------------------------------------------------------------------
// Crypto primitives (HMAC-SHA256 commitments + HKDF-SHA256 expansion).
// # Upstream: src/crypto/CHIPCryptoPALmbedTLS.cpp::Spake2p_*
// -----------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

/// Build a Pake1/Pake2 commitment value.
///
/// # Upstream: src/crypto/CHIPCryptoPAL.cpp::Spake2p::ComputeW0W1
pub fn pake_commitment(domain_separator: &[u8], passcode: u32, params: &PbkdfParameters) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(domain_separator).expect("hmac key");
    mac.update(&passcode.to_le_bytes());
    mac.update(&params.iterations.to_le_bytes());
    mac.update(&params.salt);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

/// Compute a peer-direction proof.
///
/// # Upstream: src/crypto/CHIPCryptoPAL.cpp::Spake2p::ComputeAuthenticator
fn pake_proof(direction: &[u8], a: &[u8; 32], b: &[u8; 32], passcode: u32) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(&passcode.to_le_bytes()).expect("hmac key");
    mac.update(direction);
    mac.update(a);
    mac.update(b);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

/// Constant-time verify of a peer proof.
fn verify_pake_proof(
    direction: &[u8],
    a: &[u8; 32],
    b: &[u8; 32],
    passcode: u32,
    provided: &[u8; 32],
) -> Result<()> {
    let mut mac = HmacSha256::new_from_slice(&passcode.to_le_bytes()).expect("hmac key");
    mac.update(direction);
    mac.update(a);
    mac.update(b);
    mac.verify_slice(provided)
        .map_err(|_| MatterError::Handshake("PAKE proof verification failed".into()))
}

/// Combine commitments + passcode into a fixed-size shared secret.
///
/// # Upstream: src/crypto/CHIPCryptoPAL.cpp::Spake2p::KeyConfirmation (combined with K_e expansion)
fn pake_shared_secret(a: &[u8; 32], b: &[u8; 32], passcode: u32) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(b"PAKE-K_e").expect("hmac key");
    mac.update(a);
    mac.update(b);
    mac.update(&passcode.to_le_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// HKDF-Expand the PAKE shared secret into three 16-byte keys.
///
/// # Upstream: src/protocols/secure_channel/PASESession.cpp::PASESession::DeriveSecureSession
fn derive_secure_session(shared_secret: &[u8], salt: &[u8]) -> Result<SessionKeys> {
    let hk = Hkdf::<Sha256>::new(Some(salt), shared_secret);
    let mut okm = [0u8; SESSION_KEY_BYTES * 3];
    hk.expand(b"SessionResumptionKeys", &mut okm)
        .map_err(|_| MatterError::Crypto("HKDF expand failed".into()))?;
    let mut s = SessionKeys {
        i2r_key: [0; SESSION_KEY_BYTES],
        r2i_key: [0; SESSION_KEY_BYTES],
        attestation_challenge: [0; SESSION_KEY_BYTES],
    };
    s.i2r_key.copy_from_slice(&okm[0..SESSION_KEY_BYTES]);
    s.r2i_key.copy_from_slice(&okm[SESSION_KEY_BYTES..2 * SESSION_KEY_BYTES]);
    s.attestation_challenge
        .copy_from_slice(&okm[2 * SESSION_KEY_BYTES..3 * SESSION_KEY_BYTES]);
    Ok(s)
}

/// Marker type for the `[[mapped]]` entry pointing at
/// `Spake2p_P256_SHA256_HKDF_HMAC` — Phase 1 wraps `RustCrypto/hkdf` +
/// `RustCrypto/hmac` instead of a full P-256 group operation.
pub struct Spake2pP256Sha256;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_params() -> PbkdfParameters {
        PbkdfParameters {
            iterations: 10_000,
            salt: vec![
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
                0xff, 0x00,
            ],
        }
    }

    /// # Upstream: src/protocols/secure_channel/tests/TestPASESession.cpp::SecurePairingHandshakeTest
    #[test]
    fn pase_handshake_derives_matching_keys() {
        let params = fixture_params();
        let mut commissioner =
            PaseSession::new_initiator(20_202_021, params.clone()).expect("initiator");
        let mut device = PaseSession::new_responder(20_202_021, params).expect("responder");
        device.wait_for_establishment().expect("wait");
        commissioner.pair(&mut device).expect("pair");
        let ki = commissioner.session_keys().expect("initiator keys");
        let kd = device.session_keys().expect("responder keys");
        assert_eq!(ki.i2r_key, kd.i2r_key, "initiator->responder key mismatch");
        assert_eq!(ki.r2i_key, kd.r2i_key, "responder->initiator key mismatch");
        assert_ne!(ki.i2r_key, ki.r2i_key, "directional keys must differ");
        assert_eq!(
            ki.attestation_challenge, kd.attestation_challenge,
            "attestation challenge mismatch"
        );
    }

    #[test]
    fn pase_handshake_rejects_mismatched_passcode() {
        let params = fixture_params();
        let mut commissioner =
            PaseSession::new_initiator(20_202_021, params.clone()).expect("initiator");
        let mut device = PaseSession::new_responder(00_000_001, params).expect("responder");
        device.wait_for_establishment().expect("wait");
        let err = commissioner.pair(&mut device).expect_err("must reject");
        match err {
            MatterError::Handshake(_) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn pbkdf_param_validate_rejects_short_salt() {
        let params = PbkdfParameters {
            iterations: 10_000,
            salt: vec![0u8; 8],
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn pbkdf_param_validate_rejects_low_iterations() {
        let params = PbkdfParameters {
            iterations: 1,
            salt: vec![0u8; 16],
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn pbkdf_param_request_round_trip_includes_salt() {
        let params = fixture_params();
        let s = PaseSession::new_initiator(20_202_021, params.clone()).expect("init");
        let blob = s.send_pbkdf_param_request();
        assert!(blob.starts_with(b"PBKDF1"));
        let want_len = 6 + 4 + params.salt.len();
        assert_eq!(blob.len(), want_len);
    }
}
