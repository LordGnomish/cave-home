// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::expect_used)] // test harness: a failed expect is the test failing
//! End-to-end JSON write-path test against the in-process booted control plane.
//!
//! This is the real `kubectl apply` shape exercised over a real socket: boot the
//! whole single-binary runtime ([`run_until_on_listener`]) — apiserver listener,
//! the supervised scheduler + kubelet reconcilers, the seeded store — then create
//! a Pod by posting JSON over TCP. The test asserts the object is persisted
//! (readable back via the wire GET) and that the supervised control loops then
//! drive it to `Running` through the mock CRI, with the container statuses the
//! kubelet writes back. Nothing is faked: the bytes go over a loopback socket
//! into the same `apirest::handle` write path a real client hits.

use std::time::Duration;

use cave_home_binary::node::LocalNode;
use cave_home_binary::server::{run_until_on_listener, RuntimeConfig};
use cave_home_orchestration::role::NodeIntent;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Send one raw HTTP request to `addr` and return `(status_line, body)`.
async fn request(addr: std::net::SocketAddr, raw: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(raw.as_bytes()).await.expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status_line = text.lines().next().unwrap_or_default().to_string();
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    (status_line, body)
}

async fn get(addr: std::net::SocketAddr, path: &str) -> (String, String) {
    request(addr, &format!("GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pod_created_over_the_wire_is_persisted_and_reconciled_to_running() {
    // Bind first so the test learns the ephemeral port, then hand the listener
    // to the booted runtime.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    let cfg = RuntimeConfig {
        intent: NodeIntent::PrimaryHub,
        node: LocalNode::new("hub-01", "127.0.0.1"),
        bind_addr: "127.0.0.1".to_string(),
        bind_port: addr.port(),
        // Fast reconcile so schedule→run happens within the test deadline.
        reconcile_interval: Duration::from_millis(25),
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

    // Wait until the apiserver answers readiness over the wire.
    let ready = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(mut s) = TcpStream::connect(addr).await {
                let _ = s
                    .write_all(b"GET /healthz HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
                    .await;
                let mut buf = Vec::new();
                if s.read_to_end(&mut buf).await.is_ok() && String::from_utf8_lossy(&buf).contains("200") {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(ready.is_ok(), "apiserver never became ready");

    // 1. Create a Pod via the JSON write path (POST to the collection) — exactly
    //    what `kubectl apply`/`kubectl run` does. No nodeName, no status: the
    //    supervised scheduler + kubelet must fill those in.
    let pod = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"web"},"spec":{"containers":[{"name":"app","image":"nginx:1.27"}]}}"#;
    let post = format!(
        "POST /api/v1/namespaces/default/pods HTTP/1.1\r\nHost: t\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        pod.len(),
        pod
    );
    let (status, body) = request(addr, &post).await;
    assert!(status.contains("201"), "create should return 201 Created: {status} / {body}");
    assert!(body.contains("\"name\":\"web\""), "created object echoed: {body}");
    // The server stamped a uid + injected the path namespace — it really persisted.
    assert!(body.contains("\"uid\":\"uid-"), "server stamped a uid: {body}");
    assert!(body.contains("\"namespace\":\"default\""), "path namespace injected: {body}");

    // 2. It is immediately readable back over the wire (persisted in the store).
    let (gstatus, gbody) = get(addr, "/api/v1/namespaces/default/pods/web").await;
    assert!(gstatus.contains("200"), "GET after create: {gstatus} / {gbody}");
    assert!(gbody.contains("\"kind\":\"Pod\""), "{gbody}");

    // 3. The supervised scheduler binds it and the kubelet runs it: poll until
    //    the apiserver reports Running with the kubelet's container status.
    let running = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (_s, b) = get(addr, "/api/v1/namespaces/default/pods/web").await;
            if b.contains("\"phase\":\"Running\"") {
                return b;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("pod reached Running within the deadline");

    // The reconcile really happened end-to-end: bound to this node, container
    // started by the (mock) CRI, status written back through the apiserver.
    assert!(running.contains("\"nodeName\":\"hub-01\""), "scheduler bound the pod: {running}");
    assert!(running.contains("\"phase\":\"Running\""), "{running}");
    assert!(running.contains("\"containerStatuses\""), "kubelet wrote container status: {running}");
    assert!(running.contains("\"ready\":true"), "container reported ready: {running}");

    // 4. It also appears in the collection listing (the write landed in the store
    //    the list path reads).
    let (_ls, lbody) = get(addr, "/api/v1/pods").await;
    assert!(lbody.contains("\"kind\":\"PodList\""), "{lbody}");
    assert!(lbody.contains("\"name\":\"web\""), "created pod listed: {lbody}");

    // Graceful shutdown: signal, then the runtime drains + tears down in order.
    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("runtime shuts down within the deadline");
}
