// SPDX-License-Identifier: Apache-2.0
//! Exec / Attach / PortForward URL negotiation over gRPC.
//!
//! Per the CRI design these RPCs do not stream bytes themselves: they return a
//! URL that the kubelet's streaming client subsequently dials (SPDY/WebSocket).
//! These tests cover the gRPC half — obtaining the URL. The byte-streaming
//! dialer is tracked as deferred (see the crate handoff / parity manifest).
#![cfg(feature = "remote-cri")]

mod common;

use std::io::Cursor;

use cave_home_kubelet_rs::cri::remote::streaming::{AttachRequest, ExecRequest, PortForwardRequest};
use cave_home_kubelet_rs::cri::remote::ws::conn::{WsConnection, V5_CHANNEL_PROTOCOL};
use cave_home_kubelet_rs::cri::remote::ws::frame::Frame;
use cave_home_kubelet_rs::cri::remote::ws::proxy::{channel, channel_frame};
use cave_home_kubelet_rs::cri::remote::RemoteCriClient;
use cave_home_kubelet_rs::cri::types as t;
use cave_home_kubelet_rs::cri::CriClient;
use tokio::net::{TcpListener, TcpStream};

use common::start_mock_cri_server;

async fn running_container(client: &RemoteCriClient) -> (String, String) {
    let sandbox_cfg = t::PodSandboxConfig {
        metadata: t::PodSandboxMetadata {
            name: "web".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let sandbox_id = client.run_pod_sandbox(sandbox_cfg.clone()).await.unwrap();
    let cfg = t::ContainerConfig {
        metadata: t::ContainerMetadata {
            name: "app".into(),
            attempt: 0,
        },
        ..Default::default()
    };
    let container_id = client
        .create_container(&sandbox_id, cfg, sandbox_cfg)
        .await
        .unwrap();
    client.start_container(&container_id).await.unwrap();
    (sandbox_id, container_id)
}

#[tokio::test]
async fn exec_returns_streaming_url() {
    let server = start_mock_cri_server().await;
    let client = RemoteCriClient::connect_uds(&server.socket_path).await.unwrap();
    let (_sb, cid) = running_container(&client).await;

    let url = client
        .exec(ExecRequest {
            container_id: cid.clone(),
            cmd: vec!["ls".into(), "-la".into()],
            tty: false,
            stdin: false,
            stdout: true,
            stderr: true,
        })
        .await
        .unwrap();
    assert!(url.starts_with("http"), "url = {url}");
    assert!(url.contains("exec"), "url = {url}");
    assert!(url.contains(&cid), "url = {url}");
}

#[tokio::test]
async fn attach_returns_streaming_url() {
    let server = start_mock_cri_server().await;
    let client = RemoteCriClient::connect_uds(&server.socket_path).await.unwrap();
    let (_sb, cid) = running_container(&client).await;

    let url = client
        .attach(AttachRequest {
            container_id: cid.clone(),
            stdin: true,
            tty: true,
            stdout: true,
            stderr: false,
        })
        .await
        .unwrap();
    assert!(url.contains("attach"), "url = {url}");
    assert!(url.contains(&cid), "url = {url}");
}

/// Spawn a one-shot in-process WS streaming server that upper-cases the first
/// stdin message onto stdout, then closes; returns the `http://` URL to dial.
async fn spawn_ws_echo_upper() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut conn: WsConnection<TcpStream> =
            WsConnection::accept(sock, V5_CHANNEL_PROTOCOL).await.unwrap();
        let msg = conn.recv().await.unwrap().expect("stdin");
        let upper = msg.payload[1..].to_ascii_uppercase();
        conn.send(&channel_frame(channel::STDOUT, &upper)).await.unwrap();
        conn.send(&channel_frame(channel::ERROR, b"")).await.unwrap();
        conn.send(&Frame::close()).await.unwrap();
        // Polite close: keep the socket open until the client finishes reading
        // (drain its remaining frames to EOF) so it never sees a TCP reset.
        while let Ok(Some(_)) = conn.recv().await {}
    });
    format!("http://{addr}/exec/streamed")
}

/// Headline streaming acceptance: negotiate the URL over gRPC, then dial it
/// over WebSocket and bridge stdio — the full kubelet exec path.
#[tokio::test]
async fn exec_streamed_negotiates_then_bridges_stdio() {
    let server = start_mock_cri_server().await;
    server.runtime.set_stream_url(spawn_ws_echo_upper().await);
    let client = RemoteCriClient::connect_uds(&server.socket_path).await.unwrap();
    let (_sb, cid) = running_container(&client).await;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = client
        .exec_streamed(
            ExecRequest {
                container_id: cid,
                cmd: vec!["echo".into(), "hi".into()],
                stdout: true,
                stderr: true,
                ..Default::default()
            },
            Some(Cursor::new(b"streamed".to_vec())),
            &mut stdout,
            &mut stderr,
            None,
        )
        .await
        .expect("exec_streamed");

    assert_eq!(stdout, b"STREAMED");
    assert_eq!(outcome.error.as_deref(), Some(""));
}

#[tokio::test]
async fn port_forward_returns_streaming_url() {
    let server = start_mock_cri_server().await;
    let client = RemoteCriClient::connect_uds(&server.socket_path).await.unwrap();
    let (sandbox_id, _cid) = running_container(&client).await;

    let url = client
        .port_forward(PortForwardRequest {
            pod_sandbox_id: sandbox_id.clone(),
            ports: vec![8080, 9090],
        })
        .await
        .unwrap();
    assert!(url.contains("portforward"), "url = {url}");
    assert!(url.contains(&sandbox_id), "url = {url}");
}
