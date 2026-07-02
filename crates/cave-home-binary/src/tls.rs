// SPDX-License-Identifier: Apache-2.0
//! TLS termination for the apiserver listener (`tls` feature).
//!
//! Behavioural reference: the K3s/Kubernetes apiserver serves on `:6443` over
//! TLS, and authenticates clients by their certificate (mTLS) when a client CA
//! is configured. This module loads a PEM server certificate + key (and an
//! optional client-CA bundle) into a rustls [`ServerConfig`], from which
//! [`crate::server`] builds a [`tokio_rustls::TlsAcceptor`] and wraps each
//! accepted TCP connection before the HTTP handler ever sees bytes.
//!
//! The crypto provider is `ring` (the offline-buildable backend
//! cave-home-unifi already uses), installed explicitly so this works regardless
//! of any process-wide default provider state.
//!
//! Loading is pure file-I/O + parsing, so [`load_server_config`] is testable
//! against on-disk PEM (the tests generate a throwaway self-signed pair rather
//! than checking key material into the tree).

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tokio_rustls::TlsAcceptor;

/// Where to find the apiserver's TLS material on disk.
///
/// `client_ca` is optional: present → the listener requires and verifies a
/// client certificate (mTLS); absent → server-auth TLS only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsConfig {
    /// PEM file with the server certificate chain (leaf first).
    pub cert: PathBuf,
    /// PEM file with the server private key (PKCS#8, PKCS#1 or SEC1).
    pub key: PathBuf,
    /// Optional PEM bundle of CAs that client certificates must chain to. Its
    /// presence switches the listener into mutual-TLS mode.
    pub client_ca: Option<PathBuf>,
}

impl TlsConfig {
    /// A server-auth-only config (no client-certificate requirement).
    #[must_use]
    pub fn new(cert: impl Into<PathBuf>, key: impl Into<PathBuf>) -> Self {
        Self { cert: cert.into(), key: key.into(), client_ca: None }
    }

    /// Add a client-CA bundle, switching the listener into mutual-TLS mode.
    #[must_use]
    pub fn with_client_ca(mut self, ca: impl Into<PathBuf>) -> Self {
        self.client_ca = Some(ca.into());
        self
    }

    /// Whether this config requests mutual TLS (a client CA is configured).
    #[must_use]
    pub const fn is_mutual(&self) -> bool {
        self.client_ca.is_some()
    }
}

/// Why building the TLS server config failed.
#[derive(Debug)]
pub enum TlsError {
    /// A PEM file could not be opened or read.
    Io(PathBuf, std::io::Error),
    /// The certificate file held no certificates.
    NoCertificates(PathBuf),
    /// The key file held no usable private key.
    NoPrivateKey(PathBuf),
    /// The client-CA bundle held no usable anchors.
    NoClientCaAnchors(PathBuf),
    /// rustls rejected the assembled configuration.
    Rustls(String),
}

impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "reading {}: {e}", p.display()),
            Self::NoCertificates(p) => write!(f, "no certificates in {}", p.display()),
            Self::NoPrivateKey(p) => write!(f, "no private key in {}", p.display()),
            Self::NoClientCaAnchors(p) => write!(f, "no usable CA certificates in {}", p.display()),
            Self::Rustls(e) => write!(f, "rustls configuration error: {e}"),
        }
    }
}

impl std::error::Error for TlsError {}

impl From<TlsError> for std::io::Error {
    fn from(e: TlsError) -> Self {
        Self::new(std::io::ErrorKind::InvalidInput, e.to_string())
    }
}

/// Open `path` as a buffered reader, mapping I/O failure onto [`TlsError::Io`].
fn open(path: &Path) -> Result<BufReader<File>, TlsError> {
    File::open(path).map(BufReader::new).map_err(|e| TlsError::Io(path.to_path_buf(), e))
}

/// Read the certificate chain from a PEM file (leaf first).
fn load_certs(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, TlsError> {
    let mut reader = open(path)?;
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<_, _>>()
        .map_err(|e| TlsError::Io(path.to_path_buf(), e))?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificates(path.to_path_buf()));
    }
    Ok(certs)
}

/// Read the single private key from a PEM file.
fn load_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>, TlsError> {
    let mut reader = open(path)?;
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| TlsError::Io(path.to_path_buf(), e))?
        .ok_or_else(|| TlsError::NoPrivateKey(path.to_path_buf()))
}

/// Build a [`RootCertStore`] of client-CA anchors from a PEM bundle (for mTLS).
fn load_client_roots(path: &Path) -> Result<RootCertStore, TlsError> {
    let anchors = load_certs(path)?;
    let mut roots = RootCertStore::empty();
    let (added, _ignored) = roots.add_parsable_certificates(anchors);
    if added == 0 {
        return Err(TlsError::NoClientCaAnchors(path.to_path_buf()));
    }
    Ok(roots)
}

