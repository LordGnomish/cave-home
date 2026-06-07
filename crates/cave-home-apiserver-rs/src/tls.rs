// SPDX-License-Identifier: Apache-2.0
//! TLS / mTLS termination in front of the std socket loop (feature `tls`).
//!
//! Behavioural reference: the apiserver serves HTTPS and, with
//! `--client-ca-file`, authenticates clients by their TLS certificate
//! (`kube-apiserver` x509 client auth). This module is the *only* place rustls
//! enters the crate; everything below it stays std-only. Because
//! [`crate::server::serve_stream`]'s request/response handling is generic over
//! the byte stream, TLS slots in by wrapping the accepted `TcpStream` in a
//! rustls [`StreamOwned`] before the same `read_request` → `handle` →
//! `write_all` flow runs over the now-encrypted channel.
//!
//! For mTLS, once the handshake verifies the client certificate against the
//! configured CA, the terminator extracts the certificate subject
//! ([`crate::x509`]) and injects it as the `X-Remote-User` / `X-Remote-Group`
//! headers that [`crate::authn::RequestHeaderAuthenticator`] reads — first
//! **stripping** any client-supplied copies so the identity cannot be spoofed.
//!
//! The crypto provider is pinned to `ring`, so the build needs no C toolchain.

use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError};

use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};

use crate::handler::ApiServer;
use crate::http::Request;
use crate::server::read_request;

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// The `ring` crypto provider this crate builds every rustls config with.
#[must_use]
pub fn ring_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Parse every PEM `CERTIFICATE` block from `pem`.
///
/// # Errors
/// [`io::ErrorKind::InvalidData`] if a block is malformed.
pub fn load_certs(pem: &[u8]) -> io::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_io)
}

/// Parse the first PEM private key (PKCS#8, PKCS#1, or SEC1) from `pem`.
///
/// # Errors
/// [`io::ErrorKind::InvalidData`] if no key is present or it is malformed.
pub fn load_private_key(pem: &[u8]) -> io::Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key in PEM"))
}

/// Build a one-way TLS server config (no client authentication) from a server
/// certificate chain and its private key (both PEM).
///
/// # Errors
/// Propagates PEM/key parse errors and any rustls configuration error.
pub fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<Arc<ServerConfig>> {
    let certs = load_certs(cert_pem)?;
    let key = load_private_key(key_pem)?;
    let config = ServerConfig::builder_with_provider(ring_provider())
        .with_safe_default_protocol_versions()
        .map_err(to_io)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(to_io)?;
    Ok(Arc::new(config))
}

/// Build a mutual-TLS server config: the server presents `cert_pem`/`key_pem`
/// and *requires* a client certificate that chains to a CA in `client_ca_pem`.
///
/// # Errors
/// Propagates PEM/key parse errors, an empty/invalid CA set, or any rustls
/// configuration error.
pub fn server_config_mtls(
    cert_pem: &[u8],
    key_pem: &[u8],
    client_ca_pem: &[u8],
) -> io::Result<Arc<ServerConfig>> {
    let mut roots = RootCertStore::empty();
    for ca in load_certs(client_ca_pem)? {
        roots.add(ca).map_err(to_io)?;
    }
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), ring_provider())
        .build()
        .map_err(to_io)?;
    let config = ServerConfig::builder_with_provider(ring_provider())
        .with_safe_default_protocol_versions()
        .map_err(to_io)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(load_certs(cert_pem)?, load_private_key(key_pem)?)
        .map_err(to_io)?;
    Ok(Arc::new(config))
}

