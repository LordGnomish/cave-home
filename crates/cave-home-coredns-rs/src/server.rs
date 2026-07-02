// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The real DNS server: UDP/TCP listeners and the resolver actor.
//!
//! This is the I/O shell the decision core was built to slot into — `CoreDNS`'s
//! `core/dnsserver/server_udp.go` and `server_tcp.go`. Three pieces:
//!
//! * [`Resolver`] — the single owner of the live [`crate::plugin::Chain`]. The chain is built
//!   from plugins that carry interior-mutable counters and are therefore
//!   `!Send`, so it cannot be shared across worker threads. Instead one
//!   resolver task owns it on a dedicated current-thread runtime; listeners
//!   hand it decoded queries over a channel and await the reply. This also
//!   gives [`Resolver::reload`] a natural home: the owner simply rebuilds its
//!   chain from a new [`ServerBlock`] between queries.
//! * [`serve_udp`] — RFC 1035 §4.2.1 datagram service, including the 512-octet
//!   limit and the `TC` truncation bit for oversized replies.
//! * [`serve_tcp`] — RFC 1035 §4.2.2 / RFC 7766 stream service with the 2-octet
//!   length prefix, handling multiple queries per connection.
//!
//! EDNS0 buffer-size negotiation (RFC 6891) is deferred with the rest of the
//! crate's EDNS support; the UDP path uses the classic 512-octet limit.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, oneshot};

use crate::build::build_chain_with;
use crate::corefile::ServerBlock;
use crate::error::{Result, WireError};
use crate::k8s::K8sSnapshot;
use crate::message::Message;
use crate::wire::Rcode;

/// The maximum DNS message size on classic UDP, without EDNS0 (RFC 1035
/// §4.2.1). A reply larger than this is truncated with the `TC` bit set so the
/// client retries over TCP.
pub const MAX_UDP_PAYLOAD: usize = 512;

/// The depth of the resolver's request queue.
const RESOLVER_BACKLOG: usize = 1024;

/// A command sent to the resolver actor. Boxed payloads keep the enum small.
enum Cmd {
    /// Resolve a decoded query; the reply goes back on the channel.
    Resolve(Box<Message>, oneshot::Sender<Message>),
    /// Replace the server block and rebuild; the build result acks back.
    Reload(Box<ServerBlock>, oneshot::Sender<Result<()>>),
    /// Replace the Kubernetes snapshot and rebuild; the result acks back.
    UpdateEndpoints(Box<K8sSnapshot>, oneshot::Sender<Result<()>>),
}

/// A cloneable handle to the single-owner resolver actor.
///
/// Cloning is cheap (it clones the channel sender); every clone dispatches to
/// the same chain, so a `metrics` plugin counts UDP and TCP queries together,
/// exactly as in `CoreDNS`.
#[derive(Clone)]
pub struct Resolver {
    tx: mpsc::Sender<Cmd>,
}

