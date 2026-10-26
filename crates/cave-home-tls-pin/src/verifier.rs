// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! rustls verifiers (RED skeleton: still accepts any certificate, as the
//! transports did before this change).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};

use crate::fingerprint::{PinSet, Sha256Fingerprint};

/// The presented certificate matched no pin.
#[derive(Debug, thiserror::Error)]
#[error("certificate pin mismatch")]
pub struct PinMismatch {
    /// SHA-256 of the presented certificate.
    pub presented: Sha256Fingerprint,
}

/// The leaf's common name was not the expected one.
#[derive(Debug, thiserror::Error)]
#[error("common name mismatch")]
pub struct CommonNameMismatch {
    /// The expected CN.
    pub expected: String,
    /// The presented CN, if one could be read.
    pub presented: Option<String>,
}

/// Accepts a server whose certificate matches a [`PinSet`].
#[derive(Debug)]
pub struct PinnedCertVerifier {
    pins: PinSet,
    provider: Arc<CryptoProvider>,
}

impl PinnedCertVerifier {
    /// Build over `pins`, verifying handshake signatures with `provider`.
    #[must_use]
    pub const fn new(pins: PinSet, provider: Arc<CryptoProvider>) -> Self {
        Self { pins, provider }
    }
}

/// Accepts a leaf chaining to a private root whose CN matches.
#[derive(Debug)]
pub struct CommonNameCaVerifier {
    common_name: String,
    provider: Arc<CryptoProvider>,
}

impl CommonNameCaVerifier {
    /// Build over `roots` (DER) and the expected `common_name`.
    ///
    /// # Errors
    /// A root that does not parse as a trust anchor.
    pub fn new(
        _roots: &[CertificateDer<'static>],
        common_name: &str,
        provider: Arc<CryptoProvider>,
    ) -> Result<Self, Error> {
        Ok(Self {
            common_name: common_name.to_owned(),
            provider,
        })
    }
}

macro_rules! accept_any_impl {
    ($t:ty) => {
        impl ServerCertVerifier for $t {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, Error> {
                let _ = &self.provider;
                Ok(ServerCertVerified::assertion())
            }
            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, Error> {
                verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
            }
            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, Error> {
                verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
            }
            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                self.provider.signature_verification_algorithms.supported_schemes()
            }
        }
    };
}

accept_any_impl!(PinnedCertVerifier);
accept_any_impl!(CommonNameCaVerifier);
