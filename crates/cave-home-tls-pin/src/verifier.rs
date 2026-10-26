// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The rustls [`ServerCertVerifier`]s behind [`crate::TlsTrust`].
//!
//! Every verifier here still checks the handshake signatures with the crypto
//! provider: a server that merely *replays* the pinned certificate without its
//! private key fails the handshake.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, TrustAnchor, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, OtherError, SignatureScheme};

use crate::fingerprint::{PinSet, Sha256Fingerprint};
use crate::x509;

/// The presented certificate matched no pin.
#[derive(Debug, thiserror::Error)]
#[error(
    "server certificate does not match the pinned fingerprint — presented certificate SHA-256 {presented}"
)]
pub struct PinMismatch {
    /// SHA-256 of the presented certificate.
    pub presented: Sha256Fingerprint,
}

/// The leaf chained to a trusted root but carried the wrong common name.
#[derive(Debug, thiserror::Error)]
#[error("server certificate common name {presented:?} is not the expected {expected:?}")]
pub struct CommonNameMismatch {
    /// The expected CN.
    pub expected: String,
    /// The presented CN, if exactly one could be read.
    pub presented: Option<String>,
}

fn reject(err: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::InvalidCertificate(CertificateError::Other(OtherError(Arc::new(err))))
}

/// Map a webpki path-building error onto the rustls error rustls itself uses.
fn pki_error(err: webpki::Error) -> Error {
    use webpki::Error as E;
    match err {
        E::BadDer | E::BadDerTime => Error::InvalidCertificate(CertificateError::BadEncoding),
        E::CertExpired { .. } => Error::InvalidCertificate(CertificateError::Expired),
        E::CertNotValidYet { .. } => Error::InvalidCertificate(CertificateError::NotValidYet),
        E::UnknownIssuer => Error::InvalidCertificate(CertificateError::UnknownIssuer),
        E::InvalidSignatureForPublicKey => Error::InvalidCertificate(CertificateError::BadSignature),
        other => reject(other),
    }
}

macro_rules! delegate_signatures {
    () => {
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
    };
}

/// Accepts a server whose leaf certificate matches a [`PinSet`].
///
/// The server name and validity period are deliberately **not** checked: a
/// LAN console's self-signed certificate names itself (`unifi.local`) while
/// the household reaches it by IP, and such certificates are minted once for
/// decades. The pin is the identity; the handshake signature proves the peer
/// holds its key.
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

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if self.pins.matches_certificate(end_entity.as_ref()) {
            Ok(ServerCertVerified::assertion())
        } else {
            let presented = Sha256Fingerprint::of(end_entity.as_ref());
            tracing::warn!(%presented, "TLS certificate pin mismatch — refusing the connection");
            Err(reject(PinMismatch { presented }))
        }
    }

    delegate_signatures!();
}

/// Accepts a leaf chaining to a private root whose subject CN is expected.
///
/// The chain is fully checked (validity, signatures, CA constraints, EKU)
/// against a set of private roots, and the leaf's subject common name must
/// equal the expected value, case-insensitively. This is how a Hue bridge is identified:
/// Signify root CA, CN = bridge id. SANs and the dialled name are not used —
/// bridge certificates carry no SAN and are reached by IP.
#[derive(Debug)]
pub struct CommonNameCaVerifier {
    roots: Vec<TrustAnchor<'static>>,
    common_name: String,
    provider: Arc<CryptoProvider>,
}

impl CommonNameCaVerifier {
    /// Build over `roots` (DER) and the expected `common_name`.
    ///
    /// # Errors
    /// A root that does not parse as a trust anchor.
    pub fn new(
        roots: &[CertificateDer<'static>],
        common_name: &str,
        provider: Arc<CryptoProvider>,
    ) -> Result<Self, Error> {
        let roots = roots
            .iter()
            .map(|der| webpki::anchor_from_trusted_cert(der).map(|a| a.to_owned()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(pki_error)?;
        Ok(Self {
            roots,
            common_name: common_name.to_owned(),
            provider,
        })
    }
}

impl ServerCertVerifier for CommonNameCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let cert = webpki::EndEntityCert::try_from(end_entity).map_err(pki_error)?;
        cert.verify_for_usage(
            self.provider.signature_verification_algorithms.all,
            &self.roots,
            intermediates,
            now,
            webpki::KeyUsage::server_auth(),
            None,
            None,
        )
        .map_err(pki_error)?;
        let presented = x509::common_name(cert.subject());
        match presented {
            Some(cn) if cn.eq_ignore_ascii_case(&self.common_name) => Ok(ServerCertVerified::assertion()),
            presented => {
                tracing::warn!(expected = %self.common_name, ?presented, "TLS common name mismatch");
                Err(reject(CommonNameMismatch {
                    expected: self.common_name.clone(),
                    presented,
                }))
            }
        }
    }

    delegate_signatures!();
}

/// Accepts every certificate. Only reachable through
/// [`crate::TlsTrust::DangerousAcceptAnyCertificate`], which logs each use.
#[derive(Debug)]
pub struct AcceptAnyCertificate {
    provider: Arc<CryptoProvider>,
}

impl AcceptAnyCertificate {
    pub const fn new(provider: Arc<CryptoProvider>) -> Self {
        Self { provider }
    }
}

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        tracing::warn!(
            server = ?server_name,
            presented = %Sha256Fingerprint::of(end_entity.as_ref()),
            "TLS certificate verification is DISABLED (explicit insecure opt-in) — accepting an unverified server"
        );
        Ok(ServerCertVerified::assertion())
    }

    delegate_signatures!();
}
