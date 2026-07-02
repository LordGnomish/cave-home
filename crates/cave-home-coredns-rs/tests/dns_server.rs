// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! End-to-end integration: a real `CoreDNS` server over real loopback sockets.
//!
//! These drive the public API exactly as the K3s node would: parse a Corefile,
//! spawn the resolver, feed it a Kubernetes API snapshot, then resolve cluster
//! names over UDP and TCP against an OS-bound socket.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

use cave_home_coredns_rs::{
    Corefile, K8sSnapshot, Message, Name, Rcode, Rdata, RecordType, Resolver, serve_tcp, serve_udp,
};

/// A Corefile a K3s node would run: a cluster zone served by the kubernetes
/// plugin, with a fallback hosts entry.
const COREFILE: &str = "cluster.local {
    kubernetes cluster.local {
        pods insecure
    }
    hosts {
        10.9.9.9 fallback.cluster.local
        fallthrough
    }
}";

/// The API's `ServiceList`: one `ClusterIP` service `web` with a named http port.
const SERVICES: &str = r#"{
    "items": [
        {
            "metadata": {"name": "web", "namespace": "default"},
            "spec": {
                "type": "ClusterIP",
                "clusterIP": "10.0.0.1",
                "ports": [{"name": "http", "protocol": "TCP", "port": 80}]
            }
        }
    ]
}"#;

const EMPTY_ENDPOINTS: &str = r#"{"items":[]}"#;

/// Build a resolver from the Corefile and seed it with the service snapshot.
async fn ready_resolver() -> Resolver {
    let block = Corefile::parse(COREFILE).unwrap().servers.pop().unwrap();
    let resolver = Resolver::spawn(block);
    resolver
        .update_endpoints(&K8sSnapshot::new(SERVICES, EMPTY_ENDPOINTS))
        .await
        .unwrap();
    resolver
}

/// One UDP query/response exchange against `addr`.
async fn udp_exchange(addr: std::net::SocketAddr, query: &Message) -> Message {
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.send_to(&query.encode(), addr).await.unwrap();
    let mut buf = [0u8; 1500];
    let n = client.recv(&mut buf).await.unwrap();
    Message::decode(&buf[..n]).unwrap()
}

/// One TCP query/response exchange against `addr` (RFC 1035 length framing).
async fn tcp_exchange(addr: std::net::SocketAddr, query: &Message) -> Message {
    let mut conn = TcpStream::connect(addr).await.unwrap();
    let bytes = query.encode();
    conn.write_all(&u16::try_from(bytes.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    conn.write_all(&bytes).await.unwrap();
    conn.flush().await.unwrap();

    let mut len_buf = [0u8; 2];
    conn.read_exact(&mut len_buf).await.unwrap();
    let mut msg = vec![0u8; u16::from_be_bytes(len_buf) as usize];
    conn.read_exact(&mut msg).await.unwrap();
    Message::decode(&msg).unwrap()
}

#[tokio::test]
async fn udp_resolves_a_cluster_service() {
    let resolver = ready_resolver().await;
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = socket.local_addr().unwrap();
    tokio::spawn(serve_udp(Arc::clone(&socket), resolver));

    let query = Message::query(
        Name::parse("web.default.svc.cluster.local").unwrap(),
        RecordType::A,
        0x4242,
    );
    let reply = udp_exchange(addr, &query).await;

    assert_eq!(reply.header.id, 0x4242);
    assert_eq!(reply.header.rcode, Rcode::NoError);
    assert!(reply.header.aa, "kubernetes answers are authoritative");
    assert_eq!(
        reply.answers[0].rdata,
        Rdata::A(std::net::Ipv4Addr::new(10, 0, 0, 1))
    );
}

#[tokio::test]
async fn tcp_resolves_a_cluster_service() {
    let resolver = ready_resolver().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_tcp(listener, resolver));

    let query = Message::query(
        Name::parse("web.default.svc.cluster.local").unwrap(),
        RecordType::A,
        0x99,
    );
    let reply = tcp_exchange(addr, &query).await;
    assert_eq!(
        reply.answers[0].rdata,
        Rdata::A(std::net::Ipv4Addr::new(10, 0, 0, 1))
    );
}

#[tokio::test]
async fn udp_and_tcp_share_one_resolver_and_agree() {
    // The same resolver handle drives both listeners; both must answer alike.
    let resolver = ready_resolver().await;
    let udp = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let udp_addr = udp.local_addr().unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    tokio::spawn(serve_udp(Arc::clone(&udp), resolver.clone()));
    tokio::spawn(serve_tcp(tcp, resolver));

    let query = Message::query(
        Name::parse("web.default.svc.cluster.local").unwrap(),
        RecordType::A,
        1,
    );
    let via_udp = udp_exchange(udp_addr, &query).await;
    let via_tcp = tcp_exchange(tcp_addr, &query).await;
    assert_eq!(via_udp.answers, via_tcp.answers);
}

#[tokio::test]
async fn srv_query_returns_the_named_port_over_tcp() {
    let resolver = ready_resolver().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_tcp(listener, resolver));

    let query = Message::query(
        Name::parse("_http._tcp.web.default.svc.cluster.local").unwrap(),
        RecordType::Srv,
        2,
    );
    let reply = tcp_exchange(addr, &query).await;
    let port = reply.answers.iter().find_map(|rr| match &rr.rdata {
        Rdata::Srv { port, .. } => Some(*port),
        _ => None,
    });
    assert_eq!(port, Some(80));
}

#[tokio::test]
async fn unknown_cluster_name_is_authoritative_nxdomain_over_udp() {
    let resolver = ready_resolver().await;
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = socket.local_addr().unwrap();
    tokio::spawn(serve_udp(Arc::clone(&socket), resolver));

    let query = Message::query(
        Name::parse("ghost.default.svc.cluster.local").unwrap(),
        RecordType::A,
        3,
    );
    let reply = udp_exchange(addr, &query).await;
    assert_eq!(reply.header.rcode, Rcode::NxDomain);
}
