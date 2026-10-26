// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Trust-on-first-use capture (RED skeleton).

use std::io::{Read, Write};
use std::time::Duration;

use rustls::pki_types::CertificateDer;

use crate::fingerprint::{Pin, Sha256Fingerprint};

/// The certificate a server presented.
#[derive(Debug, Clone)]
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
        Self {
            der,
            certificate_sha256,
            public_key_sha256: None,
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

    /// SHA-256 of the SubjectPublicKeyInfo, if parseable.
    #[must_use]
    pub const fn public_key_sha256(&self) -> Option<Sha256Fingerprint> {
        self.public_key_sha256
    }

    /// A certificate pin.
    #[must_use]
    pub const fn certificate_pin(&self) -> Pin {
        Pin::Certificate(self.certificate_sha256)
    }

    /// A public-key pin.
    #[must_use]
    pub fn public_key_pin(&self) -> Option<Pin> {
        self.public_key_sha256.map(Pin::PublicKey)
    }
}

/// Why a capture failed.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// Not implemented yet.
    #[error("capture not implemented")]
    Unimplemented,
}

/// Connect to `addr` and read the presented certificate.
///
/// # Errors
/// See [`CaptureError`].
pub fn capture_presented_certificate(
    _addr: &str,
    _server_name: &str,
    _timeout: Duration,
) -> Result<PresentedCertificate, CaptureError> {
    Err(CaptureError::Unimplemented)
}

/// Read the presented certificate over an already-connected stream.
///
/// # Errors
/// See [`CaptureError`].
pub fn capture_over<S: Read + Write>(_stream: &mut S, _server_name: &str) -> Result<PresentedCertificate, CaptureError> {
    Err(CaptureError::Unimplemented)
}
