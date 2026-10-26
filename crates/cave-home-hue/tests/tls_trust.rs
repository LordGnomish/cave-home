// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Bridge TLS trust against a local HTTPS "bridge" (rcgen, offline). The
//! application key is the bridge's only credential; it must never be sent to a
//! host that failed verification.
#![cfg(feature = "runtime")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cave_home_hue::errors::HueError;
use cave_home_hue::tls::{Pin, PinSet, Sha256Fingerprint, TlsTrust};
use cave_home_hue::v2::controllers::base::V2Request;
use cave_home_hue::v2::transport::{
    ReqwestTransport, SIGNIFY_HUE_ROOT_CA_PEM, signify_bridge_trust, signify_root_ca_der,
};
use cave_home_tls_pin::test_support::{TestCa, TlsTestServer, self_signed};

const EMPTY_ENVELOPE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 23\r\nConnection: close\r\n\r\n{\"errors\":[],\"data\":[]}";

const BRIDGE_ID: &str = "001788fffe1234ab";

#[tokio::test]
async fn pinned_bridge_round_trips_with_the_app_key() {
    let cert = self_signed(&[BRIDGE_ID]);
    let server = TlsTestServer::start(vec![cert.cert.clone()], cert.key.clone_key(), EMPTY_ENVELOPE);
    let trust = TlsTrust::pinned(PinSet::from(Pin::Certificate(Sha256Fingerprint::of(cert.cert.as_ref())))).unwrap();

    let t = ReqwestTransport::new(&server.authority(), "APP-KEY", &trust).unwrap();
    t.get("resource/light").await.unwrap();
    assert!(server.received()[0].contains("APP-KEY"));
}

#[tokio::test]
async fn pinned_bridge_rejects_a_mitm_and_keeps_the_app_key() {
    let bridge = self_signed(&[BRIDGE_ID]);
    let mitm = self_signed(&[BRIDGE_ID]);
    let server = TlsTestServer::start(vec![mitm.cert.clone()], mitm.key.clone_key(), EMPTY_ENVELOPE);
    let trust =
        TlsTrust::pinned(PinSet::from(Pin::Certificate(Sha256Fingerprint::of(bridge.cert.as_ref())))).unwrap();

    let t = ReqwestTransport::new(&server.authority(), "APP-KEY", &trust).unwrap();
    let err = t.get("resource/light").await.unwrap_err();
    assert!(matches!(err, HueError::Transport(_)), "got {err:?}");
    server.wait_idle();
    assert!(server.received().is_empty(), "app key leaked: {:?}", server.received());
}

#[tokio::test]
async fn root_issued_bridge_cert_with_matching_bridge_id_is_accepted() {
    let root = TestCa::new("root-bridge");
    let leaf = root.issue_leaf(BRIDGE_ID, &[]);
    let server = TlsTestServer::start(vec![leaf.cert.clone()], leaf.key.clone_key(), EMPTY_ENVELOPE);

    let trust = TlsTrust::common_name_ca(vec![root.der()], &BRIDGE_ID.to_uppercase()).unwrap();
    let t = ReqwestTransport::new(&server.authority(), "APP-KEY", &trust).unwrap();
    t.get("resource/light").await.unwrap();
}

#[tokio::test]
async fn root_issued_cert_for_another_bridge_is_rejected() {
    let root = TestCa::new("root-bridge");
    let other = root.issue_leaf("001788fffe000000", &[]);
    let server = TlsTestServer::start(vec![other.cert.clone()], other.key.clone_key(), EMPTY_ENVELOPE);

    let trust = TlsTrust::common_name_ca(vec![root.der()], BRIDGE_ID).unwrap();
    let t = ReqwestTransport::new(&server.authority(), "APP-KEY", &trust).unwrap();
    assert!(t.get("resource/light").await.is_err());
    server.wait_idle();
    assert!(server.received().is_empty());
}

#[test]
fn embedded_signify_root_is_the_published_hue_root() {
    assert!(SIGNIFY_HUE_ROOT_CA_PEM.contains("BEGIN CERTIFICATE"));
    let der = signify_root_ca_der().unwrap();
    // `openssl x509 -noout -fingerprint -sha256` of the published root-bridge
    // certificate (C=NL, O=Philips Hue, CN=root-bridge, 2017-01-01..2038-01-19).
    assert_eq!(
        Sha256Fingerprint::of(der.as_ref()).to_string(),
        "F0:BD:8E:65:09:E8:2F:77:4D:63:BC:00:9D:53:88:C9:69:FE:3D:CF:7D:6D:54:1D:63:51:B7:2B:89:8D:8A:CF"
    );
}

#[tokio::test]
async fn signify_trust_requires_the_bridge_id_and_rejects_foreign_roots() {
    assert!(signify_bridge_trust("").is_err());
    let trust = signify_bridge_trust(BRIDGE_ID).unwrap();
    assert!(matches!(&trust, TlsTrust::CommonNameCa { common_name, .. } if common_name == BRIDGE_ID));

    // A cert with the right CN but not issued by the Signify root fails.
    let forged = TestCa::new("root-bridge").issue_leaf(BRIDGE_ID, &[]);
    let server = TlsTestServer::start(vec![forged.cert.clone()], forged.key.clone_key(), EMPTY_ENVELOPE);
    let t = ReqwestTransport::new(&server.authority(), "APP-KEY", &trust).unwrap();
    assert!(t.get("resource/light").await.is_err());
    server.wait_idle();
    assert!(server.received().is_empty());
}
