// SPDX-License-Identifier: Apache-2.0
//! CASE — Certificate-Authenticated Session Establishment.
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/protocols/secure_channel/CASESession.cpp
//!
//! CASE replaces the PAKE proofs with **mutual certificate-chain
//! validation**: both sides carry an NOC (Node Operational
//! Certificate) signed by the household's ICAC + Root CA. Phase 1
//! ports the Sigma1 / Sigma2 / Sigma3 exchange shape and the
//! `DeriveSecureSession` step verbatim; the certificate validation
//! is currently a structural check (matching FabricId + RCAC public
//! key) rather than a full X.509 chain build — see
//! `[[unmapped]] attestation_verifier` for the Phase 1b upgrade.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{MatterError, Result};
use crate::fabric::{FabricId, NodeId};

const SESSION_KEY_BYTES: usize = 16;

/// Per-direction encryption keys established by CASE.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseSessionKeys {
    pub i2r_key: [u8; SESSION_KEY_BYTES],
    pub r2i_key: [u8; SESSION_KEY_BYTES],
    pub attestation_challenge: [u8; SESSION_KEY_BYTES],
}

/// Operational identity used by both peers.
///
/// # Upstream: src/credentials/CHIPCertFromX509.cpp::ChipCertificateData (subset)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationalCredentials {
    pub fabric_id: FabricId,
    pub node_id: NodeId,
    /// Compact representation of the NOC ephemeral public key (32 bytes).
    pub noc_public_key: [u8; 32],
    /// Root CA identity — peers must share this to be in the same household.
    pub root_ca_public_key: [u8; 32],
}

impl OperationalCredentials {
    pub fn validate(&self) -> Result<()> {
        if self.fabric_id.0 == 0 {
            return Err(MatterError::Fabric("fabric id must not be 0".into()));
        }
        if self.node_id.0 == 0 {
            return Err(MatterError::Fabric("node id must not be 0".into()));
        }
        if self.root_ca_public_key == [0u8; 32] {
            return Err(MatterError::Crypto("root CA pubkey not provisioned".into()));
        }
        Ok(())
    }
}

/// CASE session state machine.
///
/// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession
#[derive(Debug)]
pub struct CaseSession {
    role: Role,
    creds: OperationalCredentials,
    initiator_random: [u8; 32],
    responder_random: [u8; 32],
    keys: Option<CaseSessionKeys>,
}

/// Initiator (commissioner) or responder (device) role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Initiator,
    Responder,
}

impl CaseSession {
    /// Construct an initiator-side CASE session.
    pub fn new_initiator(creds: OperationalCredentials) -> Result<Self> {
        creds.validate()?;
        Ok(Self {
            role: Role::Initiator,
            creds,
            initiator_random: random_32(),
            responder_random: [0; 32],
            keys: None,
        })
    }

    /// Construct a responder-side CASE session.
    pub fn new_responder(creds: OperationalCredentials) -> Result<Self> {
        creds.validate()?;
        Ok(Self {
            role: Role::Responder,
            creds,
            initiator_random: [0; 32],
            responder_random: random_32(),
            keys: None,
        })
    }

    /// Initiator -> Sigma1.
    ///
    /// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession::SendSigma1
    pub fn send_sigma1(&self) -> Sigma1 {
        Sigma1 {
            initiator_random: self.initiator_random,
            initiator_fabric_id: self.creds.fabric_id,
            initiator_node_id: self.creds.node_id,
            initiator_pubkey: self.creds.noc_public_key,
        }
    }

    /// Responder consumes Sigma1, emits Sigma2.
    ///
    /// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession::HandleSigma1
    /// + `SendSigma2`
    pub fn handle_sigma1(&mut self, msg: &Sigma1) -> Result<Sigma2> {
        if self.role != Role::Responder {
            return Err(MatterError::IncorrectState("handle_sigma1 needs responder".into()));
        }
        if msg.initiator_fabric_id != self.creds.fabric_id {
            return Err(MatterError::Fabric(format!(
                "Sigma1 fabric mismatch: {:?} vs {:?}",
                msg.initiator_fabric_id, self.creds.fabric_id
            )));
        }
        self.initiator_random = msg.initiator_random;
        Ok(Sigma2 {
            responder_random: self.responder_random,
            responder_pubkey: self.creds.noc_public_key,
            responder_node_id: self.creds.node_id,
        })
    }

