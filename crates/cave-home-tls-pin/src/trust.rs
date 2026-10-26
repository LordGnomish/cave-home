// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The trust policy (RED skeleton).

use std::sync::Arc;

use rustls::pki_types::CertificateDer;

use crate::fingerprint::PinSet;
use crate::verifier::{CommonNameCaVerifier, PinnedCertVerifier};

/// How a client decides whether to trust a server certificate.
#[derive(Debug, Clone)]
pub enum TlsTrust {
    /// Accept only a certificate matching one of these pins.
    Pinned(PinSet),
    /// Chain to one of `roots`; require the leaf CN to equal `common_name`.
    CommonNameCa {
        /// Trust roots (DER).
        roots: Vec<CertificateDer<'static>>,
        /// Expected leaf subject CN (case-insensitive).
        common_name: String,
    },
    /// Accept any certificate. Explicit opt-in only; logged on every use.
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
    /// rustls rejected the configuration.
    #[error("TLS configuration: {0}")]
    Tls(#[from] rustls::Error),
}

impl TlsTrust {
    /// Pin to `pins`.
    ///
    /// # Errors
    /// [`TrustError::NoPins`] for an empty set.
    pub fn pinned(pins: PinSet) -> Result<Self, TrustError> {
        Ok(Self::Pinned(pins))
    }

    /// Private root(s) plus an expected common name.
    ///
    /// # Errors
    /// No roots, an empty name, or an unparseable root.
    pub fn common_name_ca(roots: Vec<CertificateDer<'static>>, common_name: &str) -> Result<Self, TrustError> {
        Ok(Self::CommonNameCa {
            roots,
            common_name: common_name.to_owned(),
        })
    }

    /// Accept any certificate (logged).
    #[must_use]
    pub const fn dangerous_accept_any_certificate() -> Self {
        Self::DangerousAcceptAnyCertificate
    }

    /// Whether this policy disables verification.
    #[must_use]
    pub const fn is_dangerous(&self) -> bool {
        false
    }

    /// Build the rustls client config.
    ///
    /// # Errors
    /// See [`TrustError`].
    pub fn client_config(&self) -> Result<rustls::ClientConfig, TrustError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> = match self {
            Self::Pinned(pins) => Arc::new(PinnedCertVerifier::new(pins.clone(), Arc::clone(&provider))),
            Self::CommonNameCa { roots, common_name } => Arc::new(CommonNameCaVerifier::new(
                roots,
                common_name,
                Arc::clone(&provider),
            )?),
            Self::DangerousAcceptAnyCertificate => Arc::new(PinnedCertVerifier::new(
                PinSet::default(),
                Arc::clone(&provider),
            )),
        };
        Ok(rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth())
    }
}
