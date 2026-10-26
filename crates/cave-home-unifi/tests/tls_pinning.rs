// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! TLS trust for the real transports, against a local HTTPS "console" with an
//! rcgen self-signed certificate (offline). The threat: a compromised device on
//! the household LAN answering for the console. A mismatched certificate must
//! fail the handshake before the admin password, the API key or the Access
//! door-unlock bearer token is written to the socket.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use cave_home_tls_pin::test_support::{TestCert, TlsTestServer, self_signed};
use cave_home_unifi::access::{AccessClient, AccessConfig};
use cave_home_unifi::client::ConsoleClient;
use cave_home_unifi::console::Console;
use cave_home_unifi::tls::{Pin, PinSet, Sha256Fingerprint, TlsTrust};
use cave_home_unifi::transport::ReqwestTransport;
use cave_home_unifi::ws::{TungsteniteConnector, WsConnector, WsRequest};
use cave_home_unifi::{Credentials, HttpMethod, HttpRequest, HttpTransport};

const ENVELOPE_OK: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n{\"meta\":{\"rc\":\"ok\"},\"data\":[]}\r\n";

fn pin_of(cert: &TestCert) -> TlsTrust {
    TlsTrust::pinned(PinSet::from(Pin::Certificate(Sha256Fingerprint::of(cert.cert.as_ref())))).unwrap()
}

fn console(server: &TlsTestServer) -> Console {
    Console::unifi_os("127.0.0.1").with_port(server.addr().port())
}

#[tokio::test]
async fn pinned_transport_reaches_the_pinned_console() {
    let cert = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), ENVELOPE_OK);
    let transport = ReqwestTransport::new(Duration::from_secs(5), &pin_of(&cert)).unwrap();

    let url = console(&server).network_site_url("default", "stat/device");
    let resp = transport.execute(HttpRequest::new(HttpMethod::Get, url)).await.unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(server.received().len(), 1);
}

#[tokio::test]
async fn pinned_transport_refuses_a_mitm_before_sending_the_password() {
    let console_cert = self_signed(&["unifi.local"]);
    let mitm = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), ENVELOPE_OK);

    let transport = ReqwestTransport::new(Duration::from_secs(5), &pin_of(&console_cert)).unwrap();
    let client = ConsoleClient::new(console(&server), transport, Credentials::password("admin", "hunter2"));
    assert!(client.login().await.is_err(), "login through a MITM must fail");

    server.wait_idle();
    let seen = server.received();
    assert!(seen.is_empty(), "the MITM received: {seen:?}");
}

#[tokio::test]
async fn access_bearer_token_never_reaches_an_unpinned_host() {
    let access_cert = self_signed(&["unifi.local"]);
    let mitm = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), ENVELOPE_OK);

    let access = AccessClient::new(
        AccessConfig::new("127.0.0.1", "DOOR-UNLOCK-TOKEN").with_port(server.addr().port()),
        ReqwestTransport::new(Duration::from_secs(5), &pin_of(&access_cert)).unwrap(),
    );
    assert!(access.unlock("front-door").await.is_err());
    server.wait_idle();
    assert!(server.received().is_empty(), "token leaked: {:?}", server.received());
}

#[tokio::test]
async fn websocket_connector_sends_the_bearer_only_to_the_pinned_host() {
    const REFUSE_UPGRADE: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let access_cert = self_signed(&["unifi.local"]);

    // Pinned host: the upgrade request (carrying the bearer) arrives.
    let good = TlsTestServer::start(vec![access_cert.cert.clone()], access_cert.key.clone_key(), REFUSE_UPGRADE);
    let cfg = AccessConfig::new("127.0.0.1", "DOOR-UNLOCK-TOKEN").with_port(good.addr().port());
    let connector = TungsteniteConnector::new(&pin_of(&access_cert)).unwrap();
    let _ = connector.connect(WsRequest::access_notifications(&cfg)).await;
    good.wait_idle();
    assert_eq!(good.received().len(), 1);
    assert!(good.received()[0].contains("DOOR-UNLOCK-TOKEN"));

    // Anyone else: the handshake fails and nothing is sent.
    let mitm = self_signed(&["unifi.local"]);
    let bad = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), REFUSE_UPGRADE);
    let cfg = AccessConfig::new("127.0.0.1", "DOOR-UNLOCK-TOKEN").with_port(bad.addr().port());
    assert!(connector.connect(WsRequest::access_notifications(&cfg)).await.is_err());
    bad.wait_idle();
    assert!(bad.received().is_empty(), "token leaked over ws: {:?}", bad.received());
}

#[tokio::test]
async fn accepting_any_certificate_is_an_explicit_opt_in() {
    let cert = self_signed(&["whatever"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), ENVELOPE_OK);
    let transport =
        ReqwestTransport::new(Duration::from_secs(5), &TlsTrust::dangerous_accept_any_certificate()).unwrap();
    let url = console(&server).network_site_url("default", "stat/device");
    assert_eq!(
        transport.execute(HttpRequest::new(HttpMethod::Get, url)).await.unwrap().status,
        200
    );
}
