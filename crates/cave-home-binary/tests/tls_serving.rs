// SPDX-License-Identifier: Apache-2.0
#![cfg(feature = "tls")]
#![allow(clippy::expect_used)] // test harness: a failed expect is the test failing
//! Live TLS serving tests: boot the runtime with in-test generated certificates
//! and prove the apiserver answers over a real rustls client connection — and
//! that TLS never replaces authentication.
//!
//! This goes the whole way: a throwaway CA, a server certificate and client
//! certificates are generated in-test, written to disk, and handed to the
//! runtime via `RuntimeConfig::tls`. The runtime terminates TLS on its accept
//! loop; real `tokio-rustls` clients (trusting that CA) complete the handshake
//! and get the apiserver's plaintext-inside-TLS HTTP response. A verified
//! client certificate authenticates as its subject (CN → user, O → groups);
//! without one, a bearer token is still required. No fixture key material is
//! checked in.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cave_home_apiserver_rs::rbac::UserInfo;
use cave_home_binary::access::ADMIN_GROUP;
use cave_home_binary::node::LocalNode;
use cave_home_binary::server::{run_until_on_listener, AccessConfig, Limits, RuntimeConfig};
use cave_home_binary::tls::TlsConfig;
use cave_home_orchestration::role::NodeIntent;
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_rustls::TlsConnector;

const TOKEN: &str = "tls-admin-token";

/// A throwaway PKI: a CA, a `localhost` server leaf, and a way to mint client
/// leaves — all signed by that CA and written under `dir`.
struct Pki {
    dir: PathBuf,
    ca: rcgen::Certificate,
    ca_key: KeyPair,
}

impl Pki {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cavehome-tls-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let ca_key = KeyPair::generate().expect("ca key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
        params.distinguished_name.push(DnType::CommonName, "cave-home test CA");
        let ca = params.self_signed(&ca_key).expect("ca cert");
        std::fs::write(dir.join("ca.crt"), ca.pem()).expect("write ca");
        Self { dir, ca, ca_key }
    }

    fn ca_path(&self) -> PathBuf {
        self.dir.join("ca.crt")
    }

