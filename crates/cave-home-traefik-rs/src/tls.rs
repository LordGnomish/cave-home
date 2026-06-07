// SPDX-License-Identifier: Apache-2.0
//! TLS termination: PEM loading and SNI-based certificate selection.
//!
//! Spec basis: Traefik terminates TLS on its HTTPS entrypoints and selects the
//! certificate by SNI, matching the most specific configured domain (exact then
//! wildcard) and falling back to a default certificate.
//!
//! This builds a rustls [`ServerConfig`] driven by a [`SniResolver`]. The
//! selection logic is factored into [`SniResolver::select_cert`] so it is
//! unit-testable without constructing a TLS `ClientHello`.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;

/// A TLS configuration error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsError {
    /// The certificate PEM contained no certificates.
    NoCerts,
    /// The key PEM contained no usable private key.
    NoKey,
    /// A PEM block could not be parsed.
    BadPem(String),
    /// The private key was not a supported signing key.
    Sign(String),
}

impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCerts => write!(f, "no certificates in PEM"),
            Self::NoKey => write!(f, "no private key in PEM"),
            Self::BadPem(e) => write!(f, "bad PEM: {e}"),
            Self::Sign(e) => write!(f, "unsupported signing key: {e}"),
        }
    }
}

impl std::error::Error for TlsError {}

/// Parse a certificate chain from PEM bytes.
///
/// # Errors
/// [`TlsError::NoCerts`] if empty, [`TlsError::BadPem`] on a malformed block.
pub fn load_certs(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let mut reader = std::io::BufReader::new(pem);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::BadPem(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError::NoCerts);
    }
    Ok(certs)
}

/// Parse a single private key from PEM bytes (PKCS#8 / SEC1 / PKCS#1).
///
/// # Errors
/// [`TlsError::NoKey`] if none found, [`TlsError::BadPem`] on a malformed block.
pub fn load_private_key(pem: &[u8]) -> Result<PrivateKeyDer<'static>, TlsError> {
    let mut reader = std::io::BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| TlsError::BadPem(e.to_string()))?
        .ok_or(TlsError::NoKey)
}

/// Build a rustls [`CertifiedKey`] (chain + signing key) from PEM material.
///
/// # Errors
/// Propagates parse errors and [`TlsError::Sign`] for an unusable key.
pub fn certified_key(cert_pem: &[u8], key_pem: &[u8]) -> Result<Arc<CertifiedKey>, TlsError> {
    let certs = load_certs(cert_pem)?;
    let key = load_private_key(key_pem)?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|e| TlsError::Sign(e.to_string()))?;
    Ok(Arc::new(CertifiedKey::new(certs, signing)))
}

/// Generate a self-signed certificate + key (PEM) for `hosts` — used for the
/// default fallback certificate and in tests.
///
/// # Errors
/// [`TlsError::BadPem`] if generation fails.
pub fn self_signed(hosts: &[String]) -> Result<(String, String), TlsError> {
    let ck = rcgen::generate_simple_self_signed(hosts.to_vec())
        .map_err(|e| TlsError::BadPem(e.to_string()))?;
    Ok((ck.cert.pem(), ck.key_pair.serialize_pem()))
}

/// An SNI certificate resolver: exact + wildcard host match over a default.
#[derive(Debug, Default)]
pub struct SniResolver {
    default: Option<Arc<CertifiedKey>>,
    by_host: Vec<(String, Arc<CertifiedKey>)>,
}

impl SniResolver {
    /// A resolver with an optional default (fallback) certificate.
    #[must_use]
    pub const fn new(default: Option<Arc<CertifiedKey>>) -> Self {
        Self { default, by_host: Vec::new() }
    }

    /// Register a certificate for an SNI host (`example.com` or `*.example.com`).
    pub fn insert(&mut self, host: &str, key: Arc<CertifiedKey>) {
        self.by_host.push((host.to_ascii_lowercase(), key));
    }

