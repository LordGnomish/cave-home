// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! End-to-end transport tests against a mock System Access Point.
//!
//! The unit tests pin each pure layer (request shapes, JSON DTOs, event codec)
//! without a network. These tests close the loop over a *real* socket:
//!
//! - REST: a [`wiremock`] HTTP server stands in for the SysAP `fhapi` REST
//!   surface. The real [`FreeAtHomeClient`] (reqwest) issues device-list,
//!   configuration/discovery, read and write calls over HTTP and we assert the
//!   wire interaction (paths, Basic auth header, body) and the decoded result.
//! - WebSocket: a real `tokio-tungstenite` server accepts the client's live
//!   subscription and pushes a datapoint frame; we assert the client decodes it
//!   into a typed event and counts the state change.
//!
//! The client is pointed at each mock via [`ClientConfig::with_origin`], so the
//! exact same production code path runs as against a physical SysAP (minus TLS,
//! which the SysAP terminates and which is covered by the insecure-cert seam).

// Tests lean on expect/unwrap/panic for clarity (workspace convention); the
// pedantic group is relaxed for the same reason integration tests are exempt
// from the library's lint gate.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::time::Duration;

use cave_home_free_home::DeviceKind;
use cave_home_freeathome::{
    AuthMethod, ClientConfig, FreeAtHomeClient, FreeAtHomeDevice, FreeAtHomeEvent,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::net::TcpListener;
use wiremock::matchers::{body_string, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SYSAP_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// A configuration tree with one of every supported kind.
fn configuration_json() -> String {
    format!(
        r#"{{
          "{SYSAP_UUID}": {{
            "devices": {{
              "ABB700C12345": {{
                "displayName": "Wohnzimmer Decke",
                "channels": {{
                  "ch0000": {{
                    "functionID": "0012",
                    "room": "Wohnzimmer",
                    "inputs":  {{ "idp0000": {{ "pairingID": 1,   "value": "1" }} }},
                    "outputs": {{ "odp0000": {{ "pairingID": 256, "value": "1" }} }}
                  }}
                }}
              }},
              "ABB700C22222": {{
                "displayName": "Schlafzimmer Rollladen",
                "channels": {{
                  "ch0001": {{
                    "functionID": "0061",
                    "outputs": {{ "odp0001": {{ "pairingID": 133, "value": "30" }} }}
                  }}
                }}
              }}
            }}
          }}
        }}"#
    )
}

fn devicelist_json() -> String {
    format!(r#"{{ "{SYSAP_UUID}": ["ABB700C12345", "ABB700C22222"] }}"#)
}

fn client_for(origin: &str) -> FreeAtHomeClient {
    let config = ClientConfig::new("unused.host", AuthMethod::basic("installer", "secret"))
        .with_origin(origin);
    FreeAtHomeClient::new(config).expect("client builds")
}

#[tokio::test]
async fn rest_device_list_and_discovery_e2e() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/fhapi/v1/api/rest/devicelist"))
        .respond_with(ResponseTemplate::new(200).set_body_string(devicelist_json()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhapi/v1/api/rest/configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_string(configuration_json()))
        .mount(&server)
        .await;

    let client = client_for(&server.uri());

    // Device list round-trips over real HTTP.
    let list = client.device_list().await.expect("device list");
    assert_eq!(list.serials(), vec!["ABB700C12345", "ABB700C22222"]);

    // Discovery parses the configuration into typed, kind-projected devices.
    let devices = client.discover().await.expect("discover");
    assert_eq!(devices.len(), 2);

    let light = devices
        .iter()
        .find(|d| d.kind() == DeviceKind::Light)
        .expect("a light");
    assert_eq!(light.friendly_name(), "Wohnzimmer Decke");
    assert_eq!(light.room(), Some("Wohnzimmer"));
    assert_eq!(light.display_state(), "on");

    let cover = devices
        .iter()
        .find(|d| d.kind() == DeviceKind::Cover)
        .expect("a cover");
    assert_eq!(cover.display_state(), "30");

    // Two successful REST round-trips were observed.
    assert_eq!(client.metrics().latency_count(), 2);

    // The Basic auth header reached the wire on every request.
    let expected = client.authorization_header().expect("basic header");
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 2);
    for req in &requests {
        let got = req
            .headers
            .get("authorization")
            .expect("authorization header present");
        assert_eq!(got.to_str().unwrap(), expected);
    }
}

#[tokio::test]
async fn rest_read_and_write_datapoint_e2e() {
    let server = MockServer::start().await;

    // Read odp0000 → "1".
    Mock::given(method("GET"))
        .and(path(
            "/fhapi/v1/api/rest/datapoint/ABB700C12345/ch0000/odp0000",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("1"))
        .mount(&server)
        .await;
    // Write idp0000 = "0", asserting the PUT body.
    Mock::given(method("PUT"))
        .and(path(
            "/fhapi/v1/api/rest/datapoint/ABB700C12345/ch0000/idp0000",
        ))
        .and(body_string("0"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&server)
        .await;

    let client = client_for(&server.uri());
    use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial, Direction};
    let serial = DeviceSerial::parse("ABB700C12345").unwrap();

    let value = client
        .get_datapoint(
            serial.clone(),
            ChannelId::new(0),
            DatapointId::new(Direction::Output, 0),
        )
        .await
        .expect("read ok");
    assert_eq!(value, "1");

    client
        .set_datapoint(
            serial,
            ChannelId::new(0),
            DatapointId::new(Direction::Input, 0),
            "0",
        )
        .await
        .expect("write ok");
}

#[tokio::test]
async fn rest_unauthorized_is_auth_error_and_counted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fhapi/v1/api/rest/devicelist"))
        .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
        .mount(&server)
        .await;

    let client = client_for(&server.uri());
    let err = client
        .device_list()
        .await
        .expect_err("should be unauthorized");
    assert!(
        matches!(err, cave_home_freeathome::FreeAtHomeError::Auth(_)),
        "expected auth error, got {err:?}"
    );
    assert!(
        client
            .metrics()
            .render()
            .contains("freeathome_auth_failures_total 1")
    );
}

#[tokio::test]
async fn websocket_live_event_e2e() {
    // A real WebSocket server: accept one connection, push a datapoint frame.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let frame = format!(
        r#"{{ "{SYSAP_UUID}": {{ "datapoints": {{ "ABB700C12345/ch0000/odp0000": "1" }} }} }}"#
    );

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("ws handshake");
        ws.send(tokio_tungstenite::tungstenite::Message::Text(frame))
            .await
            .expect("send frame");
        // Keep the connection open until the client task is aborted.
        let _ = ws.next().await;
    });

    // Point the client's WS URL at the local server (http origin → ws scheme).
    let client = client_for(&format!("http://{addr}"));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sub_client = client.clone();
    let sub = tokio::spawn(async move {
        let _ = sub_client
            .subscribe(move |ev| {
                let _ = tx.send(ev);
            })
            .await;
    });

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("event within timeout")
        .expect("an event");

    match event {
        FreeAtHomeEvent::DatapointUpdate(u) => {
            assert_eq!(u.serial().as_str(), "ABB700C12345");
            assert_eq!(u.value(), "1");
        }
        other => panic!("expected a datapoint update, got {other:?}"),
    }

    // The live-update path counted the state change.
    assert!(client.metrics().state_changes() >= 1);

    sub.abort();
    server.abort();
}