    fn ca_der(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.ca.der().to_vec())
    }

    /// The server leaf for `localhost`; returns `(cert path, key path)`.
    fn server(&self) -> (PathBuf, PathBuf) {
        let key = KeyPair::generate().expect("server key");
        let params = CertificateParams::new(vec!["localhost".to_string()]).expect("server params");
        let cert = params.signed_by(&key, &self.ca, &self.ca_key).expect("server cert");
        let (cert_path, key_path) = (self.dir.join("server.crt"), self.dir.join("server.key"));
        std::fs::write(&cert_path, cert.pem()).expect("write cert");
        std::fs::write(&key_path, key.serialize_pem()).expect("write key");
        (cert_path, key_path)
    }

    /// A client leaf with subject `CN=<user>, O=<group>…`.
    fn client(&self, user: &str, groups: &[&str]) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        let key = KeyPair::generate().expect("client key");
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("client params");
        params.distinguished_name.push(DnType::CommonName, user);
        for g in groups {
            params.distinguished_name.push(DnType::OrganizationName, *g);
        }
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let cert = params.signed_by(&key, &self.ca, &self.ca_key).expect("client cert");
        (
            vec![CertificateDer::from(cert.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
    }
}

fn roots(ca: CertificateDer<'static>) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.add(ca).expect("add trust anchor");
    roots
}

/// A rustls client trusting `ca`, optionally presenting a client certificate.
/// The crypto provider is pinned to `ring` so the test does not depend on a
/// process-wide default provider being installed.
fn connector(ca: CertificateDer<'static>, client: Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>) -> TlsConnector {
    let builder = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("default protocol versions")
        .with_root_certificates(roots(ca));
    let config = match client {
        Some((chain, key)) => builder.with_client_auth_cert(chain, key).expect("client auth"),
        None => builder.with_no_client_auth(),
    };
    TlsConnector::from(Arc::new(config))
}

/// Boot the runtime over TLS (mutual TLS when `client_ca` is set) on an
/// ephemeral loopback port, accepting [`TOKEN`] as an admin.
async fn boot(cert: &Path, key: &Path, client_ca: Option<&Path>) -> (std::net::SocketAddr, watch::Sender<bool>, tokio::task::JoinHandle<std::io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut tls = TlsConfig::new(cert, key);
    if let Some(ca) = client_ca {
        tls = tls.with_client_ca(ca);
    }
    let cfg = RuntimeConfig {
        intent: NodeIntent::PrimaryHub,
        node: LocalNode::new("hub-01", "127.0.0.1"),
        bind_addr: "127.0.0.1".to_string(),
        bind_port: addr.port(),
        reconcile_interval: Duration::from_millis(50),
        tls: Some(tls),
        access: AccessConfig {
            tokens: vec![(TOKEN.to_string(), UserInfo::new("tls-admin").with_groups(&[ADMIN_GROUP]))],
            ..AccessConfig::default()
        },
        limits: Limits::default(),
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    let shutdown = async move {
        let mut rx = stop_rx;
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    };
    let server = tokio::spawn(run_until_on_listener(cfg, listener, shutdown));
    (addr, stop_tx, server)
}

/// One TLS round trip; retries the connect until the listener is up. Returns
/// the full response text, or `None` if the handshake was refused.
async fn tls_exchange(addr: std::net::SocketAddr, connector: &TlsConnector, raw: &str) -> Option<String> {
    let domain = ServerName::try_from("localhost").expect("server name");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(tcp) = TcpStream::connect(addr).await {
            return match connector.connect(domain.clone(), tcp).await {
                Ok(mut tls) => {
                    tls.write_all(raw.as_bytes()).await.ok()?;
                    let mut buf = Vec::new();
                    let _ = tls.read_to_end(&mut buf).await;
                    Some(String::from_utf8_lossy(&buf).into_owned())
                }
                Err(_) => None,
            };
        }
        assert!(tokio::time::Instant::now() < deadline, "listener never came up");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn get(path: &str, token: Option<&str>) -> String {
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apiserver_serves_over_real_tls() {
    let pki = Pki::new("serve");
    let (cert, key) = pki.server();
    let (addr, stop, server) = boot(&cert, &key, None).await;
    let client = connector(pki.ca_der(), None);

    // A plain-HTTP server would fail the handshake, so a 200 here proves the
    // TLS terminator is really in the path.
    let health = tls_exchange(addr, &client, &get("/healthz", None)).await.expect("handshake");
    assert!(health.contains("200"), "200 over TLS: {health}");
    assert!(health.contains("ok"), "healthz body over TLS: {health}");

    // The write path works over TLS too: create a Pod, read it back encrypted.
    let pod = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"tls-pod"},"spec":{"containers":[{"name":"c","image":"nginx"}]}}"#;
    let post = format!(
        "POST /api/v1/namespaces/default/pods HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        pod.len(),
        pod
    );
    let resp = tls_exchange(addr, &client, &post).await.expect("handshake");
    assert!(resp.contains("201"), "create over TLS returns 201: {resp}");
    assert!(resp.contains("tls-pod"), "{resp}");

    let _ = stop.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await.expect("shuts down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_does_not_replace_authentication() {
    let pki = Pki::new("noauth");
    let (cert, key) = pki.server();
    let (addr, stop, server) = boot(&cert, &key, None).await;
    let client = connector(pki.ca_der(), None);
    let resp = tls_exchange(addr, &client, &get("/api/v1/pods", None)).await.expect("handshake");
    assert!(resp.starts_with("HTTP/1.1 401"), "encrypted but anonymous is still refused: {resp}");
    let _ = stop.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_certificates_authenticate_as_their_subject() {
    let pki = Pki::new("mtls");
    let (cert, key) = pki.server();
    let (addr, stop, server) = boot(&cert, &key, Some(&pki.ca_path())).await;

    // CN=alice, O=system:masters → cluster admin, no token needed.
    let admin = connector(pki.ca_der(), Some(pki.client("alice", &[ADMIN_GROUP])));
    let resp = tls_exchange(addr, &admin, &get("/api/v1/pods", None)).await.expect("mTLS handshake");
    assert!(resp.starts_with("HTTP/1.1 200"), "admin client cert: {resp}");

    // A verified certificate without an RBAC grant authenticates but is denied.
    let nobody = connector(pki.ca_der(), Some(pki.client("mallory", &[])));
    let resp = tls_exchange(addr, &nobody, &get("/api/v1/pods", None)).await.expect("mTLS handshake");
    assert!(resp.starts_with("HTTP/1.1 403"), "authenticated but unauthorized: {resp}");

    // Identity headers on the wire never override the certificate.
    let spoof = "GET /api/v1/pods HTTP/1.1\r\nHost: localhost\r\nX-Remote-User: system:admin\r\nX-Remote-Group: system:masters\r\nConnection: close\r\n\r\n";
    let resp = tls_exchange(addr, &nobody, spoof).await.expect("mTLS handshake");
    assert!(resp.starts_with("HTTP/1.1 403"), "spoofed X-Remote-* must be ignored: {resp}");

    let _ = stop.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tokens_still_work_when_a_client_ca_is_configured() {
    let pki = Pki::new("mtls-token");
    let (cert, key) = pki.server();
    let (addr, stop, server) = boot(&cert, &key, Some(&pki.ca_path())).await;
    let no_cert = connector(pki.ca_der(), None);
    let resp = tls_exchange(addr, &no_cert, &get("/api/v1/pods", Some(TOKEN)))
        .await
        .expect("a client CA makes certificates optional, as upstream --client-ca-file does");
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    let resp = tls_exchange(addr, &no_cert, &get("/api/v1/pods", None)).await.expect("handshake");
    assert!(resp.starts_with("HTTP/1.1 401"), "neither certificate nor token: {resp}");
    let _ = stop.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

/// The scalar value of `key: value` on the first line of `yaml` that has it
/// (plain, or JSON-quoted as the kubeconfig writer quotes unusual paths).
fn yaml_field(yaml: &str, key: &str) -> Option<String> {
    let raw = yaml.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':').map(str::trim))?;
    if raw.starts_with('"') {
        return cave_home_apiserver_rs::json::Value::parse(raw).ok()?.as_str().map(str::to_string);
    }
    Some(raw.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_boot_serves_https_with_a_generated_ca() {
    // No certificate configured: like k3s, the boot generates its own CA and
    // serving certificate, so kubectl (which never sends credentials over plain
    // HTTP) can use the generated kubeconfig as-is.
    let dir = std::env::temp_dir().join(format!(
        "cavehome-tls-bootstrap-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let cfg = RuntimeConfig {
        intent: NodeIntent::PrimaryHub,
        node: LocalNode::new("hub-01", "127.0.0.1"),
        bind_addr: "127.0.0.1".to_string(),
        bind_port: addr.port(),
        reconcile_interval: Duration::from_millis(50),
        tls: None,
        access: AccessConfig { bootstrap_dir: Some(dir.clone()), ..AccessConfig::default() },
        limits: Limits::default(),
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    let shutdown = async move {
        let mut rx = stop_rx;
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    };
    let server = tokio::spawn(run_until_on_listener(cfg, listener, shutdown));

    let kubeconfig_path = dir.join("kubeconfig.yaml");
    let kubeconfig = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&kubeconfig_path) {
                if text.contains("current-context") {
                    return text;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("kubeconfig written at boot");
    assert_eq!(yaml_field(&kubeconfig, "server"), Some(format!("https://127.0.0.1:{}", addr.port())), "{kubeconfig}");
    let ca_path = yaml_field(&kubeconfig, "certificate-authority").expect("kubeconfig trusts the generated CA");
    let token = yaml_field(&kubeconfig, "token").expect("kubeconfig carries the admin token");

    let ca_pem = std::fs::read(ca_path).expect("generated CA on disk");
    let ca = rustls_pemfile::certs(&mut ca_pem.as_slice()).next().expect("a CA cert").expect("parses");
    let client = connector(ca, None);
    let domain = ServerName::try_from("127.0.0.1").expect("ip server name");
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let mut tls = client.connect(domain, tcp).await.expect("the generated serving cert verifies for 127.0.0.1");
    let raw = format!("GET /api/v1/nodes HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n", addr.port());
    tls.write_all(raw.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    let _ = tls.read_to_end(&mut buf).await;
    let resp = String::from_utf8_lossy(&buf).into_owned();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("hub-01"), "{resp}");

    // Plain HTTP is not served on that port.
    let mut plain = TcpStream::connect(addr).await.expect("connect");
    plain.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await.expect("write");
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), plain.read_to_end(&mut buf)).await;
    assert!(!String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 200"), "plaintext must not be answered");

    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}
