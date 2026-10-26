// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Trust-on-first-use capture for onboarding.
//!
//! [`capture_presented_certificate`] opens a TLS handshake, records the leaf
//! certificate the host presents, and then **aborts** the handshake from the
//! certificate check — no application byte (so no credential) is ever sent.
//! The caller shows the fingerprint to the household (ideally next to the one
//! the appliance's own UI shows) and stores the pin they confirm.
//!
//! TOFU's limit is honest: if an attacker already answers for the host at
//! capture time, their certificate is what gets captured. Capture on a trusted
//! link (cable, the appliance's own setup network) when it matters.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};

use crate::fingerprint::{spki_fingerprint, Pin, Sha256Fingerprint};
use crate::trust::provider;

/// The certificate a server presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedCertificate {
    der: CertificateDer<'static>,
    certificate_sha256: Sha256Fingerprint,
    public_key_sha256: Option<Sha256Fingerprint>,
}

impl PresentedCertificate {
    /// Fingerprint a DER certificate.
    #[must_use]
    pub fn from_der(der: CertificateDer<'static>) -> Self {
        let certificate_sha256 = Sha256Fingerprint::of(der.as_ref());
        let public_key_sha256 = spki_fingerprint(der.as_ref());
        Self {
            der,
            certificate_sha256,
            public_key_sha256,
        }
    }

    /// The DER certificate.
    #[must_use]
    pub const fn der(&self) -> &CertificateDer<'static> {
        &self.der
    }

    /// SHA-256 of the certificate.
    #[must_use]
    pub const fn certificate_sha256(&self) -> Sha256Fingerprint {
        self.certificate_sha256
    }

    /// SHA-256 of the `SubjectPublicKeyInfo`, if the certificate parses.
    #[must_use]
    pub const fn public_key_sha256(&self) -> Option<Sha256Fingerprint> {
        self.public_key_sha256
    }

    /// A pin for exactly this certificate.
    #[must_use]
    pub const fn certificate_pin(&self) -> Pin {
        Pin::Certificate(self.certificate_sha256)
    }

    /// A pin for this certificate's public key (survives a same-key re-issue).
    #[must_use]
    pub fn public_key_pin(&self) -> Option<Pin> {
        self.public_key_sha256.map(Pin::PublicKey)
    }
}

/// Why a capture failed.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// The host name / address could not be resolved or used as a TLS name.
    #[error("bad address {0:?}")]
    BadAddress(String),
    /// TCP connect / IO failure.
    #[error("connect: {0}")]
    Io(#[from] std::io::Error),
    /// The handshake failed before a certificate was presented.
    #[error("TLS handshake failed before a certificate was presented: {0}")]
    Tls(String),
}

/// Records the leaf and stops the handshake.
#[derive(Debug)]
struct Capture {
    slot: Mutex<Option<CertificateDer<'static>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Capture {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        *self.slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(end_entity.clone().into_owned());
        // Abort: capture never proceeds to send application data.
        Err(Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure))
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

/// Connect to `addr` (`host:port`), read the certificate it presents for
/// `server_name` (sent as SNI when it is a DNS name), and abort the handshake.
///
/// # Errors
/// See [`CaptureError`].
pub fn capture_presented_certificate(
    addr: &str,
    server_name: &str,
    timeout: Duration,
) -> Result<PresentedCertificate, CaptureError> {
    let addrs: Vec<_> = addr
        .to_socket_addrs()
        .map_err(|_| CaptureError::BadAddress(addr.to_owned()))?
        .collect();
    let mut last_err = None;
    for sock in addrs {
        match TcpStream::connect_timeout(&sock, timeout) {
            Ok(mut tcp) => {
                tcp.set_read_timeout(Some(timeout))?;
                tcp.set_write_timeout(Some(timeout))?;
                return capture_over(&mut tcp, server_name);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.map_or_else(|| CaptureError::BadAddress(addr.to_owned()), CaptureError::Io))
}

/// Read the presented certificate over an already-connected stream.
///
/// # Errors
/// See [`CaptureError`].
pub fn capture_over<S: Read + Write>(stream: &mut S, server_name: &str) -> Result<PresentedCertificate, CaptureError> {
    let name = ServerName::try_from(server_name.to_owned()).map_err(|_| CaptureError::BadAddress(server_name.to_owned()))?;
    let provider = provider();
    let capture = Arc::new(Capture {
        slot: Mutex::new(None),
        provider: Arc::clone(&provider),
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| CaptureError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::clone(&capture) as Arc<dyn ServerCertVerifier>)
        .with_no_client_auth();
    let mut conn =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| CaptureError::Tls(e.to_string()))?;

    let mut failure = None;
    let mut idle_rounds = 0;
    while conn.is_handshaking() {
        match conn.complete_io(stream) {
            Ok((0, 0)) => {
                idle_rounds += 1;
                if idle_rounds > 2 {
                    failure = Some("connection closed during handshake".to_owned());
                    break;
                }
            }
            Ok(_) => idle_rounds = 0,
            Err(e) => {
                failure = Some(e.to_string());
                break;
            }
        }
    }
    let captured = capture.slot.lock().unwrap_or_else(PoisonError::into_inner).take();
    captured.map(PresentedCertificate::from_der).ok_or_else(|| {
        CaptureError::Tls(failure.unwrap_or_else(|| "server presented no certificate".to_owned()))
    })
}
