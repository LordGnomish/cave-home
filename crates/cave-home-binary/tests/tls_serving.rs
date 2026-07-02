// SPDX-License-Identifier: Apache-2.0
#![cfg(feature = "tls")]
#![allow(clippy::expect_used)] // test harness: a failed expect is the test failing
//! Live TLS serving test: boot the runtime with a self-signed server cert and
//! prove the apiserver answers over a real rustls client connection.
//!
//! This goes the whole way: a throwaway self-signed cert+key is generated
//! in-test, written to disk, and handed to the runtime via `RuntimeConfig::tls`.
//! The runtime then terminates TLS on its accept loop, and a real `tokio-rustls`
//! client (trusting that cert) completes the handshake and gets the apiserver's
//! plaintext-inside-TLS HTTP response. No fixture key material is checked in.

use std::sync::Arc;
use std::time::Duration;

use cave_home_binary::node::LocalNode;
use cave_home_binary::server::{run_until_on_listener, RuntimeConfig};
use cave_home_binary::tls::TlsConfig;
use cave_home_orchestration::role::NodeIntent;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_rustls::TlsConnector;

/// Generate a self-signed cert for `localhost`, write cert+key to `dir`, and
/// return their paths plus the cert DER (for the client trust store).
fn self_signed(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, CertificateDer<'static>) {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("gen");
    let cert_der = CertificateDer::from(key.cert.der().to_vec());
    let cert_path = dir.join("server.crt");
    let key_path = dir.join("server.key");
    std::fs::write(&cert_path, key.cert.pem()).expect("write cert");
    std::fs::write(&key_path, key.key_pair.serialize_pem()).expect("write key");
    (cert_path, key_path, cert_der)
}

/// A rustls client config that trusts exactly the given server certificate.
/// The crypto provider is pinned to `ring` so the test does not depend on a
/// process-wide default provider being installed.
fn client_config(server_cert: CertificateDer<'static>) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(server_cert).expect("add trust anchor");
    ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apiserver_serves_over_real_tls() {
    let dir = std::env::temp_dir().join(format!("cavehome-tls-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let (cert_path, key_path, cert_der) = self_signed(&dir);

    // Bind first to learn the ephemeral port, then boot the runtime with TLS.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    let cfg = RuntimeConfig {
        intent: NodeIntent::PrimaryHub,
        node: LocalNode::new("hub-01", "127.0.0.1"),
        bind_addr: "127.0.0.1".to_string(),
        bind_port: addr.port(),
        reconcile_interval: Duration::from_millis(50),
        tls: Some(TlsConfig::new(cert_path, key_path)),
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

    // A real TLS client trusting the server's self-signed cert.
    let connector = TlsConnector::from(Arc::new(client_config(cert_der)));
    let domain = ServerName::try_from("localhost").expect("server name");

    // Connect + handshake + GET /healthz over TLS, retrying until the listener is
    // up. A plain-HTTP server would fail the handshake, so a 200 here proves the
    // TLS terminator is really in the path.
    let body = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(tcp) = TcpStream::connect(addr).await {
                if let Ok(mut tls) = connector.connect(domain.clone(), tcp).await {
                    tls.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                        .await
                        .expect("write");
                    let mut buf = Vec::new();
                    let _ = tls.read_to_end(&mut buf).await;
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if text.contains("200") {
                        return text;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("apiserver answered over TLS within the deadline");

    assert!(body.contains("200"), "200 over TLS: {body}");
    assert!(body.contains("ok"), "healthz body over TLS: {body}");

    // The write path works over TLS too: create a Pod, read it back encrypted.
    let connector2 = connector.clone();
    let pod = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"tls-pod"},"spec":{"containers":[{"name":"c","image":"nginx"}]}}"#;
    let tcp = TcpStream::connect(addr).await.expect("connect");
    let mut tls = connector2.connect(domain.clone(), tcp).await.expect("handshake");
    let post = format!(
        "POST /api/v1/namespaces/default/pods HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        pod.len(),
        pod
    );
    tls.write_all(post.as_bytes()).await.expect("write post");
    let mut buf = Vec::new();
    let _ = tls.read_to_end(&mut buf).await;
    let resp = String::from_utf8_lossy(&buf).into_owned();
    assert!(resp.contains("201"), "create over TLS returns 201: {resp}");
    assert!(resp.contains("tls-pod"), "{resp}");

    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await.expect("shuts down");
}
