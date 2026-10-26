// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Pinning accept / reject against a real rustls server on 127.0.0.1.
//!
//! Every test runs a full TLS handshake (rcgen certificates, offline) through
//! the `rustls::ClientConfig` that [`TlsTrust::client_config`] builds — the
//! same config the UniFi and Hue transports hand to reqwest and tungstenite.
//! The server records the application bytes it receives, so a rejected
//! handshake is also checked to have carried nothing (no credentials leak).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use cave_home_tls_pin::test_support::{TestCa, TlsTestServer, self_signed, self_signed_same_key};
use cave_home_tls_pin::{
    capture_presented_certificate, Pin, PinSet, PresentedCertificate, Sha256Fingerprint, TlsTrust,
    TrustError,
};
use rustls::pki_types::ServerName;

const OK_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

/// Connect to `server`, handshake with `trust`, send one request carrying a
/// fake credential header. Returns the handshake / IO error, if any.
fn request(trust: &TlsTrust, server: &TlsTestServer, sni: &str) -> Result<String, String> {
    let config = trust.client_config().map_err(|e| e.to_string())?;
    let name = ServerName::try_from(sni.to_owned()).map_err(|e| e.to_string())?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let mut tcp = TcpStream::connect(server.addr()).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
    tls.write_all(b"GET / HTTP/1.1\r\nHost: console\r\nAuthorization: Bearer SECRET\r\nContent-Length: 0\r\n\r\n")
        .map_err(|e| e.to_string())?;
    tls.flush().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    match tls.read_to_end(&mut out) {
        Ok(_) => {}
        // The test server closes after one response; tolerate a missing
        // close_notify as long as the response arrived.
        Err(e) if !out.is_empty() => drop(e),
        Err(e) => return Err(e.to_string()),
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[test]
fn certificate_pin_accepts_the_pinned_console() {
    let cert = self_signed(&["unifi.local"]);
    let pin = Pin::Certificate(Sha256Fingerprint::of(cert.cert.as_ref()));
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), OK_RESPONSE);

    let trust = TlsTrust::pinned(PinSet::from(pin)).unwrap();
    let body = request(&trust, &server, "unifi.local").expect("pinned handshake must succeed");

    assert!(body.ends_with("ok"), "body={body}");
    assert_eq!(server.received().len(), 1);
    assert!(server.received()[0].contains("Bearer SECRET"));
}

#[test]
fn certificate_pin_ignores_the_name_on_a_self_signed_console_cert() {
    // UniFi consoles ship a self-signed cert for "unifi.local"; the operator
    // reaches them by IP. The pin, not the name, is the identity.
    let cert = self_signed(&["unifi.local"]);
    let pin = Pin::Certificate(Sha256Fingerprint::of(cert.cert.as_ref()));
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), OK_RESPONSE);
    let trust = TlsTrust::pinned(PinSet::from(pin)).unwrap();
    assert!(request(&trust, &server, "192.168.1.1").is_ok());
}

#[test]
fn certificate_pin_rejects_a_different_certificate_and_sends_nothing() {
    let pinned = self_signed(&["unifi.local"]);
    let mitm = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), OK_RESPONSE);

    let pin = Pin::Certificate(Sha256Fingerprint::of(pinned.cert.as_ref()));
    let trust = TlsTrust::pinned(PinSet::from(pin)).unwrap();
    let err = request(&trust, &server, "unifi.local").expect_err("mismatched cert must fail closed");

    // The error names what was presented, so onboarding can show it.
    let presented = Sha256Fingerprint::of(mitm.cert.as_ref()).to_string();
    assert!(err.contains(&presented), "error should name presented fingerprint: {err}");
    server.wait_idle();
    assert!(server.received().is_empty(), "credentials reached the MITM: {:?}", server.received());
}

#[test]
fn public_key_pin_survives_certificate_reissue_with_the_same_key() {
    let (first, reissued) = self_signed_same_key(&["unifi.local"], &["unifi.local", "console"]);
    assert_ne!(first.cert.as_ref(), reissued.cert.as_ref());
    let server = TlsTestServer::start(vec![reissued.cert.clone()], reissued.key.clone_key(), OK_RESPONSE);

    let spki = PresentedCertificate::from_der(first.cert.clone())
        .public_key_pin()
        .expect("rcgen cert has an SPKI");
    assert!(matches!(spki, Pin::PublicKey(_)));
    let trust = TlsTrust::pinned(PinSet::from(spki)).unwrap();
    assert!(request(&trust, &server, "unifi.local").is_ok());

    // ...while a certificate pin of the first cert rejects the re-issue.
    let cert_pin = Pin::Certificate(Sha256Fingerprint::of(first.cert.as_ref()));
    let trust = TlsTrust::pinned(PinSet::from(cert_pin)).unwrap();
    assert!(request(&trust, &server, "unifi.local").is_err());
}

#[test]
fn any_pin_in_a_set_may_match() {
    let old = self_signed(&["unifi.local"]);
    let new = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![new.cert.clone()], new.key.clone_key(), OK_RESPONSE);
    let pins: PinSet = format!(
        "{}, {}",
        Sha256Fingerprint::of(old.cert.as_ref()),
        Sha256Fingerprint::of(new.cert.as_ref())
    )
    .parse()
    .unwrap();
    assert_eq!(pins.pins().len(), 2);
    let trust = TlsTrust::pinned(pins).unwrap();
    assert!(request(&trust, &server, "unifi.local").is_ok());
}