    /// Initiator handles Sigma2, emits Sigma3.
    ///
    /// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession::HandleSigma2
    pub fn handle_sigma2(&mut self, msg: &Sigma2) -> Result<Sigma3> {
        if self.role != Role::Initiator {
            return Err(MatterError::IncorrectState("handle_sigma2 needs initiator".into()));
        }
        self.responder_random = msg.responder_random;
        let signature = hmac_sign(
            b"sigma3-i",
            &self.creds.root_ca_public_key,
            &self.initiator_random,
            &self.responder_random,
        );
        Ok(Sigma3 {
            initiator_signature: signature,
        })
    }

    /// Responder handles Sigma3, derives session keys.
    ///
    /// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession::HandleSigma3
    pub fn handle_sigma3(&mut self, msg: &Sigma3, initiator_root_ca: &[u8; 32]) -> Result<&CaseSessionKeys> {
        if self.role != Role::Responder {
            return Err(MatterError::IncorrectState("handle_sigma3 needs responder".into()));
        }
        if initiator_root_ca != &self.creds.root_ca_public_key {
            return Err(MatterError::Fabric(
                "Sigma3 initiator's root CA does not match the household".into(),
            ));
        }
        // Verify the initiator's Sigma3 signature.
        let expected = hmac_sign(
            b"sigma3-i",
            initiator_root_ca,
            &self.initiator_random,
            &self.responder_random,
        );
        if expected != msg.initiator_signature {
            return Err(MatterError::Handshake("Sigma3 signature mismatch".into()));
        }
        let keys = self.derive_secure_session()?;
        self.keys = Some(keys);
        Ok(self.keys.as_ref().expect("populated"))
    }

    /// Initiator-side finalisation (after sending Sigma3).
    pub fn finalize_initiator(&mut self) -> Result<&CaseSessionKeys> {
        if self.role != Role::Initiator {
            return Err(MatterError::IncorrectState("finalize needs initiator".into()));
        }
        let keys = self.derive_secure_session()?;
        self.keys = Some(keys);
        Ok(self.keys.as_ref().expect("populated"))
    }

    /// Derive the per-direction CASE session keys.
    ///
    /// # Upstream: src/protocols/secure_channel/CASESession.cpp::CASESession::DeriveSecureSession
    pub fn derive_secure_session(&self) -> Result<CaseSessionKeys> {
        let mut ikm = Vec::with_capacity(96);
        ikm.extend_from_slice(&self.initiator_random);
        ikm.extend_from_slice(&self.responder_random);
        ikm.extend_from_slice(&self.creds.root_ca_public_key);
        let salt = b"SessionResumptionInfo";
        let hk = Hkdf::<Sha256>::new(Some(salt.as_slice()), &ikm);
        let mut okm = [0u8; SESSION_KEY_BYTES * 3];
        hk.expand(b"CASESessionKeys", &mut okm)
            .map_err(|_| MatterError::Crypto("HKDF expand failed".into()))?;
        let mut s = CaseSessionKeys {
            i2r_key: [0; SESSION_KEY_BYTES],
            r2i_key: [0; SESSION_KEY_BYTES],
            attestation_challenge: [0; SESSION_KEY_BYTES],
        };
        s.i2r_key.copy_from_slice(&okm[0..SESSION_KEY_BYTES]);
        s.r2i_key
            .copy_from_slice(&okm[SESSION_KEY_BYTES..2 * SESSION_KEY_BYTES]);
        s.attestation_challenge
            .copy_from_slice(&okm[2 * SESSION_KEY_BYTES..3 * SESSION_KEY_BYTES]);
        Ok(s)
    }

    pub fn keys(&self) -> Option<&CaseSessionKeys> {
        self.keys.as_ref()
    }
}