    /// Choose the certificate for an SNI name: exact match wins, then a matching
    /// wildcard, then the default.
    #[must_use]
    pub fn select_cert(&self, sni: Option<&str>) -> Option<Arc<CertifiedKey>> {
        if let Some(name) = sni {
            let name = name.to_ascii_lowercase();
            // Exact match first.
            if let Some((_, key)) = self.by_host.iter().find(|(h, _)| *h == name) {
                return Some(key.clone());
            }
            // Then a single-label wildcard (`*.suffix` matches `label.suffix`).
            if let Some((_, key)) = self
                .by_host
                .iter()
                .find(|(h, _)| wildcard_matches(h, &name))
            {
                return Some(key.clone());
            }
        }
        self.default.clone()
    }
}

/// Whether `pattern` (e.g. `*.example.com`) matches `host`, covering exactly one
/// left-most label (`a.example.com` yes, `example.com` and `a.b.example.com` no).
fn wildcard_matches(pattern: &str, host: &str) -> bool {
    let Some(suffix) = pattern.strip_prefix("*.") else {
        return false;
    };
    let Some(label_rest) = host.split_once('.') else {
        return false;
    };
    !label_rest.0.is_empty() && label_rest.1 == suffix
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.select_cert(client_hello.server_name())
    }
}

/// Build a rustls server configuration driven by `resolver`, advertising
/// HTTP/1.1 (and h2) via ALPN.
///
/// # Errors
/// [`TlsError::Sign`] if the protocol-version set is rejected by the provider.
pub fn server_config(resolver: Arc<SniResolver>) -> Result<ServerConfig, TlsError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TlsError::Sign(e.to_string()))?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert_for(host: &str) -> Arc<CertifiedKey> {
        let (cert_pem, key_pem) = self_signed(&[host.to_string()]).unwrap();
        certified_key(cert_pem.as_bytes(), key_pem.as_bytes()).unwrap()
    }

    #[test]
    fn self_signed_pem_loads_back() {
        let (cert_pem, key_pem) = self_signed(&["cave.local".to_string()]).unwrap();
        assert_eq!(load_certs(cert_pem.as_bytes()).unwrap().len(), 1);
        assert!(load_private_key(key_pem.as_bytes()).is_ok());
        assert!(certified_key(cert_pem.as_bytes(), key_pem.as_bytes()).is_ok());
    }

    #[test]
    fn empty_pem_is_rejected() {
        assert_eq!(load_certs(b""), Err(TlsError::NoCerts));
        assert_eq!(load_private_key(b""), Err(TlsError::NoKey));
    }

    #[test]
    fn exact_sni_match_wins() {
        let default = cert_for("default.local");
        let a = cert_for("a.example");
        let mut r = SniResolver::new(Some(default.clone()));
        r.insert("a.example", a.clone());
        assert!(Arc::ptr_eq(&r.select_cert(Some("a.example")).unwrap(), &a));
    }

    #[test]
    fn wildcard_sni_match() {
        let default = cert_for("default.local");
        let wild = cert_for("wild.example");
        let mut r = SniResolver::new(Some(default));
        r.insert("*.example.com", wild.clone());
        assert!(Arc::ptr_eq(&r.select_cert(Some("api.example.com")).unwrap(), &wild));
        // wildcard matches exactly one label, not the bare domain
        assert!(!Arc::ptr_eq(&r.select_cert(Some("example.com")).unwrap(), &wild));
    }

    #[test]
    fn unknown_sni_falls_back_to_default() {
        let default = cert_for("default.local");
        let r = SniResolver::new(Some(default.clone()));
        assert!(Arc::ptr_eq(&r.select_cert(Some("nope.example")).unwrap(), &default));
        assert!(Arc::ptr_eq(&r.select_cert(None).unwrap(), &default));
    }

    #[test]
    fn no_default_no_match_is_none() {
        let r = SniResolver::new(None);
        assert!(r.select_cert(Some("x")).is_none());
    }

    #[test]
    fn server_config_builds() {
        let r = Arc::new(SniResolver::new(Some(cert_for("cave.local"))));
        let cfg = server_config(r).unwrap();
        assert!(cfg.alpn_protocols.iter().any(|p| p == b"http/1.1"));
    }
}
