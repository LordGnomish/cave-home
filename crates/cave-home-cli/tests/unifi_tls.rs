// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cavehomectl unifi live` / `unifi access doors` never send the console
//! password, API key or the Access door-unlock token to an unverified peer.
//! A local TLS "console" (rcgen, offline) stands in for the real one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::ErrorKind;
use std::net::TcpListener;

use assert_cmd::Command;
use cave_home_tls_pin::Sha256Fingerprint;
use cave_home_tls_pin::test_support::{TlsTestServer, self_signed};
use predicates::str::{contains, is_empty};

const ENVELOPE_OK: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 30\r\nConnection: close\r\n\r\n{\"meta\":{\"rc\":\"ok\"},\"data\":[]}";

fn cavehomectl() -> Command {
    let mut cmd = Command::cargo_bin("cavehomectl").expect("cavehomectl binary");
    for var in [
        "CAVEHOME_UNIFI_HOST",
        "CAVEHOME_UNIFI_PORT",
        "CAVEHOME_UNIFI_API_KEY",
        "CAVEHOME_UNIFI_USER",
        "CAVEHOME_UNIFI_PASS",
        "CAVEHOME_UNIFI_KIND",
        "CAVEHOME_UNIFI_SITE",
        "CAVEHOME_UNIFI_FINGERPRINT",
        "CAVEHOME_ACCESS_HOST",
        "CAVEHOME_ACCESS_PORT",
        "CAVEHOME_ACCESS_TOKEN",
        "CAVEHOME_ACCESS_FINGERPRINT",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

fn no_connection_attempted(listener: &TcpListener) -> bool {
    matches!(listener.accept(), Err(e) if e.kind() == ErrorKind::WouldBlock)
}

#[test]
fn live_refuses_to_send_credentials_when_nothing_is_pinned() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();

    cavehomectl()
        .args(["unifi", "live", "devices"])
        .env("CAVEHOME_UNIFI_HOST", "127.0.0.1")
        .env("CAVEHOME_UNIFI_PORT", &port)
        .env("CAVEHOME_UNIFI_USER", "admin")
        .env("CAVEHOME_UNIFI_PASS", "hunter2")
        .assert()
        .code(2)
        .stdout(is_empty())
        .stderr(contains("CAVEHOME_UNIFI_FINGERPRINT"))
        .stderr(contains("--insecure"));
    assert!(no_connection_attempted(&listener), "the CLI connected without a pin");
}

#[test]
fn access_doors_refuses_to_send_the_door_token_when_nothing_is_pinned() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();

    for args in [["unifi", "access", "doors"], ["unifi", "live", "doors"]] {
        cavehomectl()
            .args(args)
            .env("CAVEHOME_ACCESS_HOST", "127.0.0.1")
            .env("CAVEHOME_ACCESS_PORT", &port)
            .env("CAVEHOME_ACCESS_TOKEN", "DOOR-UNLOCK-TOKEN")
            .assert()
            .code(2)
            .stdout(is_empty())
            .stderr(contains("CAVEHOME_ACCESS_FINGERPRINT"));
    }
    assert!(no_connection_attempted(&listener), "the CLI connected without a pin");
}

#[test]
fn live_with_a_wrong_fingerprint_fails_closed_and_names_what_was_presented() {
    let mitm = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), ENVELOPE_OK);
    let pinned = Sha256Fingerprint::of(self_signed(&["unifi.local"]).cert.as_ref());

    cavehomectl()
        .args(["unifi", "live", "devices", "--fingerprint", &pinned.to_string()])
        .env("CAVEHOME_UNIFI_HOST", "127.0.0.1")
        .env("CAVEHOME_UNIFI_PORT", server.addr().port().to_string())
        .env("CAVEHOME_UNIFI_API_KEY", "ADMIN-API-KEY")
        .assert()
        .code(1)
        .stdout(is_empty())
        .stderr(contains(Sha256Fingerprint::of(mitm.cert.as_ref()).to_string()));
    server.wait_idle();
    assert!(server.received().is_empty(), "API key leaked: {:?}", server.received());
}

#[test]
fn live_with_the_pinned_fingerprint_reaches_the_console() {
    let cert = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), ENVELOPE_OK);

    cavehomectl()
        .args(["unifi", "live", "devices"])
        .env("CAVEHOME_UNIFI_HOST", "127.0.0.1")
        .env("CAVEHOME_UNIFI_PORT", server.addr().port().to_string())
        .env("CAVEHOME_UNIFI_API_KEY", "ADMIN-API-KEY")
        .env("CAVEHOME_UNIFI_FINGERPRINT", Sha256Fingerprint::of(cert.cert.as_ref()).to_string())
        .assert()
        .success();
    assert!(server.received()[0].to_ascii_lowercase().contains("x-api-key: admin-api-key"));
}

#[test]
fn insecure_is_an_explicit_opt_in_that_warns() {
    let cert = self_signed(&["anything"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), ENVELOPE_OK);

    cavehomectl()
        .args(["unifi", "live", "devices", "--insecure"])
        .env("CAVEHOME_UNIFI_HOST", "127.0.0.1")
        .env("CAVEHOME_UNIFI_PORT", server.addr().port().to_string())
        .env("CAVEHOME_UNIFI_API_KEY", "ADMIN-API-KEY")
        .assert()
        .success()
        .stderr(contains("UYARI"))
        .stderr(contains("--insecure"));
}

#[test]
fn a_malformed_fingerprint_is_a_usage_error() {
    cavehomectl()
        .args(["unifi", "live", "devices", "--fingerprint", "not-a-fingerprint"])
        .env("CAVEHOME_UNIFI_HOST", "127.0.0.1")
        .env("CAVEHOME_UNIFI_API_KEY", "ADMIN-API-KEY")
        .assert()
        .code(2)
        .stdout(is_empty());
}

#[test]
fn fingerprint_command_captures_the_certificate_for_onboarding() {
    let cert = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), ENVELOPE_OK);
    let expected = Sha256Fingerprint::of(cert.cert.as_ref()).to_string();

    cavehomectl()
        .args(["unifi", "fingerprint", &server.authority()])
        .assert()
        .success()
        .stdout(contains(expected.as_str()))
        .stdout(contains("CAVEHOME_UNIFI_FINGERPRINT="));
    server.wait_idle();
    assert!(server.received().is_empty(), "capture must not send data");
}