impl Resolver {
    /// Spawn the resolver actor, owning a chain built from `block`.
    ///
    /// The chain is `!Send`, so it lives on a dedicated thread running its own
    /// current-thread runtime; this handle (and its clones) talk to it over a
    /// channel from any runtime. A `block` that fails to build yields a
    /// resolver that answers `SERVFAIL` until a successful [`Resolver::reload`];
    /// validate up front with [`build_chain`](crate::build::build_chain) if a
    /// hard failure is wanted.
    #[must_use]
    pub fn spawn(block: ServerBlock) -> Self {
        let (tx, mut rx) = mpsc::channel::<Cmd>(RESOLVER_BACKLOG);
        let spawned = std::thread::Builder::new()
            .name("coredns-resolver".to_string())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async move {
                    // The owner keeps the inputs the chain is built from — the
                    // server block and the latest Kubernetes snapshot — and
                    // rebuilds whenever either changes. An unbuildable initial
                    // config degrades to an empty chain (SERVFAIL) until a
                    // successful reload/update recovers it.
                    let mut block = block;
                    let mut snapshot: Option<K8sSnapshot> = None;
                    let mut chain = build_chain_with(&block, snapshot.as_ref()).unwrap_or_default();
                    while let Some(cmd) = rx.recv().await {
                        match cmd {
                            Cmd::Resolve(query, reply) => {
                                let _ = reply.send(chain.handle(&query));
                            }
                            Cmd::Reload(new_block, ack) => {
                                match build_chain_with(&new_block, snapshot.as_ref()) {
                                    Ok(new_chain) => {
                                        block = *new_block;
                                        chain = new_chain;
                                        let _ = ack.send(Ok(()));
                                    }
                                    Err(e) => {
                                        let _ = ack.send(Err(e));
                                    }
                                }
                            }
                            Cmd::UpdateEndpoints(snap, ack) => {
                                match build_chain_with(&block, Some(&snap)) {
                                    Ok(new_chain) => {
                                        snapshot = Some(*snap);
                                        chain = new_chain;
                                        let _ = ack.send(Ok(()));
                                    }
                                    Err(e) => {
                                        let _ = ack.send(Err(e));
                                    }
                                }
                            }
                        }
                    }
                });
            });
        // If the OS refused the thread, the sender's receiver is dropped and
        // every resolve falls back to SERVFAIL — never a panic.
        let _ = spawned;
        Self { tx }
    }

    /// Resolve a decoded query, returning the reply to put on the wire.
    ///
    /// If the resolver task is gone, the query is answered `SERVFAIL` (Charter
    /// §6.3: infrastructure failures are DNS rcodes, never panics).
    pub async fn resolve(&self, query: Message) -> Message {
        let fallback = query.reply().with_rcode(Rcode::ServFail);
        let (otx, orx) = oneshot::channel();
        if self
            .tx
            .send(Cmd::Resolve(Box::new(query), otx))
            .await
            .is_err()
        {
            return fallback;
        }
        orx.await.unwrap_or(fallback)
    }

    /// Hot-reload the chain from a new server block (`CoreDNS`'s `reload`).
    ///
    /// The swap happens between queries on the owner task; in-flight queries
    /// finish against the old chain, later queries see the new one.
    ///
    /// # Errors
    /// [`WireError::Config`] if the new block cannot be lowered (the old chain
    /// is kept), or if the resolver task has stopped.
    pub async fn reload(&self, block: &ServerBlock) -> Result<()> {
        let (atx, arx) = oneshot::channel();
        self.tx
            .send(Cmd::Reload(Box::new(block.clone()), atx))
            .await
            .map_err(|_| WireError::Config {
                reason: "resolver stopped",
            })?;
        arx.await.map_err(|_| WireError::Config {
            reason: "resolver stopped",
        })?
    }

    /// Push a new Kubernetes API snapshot, rebuilding the chain so the running
    /// server resolves the cluster's current services. This is how a watch
    /// update reaches the live server (`CoreDNS`'s informer-driven plugin
    /// update, expressed as a chain rebuild for the immutable plugin here).
    ///
    /// # Errors
    /// [`WireError::Config`] if the snapshot fails to convert (the old chain is
    /// kept), or if the resolver task has stopped.
    pub async fn update_endpoints(&self, snapshot: &K8sSnapshot) -> Result<()> {
        let (atx, arx) = oneshot::channel();
        self.tx
            .send(Cmd::UpdateEndpoints(Box::new(snapshot.clone()), atx))
            .await
            .map_err(|_| WireError::Config {
                reason: "resolver stopped",
            })?;
        arx.await.map_err(|_| WireError::Config {
            reason: "resolver stopped",
        })?
    }
}

