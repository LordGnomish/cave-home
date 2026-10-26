// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `cave-home-tls-pin` — TLS trust for LAN appliances.
//!
//! UniFi consoles and Philips Hue bridges serve HTTPS behind certificates no
//! public CA vouches for, so "verify against the WebPKI" always fails and
//! "accept anything" hands every admin password and door-unlock token to
//! whoever answers on the LAN (the charter's threat model includes a
//! compromised IoT device on the same network). This crate is the middle path:
//!
//! - [`TlsTrust::Pinned`] — accept exactly the certificate (or public key) the
//!   household pinned, by SHA-256 fingerprint. The server must still prove it
//!   holds the private key (handshake signatures are verified as usual).
//! - [`TlsTrust::CommonNameCa`] — chain to a configured private root and
//!   require the leaf's subject CN to equal an expected value (the Hue model:
//!   Signify root CA, CN = bridge id).
//! - [`TlsTrust::DangerousAcceptAnyCertificate`] — the old behaviour, kept only
//!   as an explicitly named opt-in that logs a warning every time it is used.
//!
//! [`TlsTrust::client_config`] produces a `rustls::ClientConfig` that the
//! callers hand to `reqwest` (`use_preconfigured_tls`) and to
//! `tokio-tungstenite` (`Connector::Rustls`). Any mismatch fails the handshake
//! before a single application byte — and so any credential — is sent.
//!
//! [`capture_presented_certificate`] is the trust-on-first-use helper for
//! onboarding: it reads the certificate a host presents, aborts the handshake,
//! and returns the fingerprints to show the household and store as the pin.

mod capture;
mod fingerprint;
mod trust;
mod verifier;
mod x509;

#[cfg(feature = "test-support")]
pub mod test_support;

/// The `rustls` PKI types (`CertificateDer`, …) used in this API.
pub use rustls::pki_types as rustls_pki;

pub use capture::{capture_over, capture_presented_certificate, CaptureError, PresentedCertificate};
pub use fingerprint::{Pin, PinParseError, PinSet, Sha256Fingerprint};
pub use trust::{TlsTrust, TrustError};
pub use verifier::{CommonNameCaVerifier, CommonNameMismatch, PinMismatch, PinnedCertVerifier};
