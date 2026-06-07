// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests for the `v5.channel.k8s.io` WebSocket streaming proxy
//! (exec / attach / port-forward) against an in-process streaming-server
//! double — the same shape containerd's CRI streaming server presents on the
//! URL the gRPC `Exec`/`Attach`/`PortForward` calls hand back.
#![cfg(feature = "remote-cri")]

use std::io::Cursor;

use cave_home_kubelet_rs::cri::remote::ws::conn::{WsConnection, V5_CHANNEL_PROTOCOL};
use cave_home_kubelet_rs::cri::remote::ws::frame::Frame;
use cave_home_kubelet_rs::cri::remote::ws::proxy::{
    channel, channel_frame, dial, run_exec, run_port_forward,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Spawn an in-process WS streaming server that runs `handler` per connection,
/// returning the `http://` URL to dial.
async fn serve<F, Fut>(handler: F) -> String
where
    F: FnOnce(WsConnection<TcpStream>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let conn = WsConnection::accept(sock, V5_CHANNEL_PROTOCOL).await.unwrap();
        handler(conn).await;
    });
    format!("http://{addr}/exec/tok")
}

#[tokio::test]
async fn exec_streams_stdin_to_stdout_and_stderr() {
    // Server: read one stdin message, echo it upper-cased on stdout, write a
    // fixed stderr line, signal a clean exit on the error channel, close.
    let url = serve(|mut conn| async move {
        let msg = conn.recv().await.unwrap().expect("stdin frame");
        let (ch, data) = (msg.payload[0], &msg.payload[1..]);
        assert_eq!(ch, channel::STDIN);
        let upper = data.to_ascii_uppercase();
        conn.send(&channel_frame(channel::STDOUT, &upper)).await.unwrap();
        conn.send(&channel_frame(channel::STDERR, b"warn: noop")).await.unwrap();
        conn.send(&channel_frame(channel::ERROR, b"")).await.unwrap();
        conn.send(&Frame::close()).await.unwrap();
    })
    .await;

    let client = dial(&url, &[V5_CHANNEL_PROTOCOL]).await.expect("dial");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = run_exec(
        client,
        Some(Cursor::new(b"hello".to_vec())),
        &mut stdout,
        &mut stderr,
        None,
    )
    .await
    .expect("run_exec");

    assert_eq!(stdout, b"HELLO");
    assert_eq!(stderr, b"warn: noop");
    assert_eq!(outcome.error.as_deref(), Some(""));
}

#[tokio::test]
async fn exec_half_closes_stdin_on_eof() {
    // Server asserts it sees the v5 CLOSE control for channel 0 after the
    // client's stdin reaches EOF.
    let url = serve(|mut conn| async move {
        let data = conn.recv().await.unwrap().expect("stdin");
        assert_eq!(&data.payload, b"\x00ping");
        let close = conn.recv().await.unwrap().expect("close control");
        assert_eq!(close.payload[0], channel::CLOSE);
        assert_eq!(close.payload[1], channel::STDIN);
        conn.send(&Frame::close()).await.unwrap();
    })
    .await;

    let client = dial(&url, &[V5_CHANNEL_PROTOCOL]).await.unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    run_exec(client, Some(Cursor::new(b"ping".to_vec())), &mut out, &mut err, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn port_forward_bridges_a_local_stream() {
    // Server echoes data-channel bytes back, after stripping the port header.
    let url = serve(|mut conn| async move {
        let first = conn.recv().await.unwrap().expect("first data frame");
        // [channel=0][port_lo][port_hi][payload...]
        assert_eq!(first.payload[0], channel::STDIN);
        let port = u16::from_le_bytes([first.payload[1], first.payload[2]]);
        assert_eq!(port, 8080);
        let echo = &first.payload[3..];
        conn.send(&channel_frame(channel::STDIN, echo)).await.unwrap();
        conn.send(&Frame::close()).await.unwrap();
    })
    .await;

    let client = dial(&url, &[V5_CHANNEL_PROTOCOL]).await.unwrap();
    let (mut local, remote) = tokio::io::duplex(4096);
    let pf = tokio::spawn(async move { run_port_forward(client, 8080, remote).await });

    local.write_all(b"GET /").await.unwrap();
    let mut buf = vec![0u8; 5];
    local.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"GET /");
    drop(local);
    let _ = pf.await.unwrap();
}