#[test]
fn fingerprint_display_is_openssl_style_and_round_trips() {
    let fp = Sha256Fingerprint::of(b"cave-home");
    let shown = fp.to_string();
    assert_eq!(shown.len(), 32 * 3 - 1);
    assert!(shown.chars().all(|c| c == ':' || c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    assert_eq!(Sha256Fingerprint::from_str(&shown).unwrap(), fp);
    // lowercase, no separators (sha256sum style) parses too
    let bare: String = shown.chars().filter(|c| *c != ':').collect::<String>().to_lowercase();
    assert_eq!(Sha256Fingerprint::from_str(&bare).unwrap(), fp);
}

#[test]
fn pin_parsing_accepts_openssl_and_curl_formats() {
    let fp = Sha256Fingerprint::of(b"leaf");
    assert_eq!(Pin::from_str(&fp.to_string()).unwrap(), Pin::Certificate(fp));
    assert_eq!(Pin::from_str(&format!("cert:{fp}")).unwrap(), Pin::Certificate(fp));
    // curl --pinnedpubkey style
    let curl = format!("sha256//{}", fp.to_base64());
    assert_eq!(Pin::from_str(&curl).unwrap(), Pin::PublicKey(fp));
    assert_eq!(Pin::PublicKey(fp).to_string(), curl);
    assert_eq!(Pin::from_str(&format!("spki:{fp}")).unwrap(), Pin::PublicKey(fp));

    for bad in ["", "abc", "sha256//not*base64", "sha256//AAAA", "zz:zz", "spki:"] {
        assert!(Pin::from_str(bad).is_err(), "should reject {bad:?}");
    }
}

#[test]
fn an_empty_pin_set_is_refused() {
    assert!(PinSet::from_str("").is_err());
    assert!(PinSet::from_str(" , ").is_err());
    assert!(matches!(TlsTrust::pinned(PinSet::default()), Err(TrustError::NoPins)));
}

#[test]
fn capture_reports_the_presented_certificate_without_sending_data() {
    let cert = self_signed(&["unifi.local"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), OK_RESPONSE);

    let seen = capture_presented_certificate(&server.addr().to_string(), "unifi.local", Duration::from_secs(5))
        .expect("capture");
    assert_eq!(seen.certificate_sha256(), Sha256Fingerprint::of(cert.cert.as_ref()));
    assert!(seen.public_key_sha256().is_some());
    assert_eq!(seen.der().as_ref(), cert.cert.as_ref());

    // The captured pin then verifies the real server.
    let trust = TlsTrust::pinned(PinSet::from(seen.certificate_pin())).unwrap();
    server.wait_idle();
    assert!(server.received().is_empty(), "capture must not send application data");
    assert!(request(&trust, &server, "unifi.local").is_ok());
}

// ---- private CA + common name (the Hue bridge model) ----------------------

const BRIDGE_ID: &str = "001788fffe1234ab";

#[test]
fn common_name_ca_accepts_a_leaf_with_the_expected_common_name() {
    let ca = TestCa::new("root-bridge");
    let leaf = ca.issue_leaf(BRIDGE_ID, &[]);
    let server = TlsTestServer::start(vec![leaf.cert.clone()], leaf.key.clone_key(), OK_RESPONSE);

    // Bridge ids are compared case-insensitively (the API reports upper-case).
    let trust = TlsTrust::common_name_ca(vec![ca.der()], &BRIDGE_ID.to_uppercase()).unwrap();
    assert!(request(&trust, &server, "192.168.1.20").is_ok());
}

#[test]
fn common_name_ca_rejects_another_bridge_from_the_same_ca() {
    let ca = TestCa::new("root-bridge");
    let other = ca.issue_leaf("001788fffe999999", &[]);
    let server = TlsTestServer::start(vec![other.cert.clone()], other.key.clone_key(), OK_RESPONSE);
    let trust = TlsTrust::common_name_ca(vec![ca.der()], BRIDGE_ID).unwrap();
    assert!(request(&trust, &server, "192.168.1.20").is_err());
    server.wait_idle();
    assert!(server.received().is_empty());
}

#[test]
fn common_name_ca_rejects_a_self_signed_leaf_claiming_the_bridge_id() {
    let ca = TestCa::new("root-bridge");
    let forged = TestCa::new("root-bridge").issue_leaf(BRIDGE_ID, &[]);
    let server = TlsTestServer::start(vec![forged.cert.clone()], forged.key.clone_key(), OK_RESPONSE);
    let trust = TlsTrust::common_name_ca(vec![ca.der()], BRIDGE_ID).unwrap();
    assert!(request(&trust, &server, "192.168.1.20").is_err());
    server.wait_idle();
    assert!(server.received().is_empty());
}

#[test]
fn common_name_ca_needs_roots_and_a_name() {
    let ca = TestCa::new("root-bridge");
    assert!(matches!(TlsTrust::common_name_ca(vec![], BRIDGE_ID), Err(TrustError::NoRoots)));
    assert!(matches!(
        TlsTrust::common_name_ca(vec![ca.der()], "  "),
        Err(TrustError::EmptyCommonName)
    ));
}

// ---- the explicit, logged opt-out -----------------------------------------

#[test]
fn dangerous_accept_any_is_explicitly_named_and_accepts_anything() {
    let cert = self_signed(&["whatever"]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), OK_RESPONSE);
    let trust = TlsTrust::dangerous_accept_any_certificate();
    assert!(trust.is_dangerous());
    assert!(!TlsTrust::pinned(PinSet::from(Pin::Certificate(Sha256Fingerprint::of(b"x"))))
        .unwrap()
        .is_dangerous());
    assert!(request(&trust, &server, "unifi.local").is_ok());
}
