// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end tests against a **mock UniFi console** stood up with `wiremock`.
//!
//! These drive the *real* `reqwest` + `rustls` transport (over plain HTTP, the
//! one knob `wiremock` exposes) through the full stack — login → session cookie
//! + CSRF capture → authorized API call → wire-to-domain mapping — so the same
//! code that talks to a real Cloud Key / UniFi OS console is exercised here. The
//! acceptance trio is covered: a **device list**, a **camera live URL**, and the
//! **intercom** path (the answer over REST + the live notification decoded by
//! the real WebSocket engine).

use std::sync::Arc;
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use cave_home_unifi::access::{AccessClient, AccessConfig};
use cave_home_unifi::client::ConsoleClient;
use cave_home_unifi::console::Console;
use cave_home_unifi::metrics::Metrics;
use cave_home_unifi::network::NetworkApi;
use cave_home_unifi::protect::ProtectApi;
use cave_home_unifi::transport::ReqwestTransport;
use cave_home_unifi::ws::{EventPump, MockWsConnection, WsMessage};
use cave_home_unifi::Credentials;

/// Build a `Console` and a real transport pointed at a wiremock server.
fn console_for(server: &MockServer) -> Console {
    let addr = server.address();
    Console::unifi_os(addr.ip().to_string())
        .with_port(addr.port())
        .with_tls(false)
}

fn transport() -> ReqwestTransport {
    ReqwestTransport::new(Duration::from_secs(5)).expect("build transport")
}

/// Mount the UniFi OS login endpoint: 200 + a `TOKEN` cookie and CSRF header.
async fn mount_login(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/api/auth/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "TOKEN=h.payload.sig; Path=/; HttpOnly")
                .insert_header("x-csrf-token", "csrf-e2e")
                .set_body_string("{}"),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn e2e_login_then_device_list() {
    let server = MockServer::start().await;
    mount_login(&server).await;
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/device"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"meta":{"rc":"ok"},"data":[
                {"_id":"d1","mac":"sw:mac","name":"Salon switch","model":"USW-24",
                 "type":"usw","state":1,
                 "port_table":[{"port_idx":1,"port_poe":true,"poe_good":true}]},
                {"_id":"d2","mac":"ap:mac","name":"Üst kat AP","type":"uap","state":1,
                 "uplink":{"uplink_mac":"sw:mac"}}
            ]}"#,
        ))
        .mount(&server)
        .await;

    let metrics = Arc::new(Metrics::new());
    let client = ConsoleClient::new(
        console_for(&server),
        transport(),
        Credentials::password("admin", "secret"),
    )
    .with_metrics(Arc::clone(&metrics));

    client.login().await.expect("login");
    assert!(client.is_authenticated());

    let api = NetworkApi::new(&client);
    let devices = api.devices("default").await.expect("devices");
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name(), "Salon switch");
    assert!(devices[0].port(1).unwrap().poe_active);
    assert_eq!(devices[1].uplink(), Some("sw:mac"));

    // The real session+metrics flowed through: one login, one device call.
    let prom = metrics.render_prometheus();
    assert!(prom.contains("unifi_logins_total 1"));
    assert!(prom.contains("unifi_requests_total{endpoint=\"network/devices\"} 1"));
}

#[tokio::test]
async fn e2e_camera_live_url_from_bootstrap() {
    let server = MockServer::start().await;
    mount_login(&server).await;
    Mock::given(method("GET"))
        .and(path("/proxy/protect/api/bootstrap"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"lastUpdateId":"u1","nvr":{"id":"n","name":"Home NVR","version":"4.0"},
                "cameras":[
                    {"id":"cam-front","name":"Front door","mac":"m","type":"UVC G4 Doorbell",
                     "state":"CONNECTED",
                     "featureFlags":{"hasMic":true,"hasSpeaker":true,"isDoorbell":true,
                                     "smartDetectTypes":["person","package"]},
                     "recordingSettings":{"mode":"detections"},
                     "channels":[{"id":0,"rtspAlias":"liveAliasXYZ"}]}
                ]}"#,
        ))
        .mount(&server)
        .await;

    let client = ConsoleClient::new(
        console_for(&server),
        transport(),
        Credentials::api_key("KEY"),
    );
    let protect = ProtectApi::new(&client);

    let bootstrap = protect.bootstrap().await.expect("bootstrap");
    assert_eq!(bootstrap.nvr.name, "Home NVR");
    assert_eq!(bootstrap.cameras.len(), 1);
    assert!(bootstrap.cameras[0].is_doorbell);

    let host = client.console().host().to_string();
    let url = protect.camera_live_url("cam-front").await.expect("live url");
    assert_eq!(url, format!("rtsps://{host}:7441/liveAliasXYZ?enableSrtp"));
}