/// Render the reply for the UDP wire, enforcing the 512-octet limit.
///
/// If the full reply exceeds [`MAX_UDP_PAYLOAD`], the answer/authority/
/// additional sections are dropped and the `TC` bit set, telling the client to
/// retry over TCP (RFC 1035 §4.2.1).
fn truncate_for_udp(reply: &Message) -> Vec<u8> {
    let full = reply.encode();
    if full.len() <= MAX_UDP_PAYLOAD {
        return full;
    }
    let mut truncated = reply.clone();
    truncated.header.tc = true;
    truncated.answers.clear();
    truncated.authority.clear();
    truncated.additional.clear();
    truncated.encode()
}

/// Build a `FORMERR` reply for an undecodable query, echoing its id if the
/// buffer is long enough to carry one. Returns `None` for a runt too short to
/// hold even the 2-octet id (nothing useful to send back).
fn formerr_bytes(raw: &[u8]) -> Option<Vec<u8>> {
    let id = u16::from_be_bytes([*raw.first()?, *raw.get(1)?]);
    let mut m = Message::empty(id);
    m.header.qr = true;
    m.header.rcode = Rcode::FormErr;
    Some(m.encode())
}

/// Produce the reply bytes for one UDP datagram, or `None` to send nothing.
async fn handle_datagram(raw: &[u8], resolver: &Resolver) -> Option<Vec<u8>> {
    match Message::decode(raw) {
        Ok(query) => {
            let reply = resolver.resolve(query).await;
            Some(truncate_for_udp(&reply))
        }
        Err(_) => formerr_bytes(raw),
    }
}

/// Serve DNS over UDP on `socket` until an unrecoverable socket error.
///
/// Each datagram is handled on its own task so a slow resolve never stalls the
/// receive loop.
///
/// # Errors
/// Propagates a fatal [`std::io::Error`] from `recv_from`.
pub async fn serve_udp(socket: Arc<UdpSocket>, resolver: Resolver) -> std::io::Result<()> {
    // 64 KiB is the largest a UDP datagram can be; DNS-over-UDP is far smaller,
    // but sizing for the maximum means a jumbo query is read whole (then
    // rejected by the codec) rather than silently clipped.
    let mut buf = vec![0u8; 65_535];
    loop {
        let (n, peer) = socket.recv_from(&mut buf).await?;
        let datagram = buf[..n].to_vec();
        let socket = Arc::clone(&socket);
        let resolver = resolver.clone();
        tokio::spawn(async move {
            if let Some(reply) = handle_datagram(&datagram, &resolver).await {
                let _ = socket.send_to(&reply, peer).await;
            }
        });
    }
}

/// Serve DNS over TCP on `listener` until an unrecoverable accept error.
///
/// Each connection is handled on its own task and may carry multiple queries
/// (RFC 7766 connection reuse).
///
/// # Errors
/// Propagates a fatal [`std::io::Error`] from `accept`.
pub async fn serve_tcp(listener: TcpListener, resolver: Resolver) -> std::io::Result<()> {
    loop {
        let (stream, _peer) = listener.accept().await?;
        let resolver = resolver.clone();
        tokio::spawn(async move {
            let _ = serve_tcp_conn(stream, resolver).await;
        });
    }
}

