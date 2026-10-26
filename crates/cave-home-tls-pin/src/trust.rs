// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The trust policy a transport is built with.

use std::sync::Arc;

use rustls::client::danger::ServerCertVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;

use crate::fingerprint::PinSet;
use crate::verifier::{AcceptAnyCertificate, CommonNameCaVerifier, PinnedCertVerifier};

/// How a client decides whether to trust a server certificate.
///
/// There is deliberately no default: every transport that talks to a LAN
/// appliance must be handed one of these, so "accept anything" can only ever
/// be a visible choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsTrust {
    /// Accept only a certificate matching one of these pins.
    Pinned(PinSet),
    /// Chain to one of `roots`; require the leaf CN to equal `common_name`
    /// (case-insensitive).
    CommonNameCa {
        /// Trust roots (DER).
        roots: Vec<CertificateDer<'static>>,
        /// Expected leaf subject CN.
        common_name: String,
    },
    /// Accept any certificate. An explicit opt-in for a household that chose
    /// to trust its LAN; a warning is logged every time a config is built
    /// from it and on every connection it accepts.
    DangerousAcceptAnyCertificate,
}

/// Why a trust policy could not be built.
#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    /// A pinned policy with no pins.
    #[error("no certificate pin configured")]
    NoPins,
    /// A CA policy with no roots.
    #[error("no trust root configured")]
    NoRoots,
    /// A CA policy with an empty expected common name.
    #[error("empty expected common name")]
    EmptyCommonName,
    /// rustls rejected the configuration (e.g. an unparseable root).
    #[error("TLS configuration: {0}")]
    Tls(#[from] rustls::Error),
}

impl TlsTrust {
    /// Pin to `pins`.
    ///
    /// # Errors
    /// [`TrustError::NoPins`] for an empty set — an empty pin set must never
    /// silently mean "trust nothing" or "trust anything".
    pub fn pinned(pins: PinSet) -> Result<Self, TrustError> {
        if pins.is_empty() {
            return Err(TrustError::NoPins);
        }
        Ok(Self::Pinned(pins))
    }

    /// Private root(s) plus an expected leaf common name.
    ///
    /// # Errors
    /// No roots, a blank name, or a root that does not parse.
    pub fn common_name_ca(roots: Vec<CertificateDer<'static>>, common_name: &str) -> Result<Self, TrustError> {
        if roots.is_empty() {
            return Err(TrustError::NoRoots);
        }
        let common_name = common_name.trim();
        if common_name.is_empty() {
            return Err(TrustError::EmptyCommonName);
        }
        let trust = Self::CommonNameCa {
            roots,
            common_name: common_name.to_owned(),
        };
        // Parse the roots now so a bad root fails at configuration time.
        trust.verifier(&provider())?;
        Ok(trust)
    }

    /// Accept any certificate. See [`TlsTrust::DangerousAcceptAnyCertificate`].
    #[must_use]
    pub const fn dangerous_accept_any_certificate() -> Self {
        Self::DangerousAcceptAnyCertificate
    }

    /// Whether this policy disables verification.
    #[must_use]
    pub const fn is_dangerous(&self) -> bool {
        matches!(self, Self::DangerousAcceptAnyCertificate)
    }

    fn verifier(&self, provider: &Arc<CryptoProvider>) -> Result<Arc<dyn ServerCertVerifier>, TrustError> {
        Ok(match self {
            Self::Pinned(pins) => {
                if pins.is_empty() {
                    return Err(TrustError::NoPins);
                }
                Arc::new(PinnedCertVerifier::new(pins.clone(), Arc::clone(provider)))
            }
            Self::CommonNameCa { roots, common_name } => {
                if roots.is_empty() {
                    return Err(TrustError::NoRoots);
                }
                Arc::new(CommonNameCaVerifier::new(roots, common_name, Arc::clone(provider))?)
            }
            Self::DangerousAcceptAnyCertificate => {
                tracing::warn!(
                    "building a TLS client that accepts ANY server certificate (explicit insecure opt-in): \
                     credentials sent over it can be read by anyone on the network path"
                );
                Arc::new(AcceptAnyCertificate::new(Arc::clone(provider)))
            }
        })
    }

    /// Build the `rustls` client config (ring provider, safe default protocol
    /// versions, no client auth, no ALPN — set `alpn_protocols` if needed).
    ///
    /// # Errors
    /// See [`TrustError`].
    pub fn client_config(&self) -> Result<rustls::ClientConfig, TrustError> {
        let provider = provider();
        let verifier = self.verifier(&provider)?;
        Ok(rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth())
    }
}

/// The crypto provider: `ring`, chosen explicitly so the result never depends
/// on which provider another crate in the graph installed as process default.
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}
