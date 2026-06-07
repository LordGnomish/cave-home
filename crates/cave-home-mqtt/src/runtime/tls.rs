//! TLS acceptor construction (rustls over the `ring` crypto provider).

use std::io::{self, BufReader, ErrorKind};
use std::sync::Arc;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// Build a [`TlsAcceptor`] from a PEM certificate chain and private key.
///
/// # Errors
/// Fails if the PEM is malformed, carries no certificate or key, or the
/// resulting rustls configuration is invalid.
pub fn acceptor_from_pem(cert_pem: &str, key_pem: &str) -> io::Result<TlsAcceptor> {
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut BufReader::new(cert_pem.as_bytes()))
            .collect::<Result<_, _>>()?;
    if certs.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "no certificates in PEM"));
    }
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_pem.as_bytes()))?
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "no private key in PEM"))?;

    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e.to_string()))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