/// Drive one TCP connection: read length-prefixed queries until EOF.
async fn serve_tcp_conn(mut stream: TcpStream, resolver: Resolver) -> std::io::Result<()> {
    loop {
        // RFC 1035 §4.2.2: each message is prefixed by its 2-octet length.
        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            return Ok(()); // clean EOF or reset between messages
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut msg = vec![0u8; len];
        stream.read_exact(&mut msg).await?;

        let reply = match Message::decode(&msg) {
            Ok(query) => resolver.resolve(query).await.encode(),
            // TCP carries full replies, so no truncation here.
            Err(_) => match formerr_bytes(&msg) {
                Some(bytes) => bytes,
                None => return Ok(()),
            },
        };
        let reply_len = u16::try_from(reply.len()).unwrap_or(u16::MAX).to_be_bytes();
        stream.write_all(&reply_len).await?;
        stream.write_all(&reply).await?;
        stream.flush().await?;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::corefile::Corefile;
    use crate::name::Name;
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use std::net::Ipv4Addr;

    fn block(text: &str) -> ServerBlock {
        Corefile::parse(text).unwrap().servers.pop().unwrap()
    }

    fn a_query(name: &str) -> Message {
        Message::query(Name::parse(name).unwrap(), RecordType::A, 0x1234)
    }

    #[tokio::test]
    async fn resolver_answers_from_its_chain() {
        let r = Resolver::spawn(block(". {\n hosts {\n 10.0.0.1 web.local \n } \n}"));
        let reply = r.resolve(a_query("web.local")).await;
        assert_eq!(reply.header.rcode, Rcode::NoError);
        assert_eq!(reply.answers.len(), 1);
        assert_eq!(reply.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[tokio::test]
    async fn reload_swaps_the_live_chain() {
        let r = Resolver::spawn(block(". {\n hosts {\n 10.0.0.1 web.local \n } \n}"));
        assert_eq!(
            r.resolve(a_query("web.local")).await.answers[0].rdata,
            Rdata::A(Ipv4Addr::new(10, 0, 0, 1))
        );
        r.reload(&block(". {\n hosts {\n 10.0.0.2 web.local \n } \n}"))
            .await
            .unwrap();
        assert_eq!(
            r.resolve(a_query("web.local")).await.answers[0].rdata,
            Rdata::A(Ipv4Addr::new(10, 0, 0, 2)),
            "reload must take effect for subsequent queries"
        );
    }

    #[tokio::test]
    async fn reload_with_a_bad_block_keeps_the_old_chain() {
        let r = Resolver::spawn(block(". {\n hosts {\n 10.0.0.1 web.local \n } \n}"));
        assert!(r.reload(&block(". {\n dnssec \n}")).await.is_err());
        // Old chain still answers.
        assert_eq!(r.resolve(a_query("web.local")).await.answers.len(), 1);
    }

    #[tokio::test]
    async fn udp_query_round_trips_over_a_real_socket() {
        let resolver = Resolver::spawn(block(". {\n hosts {\n 10.0.0.7 web.local \n } \n}"));
        let server = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = server.local_addr().unwrap();
        tokio::spawn(serve_udp(Arc::clone(&server), resolver));

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(&a_query("web.local").encode(), addr)
            .await
            .unwrap();
        let mut buf = [0u8; 512];
        let n = client.recv(&mut buf).await.unwrap();

        let reply = Message::decode(&buf[..n]).unwrap();
        assert_eq!(reply.header.id, 0x1234);
        assert_eq!(reply.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 7)));
    }

    #[tokio::test]
    async fn tcp_query_round_trips_with_a_length_prefix() {
        let resolver = Resolver::spawn(block(". {\n hosts {\n 10.0.0.8 web.local \n } \n}"));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_tcp(listener, resolver));

        let mut conn = TcpStream::connect(addr).await.unwrap();
        let query = a_query("web.local").encode();
        conn.write_all(&u16::try_from(query.len()).unwrap().to_be_bytes())
            .await
            .unwrap();
        conn.write_all(&query).await.unwrap();
        conn.flush().await.unwrap();

        let mut len_buf = [0u8; 2];
        conn.read_exact(&mut len_buf).await.unwrap();
        let len = u16::from_be_bytes(len_buf) as usize;
        let mut msg = vec![0u8; len];
        conn.read_exact(&mut msg).await.unwrap();

        let reply = Message::decode(&msg).unwrap();
        assert_eq!(reply.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 8)));
    }

    #[tokio::test]
    async fn tcp_connection_serves_multiple_queries() {
        let resolver = Resolver::spawn(block(". {\n hosts {\n 10.0.0.9 web.local \n } \n}"));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_tcp(listener, resolver));
        let mut conn = TcpStream::connect(addr).await.unwrap();

        for _ in 0..3 {
            let query = a_query("web.local").encode();
            conn.write_all(&u16::try_from(query.len()).unwrap().to_be_bytes())
                .await
                .unwrap();
            conn.write_all(&query).await.unwrap();
            conn.flush().await.unwrap();
            let mut len_buf = [0u8; 2];
            conn.read_exact(&mut len_buf).await.unwrap();
            let mut msg = vec![0u8; u16::from_be_bytes(len_buf) as usize];
            conn.read_exact(&mut msg).await.unwrap();
            assert_eq!(Message::decode(&msg).unwrap().answers.len(), 1);
        }
    }

    #[test]
    fn oversized_reply_is_truncated_with_tc_set() {
        // 64 A records (~16 octets each on the wire) blow past 512.
        let mut reply = a_query("big.local").reply();
        for i in 0..64u8 {
            reply.answers.push(ResourceRecord::new(
                Name::parse("big.local").unwrap(),
                Class::In,
                30,
                Rdata::A(Ipv4Addr::new(10, 0, 0, i)),
            ));
        }
        assert!(reply.encode().len() > MAX_UDP_PAYLOAD);

        let bytes = truncate_for_udp(&reply);
        assert!(
            bytes.len() <= MAX_UDP_PAYLOAD,
            "must fit the 512-octet limit"
        );
        let decoded = Message::decode(&bytes).unwrap();
        assert!(decoded.header.tc, "TC must be set on truncation");
        assert!(decoded.answers.is_empty(), "answers dropped on truncation");
    }

    #[test]
    fn small_reply_is_not_truncated() {
        let mut reply = a_query("web.local").reply();
        reply.answers.push(ResourceRecord::new(
            Name::parse("web.local").unwrap(),
            Class::In,
            30,
            Rdata::A(Ipv4Addr::new(10, 0, 0, 1)),
        ));
        let decoded = Message::decode(&truncate_for_udp(&reply)).unwrap();
        assert!(!decoded.header.tc);
        assert_eq!(decoded.answers.len(), 1);
    }

    #[test]
    fn malformed_query_yields_formerr_echoing_the_id() {
        // A 4-byte runt: enough for an id, too short for a header.
        let raw = [0xAB, 0xCD, 0x00, 0x00];
        let bytes = formerr_bytes(&raw).unwrap();
        let reply = Message::decode(&bytes).unwrap();
        assert_eq!(reply.header.id, 0xABCD);
        assert_eq!(reply.header.rcode, Rcode::FormErr);
        assert!(reply.header.qr);
    }

    #[test]
    fn a_one_byte_runt_has_no_reply() {
        assert!(formerr_bytes(&[0x00]).is_none());
    }

    #[tokio::test]
    async fn update_endpoints_feeds_the_running_kubernetes_plugin() {
        use crate::k8s::K8sSnapshot;

        let r = Resolver::spawn(block("cluster.local {\n kubernetes cluster.local \n}"));
        // Before the snapshot, the service is unknown (authoritative NXDOMAIN).
        let svc = Message::query(
            Name::parse("web.default.svc.cluster.local").unwrap(),
            RecordType::A,
            7,
        );
        assert_eq!(r.resolve(svc.clone()).await.header.rcode, Rcode::NxDomain);

        // Push a live snapshot; the running chain now resolves the service.
        r.update_endpoints(&K8sSnapshot::new(
            r#"{"items":[{"metadata":{"name":"web","namespace":"default"},
                "spec":{"type":"ClusterIP","clusterIP":"10.0.0.5"}}]}"#,
            r#"{"items":[]}"#,
        ))
        .await
        .unwrap();
        assert_eq!(
            r.resolve(svc).await.answers[0].rdata,
            Rdata::A(Ipv4Addr::new(10, 0, 0, 5))
        );
    }
}