#[tokio::test]
async fn e2e_intercom_answer_over_rest() {
    // The intercom *answer*: a real PUT unlock over the reqwest transport
    // against a mock Access appliance.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/developer/doors/front/unlock"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"code":"SUCCESS","msg":"door unlocked","data":{}}"#),
        )
        .mount(&server)
        .await;

    let addr = server.address();
    let cfg = AccessConfig::new(addr.ip().to_string(), "ACCESS-TOKEN")
        .with_port(addr.port())
        .with_tls(false);
    let access = AccessClient::new(cfg, transport());

    access.answer_intercom("front").await.expect("intercom unlock");
    assert!(access
        .metrics()
        .render_prometheus()
        .contains("unifi_requests_total{endpoint=\"access/intercom_unlock\"} 1"));
}

#[tokio::test]
async fn e2e_intercom_event_via_ws_engine() {
    // The intercom *event*: the real WebSocket engine decodes a live Access
    // notification frame (the doorbell call) into a typed notification.
    let metrics = Arc::new(Metrics::new());
    let frame = r#"{"event":"access.remote_view",
                    "data":{"door":{"name":"Front door"},"actor":{"name":"Visitor"}}}"#;
    let conn = Box::new(MockWsConnection::new(vec![
        WsMessage::Text(frame.to_string()),
    ]));
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let pump = EventPump::new(Arc::clone(&metrics));

    let forwarded = pump
        .access_notifications(conn, tx)
        .await
        .expect("pump intercom");
    assert_eq!(forwarded, 1);

    let note = rx.recv().await.expect("a notification");
    assert!(note.is_intercom_call(), "expected an intercom call");
    assert_eq!(note.door_name.as_deref(), Some("Front door"));
    assert_eq!(note.actor.as_deref(), Some("Visitor"));
    assert!(metrics
        .render_prometheus()
        .contains("unifi_ws_events_total{pillar=\"access\"} 1"));
}

#[tokio::test]
async fn e2e_block_client_posts_real_command() {
    let server = MockServer::start().await;
    mount_login(&server).await;
    Mock::given(method("POST"))
        .and(path("/proxy/network/api/s/default/cmd/stamgr"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"{"meta":{"rc":"ok"},"data":[]}"#),
        )
        .mount(&server)
        .await;

    let client = ConsoleClient::new(
        console_for(&server),
        transport(),
        Credentials::password("admin", "secret"),
    );
    client.login().await.expect("login");
    let api = NetworkApi::new(&client);
    api.block_client("default", "aa:bb:cc:dd:ee:ff")
        .await
        .expect("block");
}

#[tokio::test]
async fn e2e_unauthorized_then_reauth_and_retry() {
    // First device call gets a 401 (session expired); the client re-logs-in and
    // retries transparently. wiremock with `up_to_n_times` sequences this.
    let server = MockServer::start().await;
    mount_login(&server).await;

    // 1st GET -> 401
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/sta"))
        .respond_with(ResponseTemplate::new(401).set_body_string(
            r#"{"meta":{"rc":"error","msg":"api.err.LoginRequired"}}"#,
        ))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    // subsequent GET -> 200
    Mock::given(method("GET"))
        .and(path("/proxy/network/api/s/default/stat/sta"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"meta":{"rc":"ok"},"data":[{"mac":"aa:bb","hostname":"phone","is_wired":false,"essid":"Home","ap_mac":"ap1"}]}"#,
        ))
        .mount(&server)
        .await;

    let metrics = Arc::new(Metrics::new());
    let client = ConsoleClient::new(
        console_for(&server),
        transport(),
        Credentials::password("admin", "secret"),
    )
    .with_metrics(Arc::clone(&metrics));
    client.login().await.expect("login");

    let api = NetworkApi::new(&client);
    let clients = api.clients("default").await.expect("clients after reauth");
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].name(), "phone");

    let prom = metrics.render_prometheus();
    assert!(prom.contains("unifi_reauth_total 1"), "{prom}");
    assert!(prom.contains("unifi_logins_total 2"), "{prom}");
}