/// Strip any client-supplied front-proxy headers, then — when the handshake
/// produced a verified client certificate — inject the subject identity as
/// `X-Remote-User` (CN) and repeatable `X-Remote-Group` (each O).
fn inject_client_identity(req: &mut Request, peer: Option<&[CertificateDer<'_>]>) {
    // Always strip first: a value on the wire must never be trusted.
    req.headers.remove_all("x-remote-user");
    req.headers.remove_all("x-remote-group");
    if let Some(leaf) = peer.and_then(<[_]>::first) {
        if let Some(identity) = crate::x509::subject_identity(leaf.as_ref()) {
            req.headers.insert("x-remote-user", identity.name);
            for group in identity.groups {
                req.headers.insert("x-remote-group", group);
            }
        }
    }
}

/// Terminate TLS on `tcp`, then serve exactly one request through the apiserver
/// over the encrypted stream (mirroring [`crate::server::serve_stream`]). A
/// verified client certificate is turned into the front-proxy identity headers.
///
/// # Errors
/// Handshake or per-connection I/O errors.
pub fn serve_tls_stream<S: Read + Write>(
    tcp: S,
    config: &Arc<ServerConfig>,
    app: &Mutex<ApiServer>,
) -> io::Result<()> {
    let conn = ServerConnection::new(config.clone()).map_err(to_io)?;
    let mut tls = StreamOwned::new(conn, tcp);

    // read_request drives the handshake (rustls reads transparently), so the
    // peer certificate is available immediately afterwards.
    let Some(mut req) = read_request(&mut tls)? else {
        return Ok(());
    };
    inject_client_identity(&mut req, tls.conn.peer_certificates());

    let mut resp = {
        let mut guard = app.lock().unwrap_or_else(PoisonError::into_inner);
        guard.handle(&req)
    };
    resp.headers.insert("connection", "close");
    tls.write_all(&resp.to_bytes())?;
    tls.flush()
}

/// A blocking, thread-per-connection HTTPS server: the TLS-terminating sibling
/// of [`crate::server::Server`].
pub struct TlsServer {
    listener: TcpListener,
    app: Arc<Mutex<ApiServer>>,
    config: Arc<ServerConfig>,
}

impl TlsServer {
    /// Bind to `addr` (port `0` for ephemeral) with the shared apiserver and a
    /// rustls server config (from [`server_config`] / [`server_config_mtls`]).
    ///
    /// # Errors
    /// Any bind error from the OS.
    pub fn bind(
        addr: impl ToSocketAddrs,
        app: Arc<Mutex<ApiServer>>,
        config: Arc<ServerConfig>,
    ) -> io::Result<Self> {
        Ok(Self { listener: TcpListener::bind(addr)?, app, config })
    }

    /// The bound local address (resolves the ephemeral port).
    ///
    /// # Errors
    /// If the socket address cannot be read.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accept and fully serve a single TLS connection (one request).
    ///
    /// # Errors
    /// Accept, handshake, or per-connection I/O errors.
    pub fn serve_once(&self) -> io::Result<()> {
        let (tcp, _peer) = self.listener.accept()?;
        serve_tls_stream(tcp, &self.config, &self.app)
    }

    /// Run the accept loop forever. Per-connection errors (including handshake
    /// failures, e.g. a client that presents no certificate to an mTLS server)
    /// are swallowed so one bad client cannot stop the server.
    ///
    /// # Errors
    /// A fatal accept error.
    pub fn run(&self) -> io::Result<()> {
        for conn in self.listener.incoming() {
            match conn {
                Ok(tcp) => {
                    let _ = serve_tls_stream(tcp, &self.config, &self.app);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::MemoryAuditSink;
    use crate::authn::{AuthenticatorChain, RequestHeaderAuthenticator};
    use crate::handler::ApiServer;
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection};
    use std::net::TcpStream;

    const CA_CERT: &[u8] = include_bytes!("../tests/fixtures/ca.crt");
    const SERVER_CERT: &[u8] = include_bytes!("../tests/fixtures/server.crt");
    const SERVER_KEY: &[u8] = include_bytes!("../tests/fixtures/server.key");
    const CLIENT_CERT: &[u8] = include_bytes!("../tests/fixtures/client.crt");
    const CLIENT_KEY: &[u8] = include_bytes!("../tests/fixtures/client.key");

    /// A client config trusting our test CA (no client cert).
    fn client_config_plain() -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        for ca in load_certs(CA_CERT).expect("ca") {
            roots.add(ca).expect("add ca");
        }
        Arc::new(
            ClientConfig::builder_with_provider(ring_provider())
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    }

    /// A client config that also presents the `alice` client certificate (mTLS).
    fn client_config_mtls() -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        for ca in load_certs(CA_CERT).expect("ca") {
            roots.add(ca).expect("add ca");
        }
        Arc::new(
            ClientConfig::builder_with_provider(ring_provider())
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_root_certificates(roots)
                .with_client_auth_cert(load_certs(CLIENT_CERT).expect("cert"), load_private_key(CLIENT_KEY).expect("key"))
                .expect("client auth"),
        )
    }

    /// Open one TLS connection to `addr` with `config`, send `raw`, and read the
    /// whole plaintext response (tolerating an unclean TCP close at EOF).
    fn tls_call(addr: SocketAddr, config: Arc<ClientConfig>, raw: &str) -> String {
        let server_name = ServerName::try_from("127.0.0.1").expect("server name");
        let conn = ClientConnection::new(config, server_name).expect("client conn");
        let sock = TcpStream::connect(addr).expect("connect");
        let mut tls = StreamOwned::new(conn, sock);
        tls.write_all(raw.as_bytes()).expect("write");
        tls.flush().expect("flush");
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                // A `close`-without-close_notify surfaces as an error after the
                // body has been delivered; treat it as end of stream.
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n")
    }

    #[test]
    fn loads_server_cert_and_key() {
        assert_eq!(load_certs(SERVER_CERT).expect("certs").len(), 1);
        load_private_key(SERVER_KEY).expect("key");
        // A PEM with no key is a clean error, not a panic.
        assert!(load_private_key(CA_CERT).is_err());
    }

    #[test]
    fn tls_handshake_and_request_roundtrip() {
        let app = Arc::new(Mutex::new(ApiServer::new()));
        let config = server_config(SERVER_CERT, SERVER_KEY).expect("server config");
        let server = TlsServer::bind("127.0.0.1:0", app, config).expect("bind");
        let addr = server.local_addr().expect("addr");

        let worker = std::thread::spawn(move || server.serve_once().expect("serve_once"));
        let resp = tls_call(addr, client_config_plain(), &get("/healthz"));
        worker.join().expect("worker");

        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "resp: {resp}");
        assert!(resp.ends_with("ok"), "resp: {resp}");
    }

    #[test]
    fn mtls_client_cert_becomes_the_authenticated_identity() {
        // The apiserver trusts ONLY the front-proxy headers the terminator
        // injects from the verified cert; anonymous is disabled.
        let audit = Arc::new(MemoryAuditSink::new());
        let app = ApiServer::new()
            .with_authn(
                AuthenticatorChain::new()
                    .with(Box::new(RequestHeaderAuthenticator::new()))
                    .allow_anonymous(false),
            )
            .with_audit(audit.clone());
        let app = Arc::new(Mutex::new(app));

        let config = server_config_mtls(SERVER_CERT, SERVER_KEY, CA_CERT).expect("mtls config");
        let server = TlsServer::bind("127.0.0.1:0", app, config).expect("bind");
        let addr = server.local_addr().expect("addr");

        let worker = std::thread::spawn(move || server.serve_once().expect("serve_once"));
        let resp = tls_call(addr, client_config_mtls(), &get("/api/v1/namespaces/default/pods"));
        worker.join().expect("worker");

        // The list succeeds (authenticated) and the recorded identity is the
        // client certificate subject CN, not system:anonymous.
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "resp: {resp}");
        let events = audit.events();
        let user = events.last().expect("an audit event");
        assert_eq!(user.user, "alice");
    }

    #[test]
    fn mtls_rejects_client_without_certificate() {
        // A plain (cert-less) client must fail the handshake against an mTLS
        // server, so the server-side serve_once returns an error rather than a
        // 200. We assert the connection yields no valid HTTP response.
        let app = Arc::new(Mutex::new(ApiServer::new()));
        let config = server_config_mtls(SERVER_CERT, SERVER_KEY, CA_CERT).expect("mtls config");
        let server = TlsServer::bind("127.0.0.1:0", app, config).expect("bind");
        let addr = server.local_addr().expect("addr");

        let worker = std::thread::spawn(move || {
            // The handshake fails (no client cert): serve_once surfaces an error.
            server.serve_once().expect_err("handshake without client cert must fail")
        });
        let resp = tls_call(addr, client_config_plain(), &get("/healthz"));
        worker.join().expect("worker");
        assert!(!resp.contains("200 OK"), "cert-less client must not be served: {resp}");
    }
}
