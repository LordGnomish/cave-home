// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Test-only helpers (feature `test-support`).
//!
//! rcgen certificates and a tiny blocking rustls server on `127.0.0.1` that records the application bytes
//! each connection delivered. Shared by the integration tests of this crate,
//! `cave-home-unifi` and `cave-home-hue` so they all exercise real handshakes
//! offline. Never enabled in a production build.
#![allow(clippy::expect_used, clippy::missing_panics_doc)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A certificate and the private key that goes with it.
#[derive(Debug)]
pub struct TestCert {
    /// The DER certificate.
    pub cert: CertificateDer<'static>,
    /// The PKCS#8 private key.
    pub key: PrivateKeyDer<'static>,
}

fn key_der(key: &KeyPair) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()))
}

/// A fresh self-signed certificate for `names` (as a `UniFi` console ships).
#[must_use]
pub fn self_signed(names: &[&str]) -> TestCert {
    let key = KeyPair::generate().expect("keygen");
    let names: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
    let cert = CertificateParams::new(names)
        .expect("params")
        .self_signed(&key)
        .expect("self-sign");
    TestCert {
        cert: cert.der().clone(),
        key: key_der(&key),
    }
}

/// Two distinct self-signed certificates sharing one key pair (a console
/// that re-issued its certificate without rotating the key).
#[must_use]
pub fn self_signed_same_key(first: &[&str], second: &[&str]) -> (TestCert, TestCert) {
    let key = KeyPair::generate().expect("keygen");
    let make = |names: &[&str]| {
        let names: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
        let cert = CertificateParams::new(names)
            .expect("params")
            .self_signed(&key)
            .expect("self-sign");
        TestCert {
            cert: cert.der().clone(),
            key: key_der(&key),
        }
    };
    (make(first), make(second))
}

/// A private certificate authority (the Hue bridge root model).
pub struct TestCa {
    cert: rcgen::Certificate,
    key: KeyPair,
}

impl std::fmt::Debug for TestCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestCa").finish_non_exhaustive()
    }
}

impl TestCa {
    /// A new self-signed CA whose subject CN is `common_name`.
    #[must_use]
    pub fn new(common_name: &str) -> Self {
        let key = KeyPair::generate().expect("keygen");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::DigitalSignature];
        let cert = params.self_signed(&key).expect("self-sign CA");
        Self { cert, key }
    }

    /// The CA certificate (DER), to configure as a trust root.
    #[must_use]
    pub fn der(&self) -> CertificateDer<'static> {
        self.cert.der().clone()
    }

    /// Issue a leaf whose subject CN is `common_name` and with `sans` as DNS
    /// subject-alternative names (Hue bridge certs carry none).
    #[must_use]
    pub fn issue_leaf(&self, common_name: &str, sans: &[&str]) -> TestCert {
        let key = KeyPair::generate().expect("keygen");
        let sans: Vec<String> = sans.iter().map(|s| (*s).to_owned()).collect();
        let mut params = CertificateParams::new(sans).expect("params");
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        params.distinguished_name = dn;
        let cert = params.signed_by(&key, &self.cert, &self.key).expect("sign leaf");
        TestCert {
            cert: cert.der().clone(),
            key: key_der(&key),
        }
    }
}

/// A blocking TLS server on `127.0.0.1:<ephemeral>`. Each connection gets one
/// response; the application bytes it sent before that are recorded. A
/// connection whose handshake fails records nothing.
pub struct TlsTestServer {
    addr: SocketAddr,
    received: Arc<Mutex<Vec<String>>>,
    accepted: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
}

impl std::fmt::Debug for TlsTestServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsTestServer").field("addr", &self.addr).finish_non_exhaustive()
    }
}

impl TlsTestServer {
    /// Start serving `chain` / `key`, answering every request with `response`.
    #[must_use]
    pub fn start(chain: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>, response: &[u8]) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("server cert");
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let received = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let response = response.to_vec();
        {
            let received = Arc::clone(&received);
            let accepted = Arc::clone(&accepted);
            let finished = Arc::clone(&finished);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    accepted.fetch_add(1, Ordering::SeqCst);
                    let config = Arc::clone(&config);
                    let received = Arc::clone(&received);
                    let finished = Arc::clone(&finished);
                    let response = response.clone();
                    std::thread::spawn(move || {
                        serve(&config, stream, &received, &response);
                        finished.fetch_add(1, Ordering::SeqCst);
                    });
                }
            });
        }
        Self {
            addr,
            received,
            accepted,
            finished,
        }
    }

    /// The bound address.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `host:port` of the bound address.
    #[must_use]
    pub fn authority(&self) -> String {
        self.addr.to_string()
    }

    /// The application bytes (lossy UTF-8) each completed connection sent.
    #[must_use]
    pub fn received(&self) -> Vec<String> {
        self.received.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Block (up to 5 s) until at least one connection was accepted and every
    /// accepted connection has been fully handled, so [`Self::received`] is
    /// stable.
    pub fn wait_idle(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let a = self.accepted.load(Ordering::SeqCst);
            if a > 0 && a == self.finished.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn serve(config: &Arc<rustls::ServerConfig>, tcp: TcpStream, received: &Mutex<Vec<String>>, response: &[u8]) {
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(conn) = rustls::ServerConnection::new(Arc::clone(config)) else {
        return;
    };
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if request_complete(&buf) {
                    break;
                }
            }
        }
    }
    if buf.is_empty() {
        return;
    }
    received
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(String::from_utf8_lossy(&buf).into_owned());
    let _ = tls.write_all(response);
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

/// Whether `buf` holds a full HTTP/1.1 request head plus its
/// `Content-Length` body.
fn request_complete(buf: &[u8]) -> bool {
    let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
    let len = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    buf.len() >= head_end + 4 + len
}