/// Build the rustls [`ServerConfig`] for the apiserver listener from `cfg`.
///
/// Server-auth TLS always; mutual TLS additionally when `cfg.client_ca` is set
/// (the listener then requires and verifies a client certificate).
///
/// # Errors
/// [`TlsError`] if any PEM file is missing/empty/unparsable or rustls rejects
/// the assembled configuration.
pub fn load_server_config(cfg: &TlsConfig) -> Result<Arc<ServerConfig>, TlsError> {
    let certs = load_certs(&cfg.cert)?;
    let key = load_key(&cfg.key)?;

    // Pin the crypto provider to ring so this is independent of any process
    // default-provider installation order.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TlsError::Rustls(e.to_string()))?;

    let config = if let Some(ca) = &cfg.client_ca {
        let roots = load_client_roots(ca)?;
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| TlsError::Rustls(e.to_string()))?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| TlsError::Rustls(e.to_string()))?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| TlsError::Rustls(e.to_string()))?
    };

    Ok(Arc::new(config))
}

/// Build a [`TlsAcceptor`] for the apiserver listener from `cfg`.
///
/// # Errors
/// Propagates [`load_server_config`] failures.
pub fn acceptor(cfg: &TlsConfig) -> Result<TlsAcceptor, TlsError> {
    Ok(TlsAcceptor::from(load_server_config(cfg)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a throwaway self-signed cert+key pair and write them to two
    /// temp files; return the paths (and the temp dir, kept alive by the caller).
    fn self_signed(dir: &Path, sans: &[&str]) -> (PathBuf, PathBuf) {
        let subjects: Vec<String> = sans.iter().map(|s| (*s).to_string()).collect();
        let key = rcgen::generate_simple_self_signed(subjects).expect("gen self-signed");
        let cert_path = dir.join("tls.crt");
        let key_path = dir.join("tls.key");
        std::fs::write(&cert_path, key.cert.pem()).expect("write cert");
        std::fs::write(&key_path, key.key_pair.serialize_pem()).expect("write key");
        (cert_path, key_path)
    }

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cavehome-tls-{}", std::process::id()));
        let d = d.join(format!("{:?}", std::time::SystemTime::now()));
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn tlsconfig_tracks_mutual_mode() {
        let c = TlsConfig::new("/c", "/k");
        assert!(!c.is_mutual());
        let m = c.with_client_ca("/ca");
        assert!(m.is_mutual());
        assert_eq!(m.client_ca.as_deref(), Some(Path::new("/ca")));
    }

    #[test]
    fn loads_a_server_config_from_self_signed_pem() {
        let dir = tmpdir();
        let (cert, key) = self_signed(&dir, &["localhost"]);
        let cfg = TlsConfig::new(cert, key);
        assert!(!cfg.is_mutual());
        // A server-auth config builds from a valid leaf cert + key.
        load_server_config(&cfg).expect("server-auth config builds");
    }

    #[test]
    fn loads_a_mutual_tls_config_with_a_client_ca() {
        let dir = tmpdir();
        let (cert, key) = self_signed(&dir, &["localhost"]);
        // Reuse the same self-signed cert as a (degenerate) client CA bundle —
        // it parses into one anchor, which is all the verifier needs to build.
        // The mTLS branch of load_server_config (client-cert verifier) must build.
        let cfg = TlsConfig::new(cert.clone(), key).with_client_ca(cert);
        assert!(cfg.is_mutual());
        load_server_config(&cfg).expect("mTLS config builds with a client-cert verifier");
    }

    #[test]
    fn missing_cert_file_is_an_io_error() {
        let dir = tmpdir();
        let (_cert, key) = self_signed(&dir, &["localhost"]);
        let cfg = TlsConfig::new(dir.join("does-not-exist.crt"), key);
        let err = load_server_config(&cfg).expect_err("missing cert must error");
        assert!(matches!(err, TlsError::Io(_, _)), "got {err:?}");
    }

    #[test]
    fn empty_cert_file_reports_no_certificates() {
        let dir = tmpdir();
        let (_cert, key) = self_signed(&dir, &["localhost"]);
        let empty = dir.join("empty.crt");
        std::fs::write(&empty, b"# no certs here\n").expect("write");
        let cfg = TlsConfig::new(empty.clone(), key);
        let err = load_server_config(&cfg).expect_err("empty cert must error");
        assert!(matches!(&err, TlsError::NoCertificates(p) if *p == empty), "got {err:?}");
    }

    #[test]
    fn an_acceptor_is_constructible_from_a_valid_config() {
        let dir = tmpdir();
        let (cert, key) = self_signed(&dir, &["localhost"]);
        let cfg = TlsConfig::new(cert, key);
        assert!(acceptor(&cfg).is_ok(), "acceptor builds from valid material");
    }
}