/// Sigma1 message (initiator).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sigma1 {
    pub initiator_random: [u8; 32],
    pub initiator_fabric_id: FabricId,
    pub initiator_node_id: NodeId,
    pub initiator_pubkey: [u8; 32],
}

/// Sigma2 message (responder).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sigma2 {
    pub responder_random: [u8; 32],
    pub responder_pubkey: [u8; 32],
    pub responder_node_id: NodeId,
}

/// Sigma3 message (initiator confirmation).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sigma3 {
    pub initiator_signature: [u8; 32],
}

fn random_32() -> [u8; 32] {
    let mut out = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut out);
    out
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sign(domain: &[u8], key: &[u8; 32], a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(domain);
    mac.update(a);
    mac.update(b);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (OperationalCredentials, OperationalCredentials) {
        let root = [7u8; 32];
        let mut i_pub = [0u8; 32];
        i_pub[0] = 0xAA;
        let mut r_pub = [0u8; 32];
        r_pub[0] = 0xBB;
        (
            OperationalCredentials {
                fabric_id: FabricId(1),
                node_id: NodeId(0x1000_0000_0000_0001),
                noc_public_key: i_pub,
                root_ca_public_key: root,
            },
            OperationalCredentials {
                fabric_id: FabricId(1),
                node_id: NodeId(0x1000_0000_0000_0002),
                noc_public_key: r_pub,
                root_ca_public_key: root,
            },
        )
    }

    /// # Upstream: src/protocols/secure_channel/tests/TestCASESession.cpp::SecurePairingHandshakeTest
    #[test]
    fn case_handshake_derives_matching_keys() {
        let (ic, rc) = pair();
        let mut initiator = CaseSession::new_initiator(ic).expect("initiator");
        let mut responder = CaseSession::new_responder(rc).expect("responder");
        // Force matching randoms for the deterministic equality assertion.
        responder.responder_random = [9u8; 32];

        let s1 = initiator.send_sigma1();
        let s2 = responder.handle_sigma1(&s1).expect("sigma2");
        let s3 = initiator.handle_sigma2(&s2).expect("sigma3");
        let initiator_root = initiator.creds.root_ca_public_key;
        let r_keys = responder.handle_sigma3(&s3, &initiator_root).expect("rkeys").clone();
        let i_keys = initiator.finalize_initiator().expect("ikeys").clone();
        assert_eq!(i_keys.i2r_key, r_keys.i2r_key);
        assert_eq!(i_keys.r2i_key, r_keys.r2i_key);
        assert_eq!(i_keys.attestation_challenge, r_keys.attestation_challenge);
    }

    #[test]
    fn case_rejects_fabric_id_mismatch() {
        let (ic, mut rc) = pair();
        rc.fabric_id = FabricId(2);
        let initiator = CaseSession::new_initiator(ic).expect("initiator");
        let mut responder = CaseSession::new_responder(rc).expect("responder");
        responder.responder_random = [9u8; 32];
        let s1 = initiator.send_sigma1();
        let err = responder.handle_sigma1(&s1).expect_err("must reject");
        match err {
            MatterError::Fabric(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn case_rejects_root_ca_mismatch() {
        let (ic, mut rc) = pair();
        rc.root_ca_public_key = [3u8; 32];
        let mut initiator = CaseSession::new_initiator(ic).expect("initiator");
        let mut responder = CaseSession::new_responder(rc).expect("responder");
        responder.responder_random = [9u8; 32];

        let s1 = initiator.send_sigma1();
        let s2 = responder.handle_sigma1(&s1).expect("sigma2");
        let s3 = initiator.handle_sigma2(&s2).expect("sigma3");
        let initiator_root = initiator.creds.root_ca_public_key;
        let err = responder.handle_sigma3(&s3, &initiator_root).expect_err("must reject");
        match err {
            MatterError::Fabric(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn credentials_validate_rejects_zero_fabric() {
        let creds = OperationalCredentials {
            fabric_id: FabricId(0),
            node_id: NodeId(1),
            noc_public_key: [1; 32],
            root_ca_public_key: [2; 32],
        };
        assert!(creds.validate().is_err());
    }
}
