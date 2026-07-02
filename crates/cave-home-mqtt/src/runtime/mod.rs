//! Async broker runtime (behind the `runtime` feature).
//!
//! The runtime is pure transport: it accepts TCP, TLS and WebSocket
//! connections, frames MQTT packets off the wire, and feeds them to the
//! I/O-free [`Broker`](crate::broker::Broker) decision core, applying the
//! [`Action`](crate::broker::Action)s the core returns. All protocol
//! logic lives in the core; nothing here interprets MQTT semantics.

pub mod frame;
pub mod tls;
mod ws;

use crate::broker::{Action, Broker};
use crate::v5::packet::PacketV5;
use frame::{read_packet, write_packet};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_rustls::TlsAcceptor;

/// A message bound for one connection's writer task.
enum ConnMsg {
    Packet(Box<PacketV5>),
    Close,
}

/// Shared broker state plus the registry of live connection writers,
/// keyed by client identifier. A monotonic connection id disambiguates
/// session takeover so a superseded connection cannot detach the new one.
struct Hub {
    broker: Mutex<Broker>,
    conns: Mutex<HashMap<String, (u64, mpsc::UnboundedSender<ConnMsg>)>>,
    next_conn: AtomicU64,
}

impl Hub {
    fn new(broker: Broker) -> Arc<Self> {
        Arc::new(Self {
            broker: Mutex::new(broker),
            conns: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
        })
    }

    /// Apply Send/Drop actions through the connection registry.
    async fn dispatch(&self, actions: Vec<Action>) {
        if actions.is_empty() {
            return;
        }
        let conns = self.conns.lock().await;
        for action in actions {
            match action {
                Action::Send { client_id, packet } => {
                    if let Some((_, tx)) = conns.get(&client_id) {
                        let _ = tx.send(ConnMsg::Packet(Box::new(packet)));
                    }
                }
                Action::Drop { client_id, .. } => {
                    if let Some((_, tx)) = conns.get(&client_id) {
                        let _ = tx.send(ConnMsg::Close);
                    }
                }
            }
        }
    }

    /// Process a CONNECT. Self-targeted Sends (CONNACK, resumed-queue
    /// deliveries) go straight to this connection's writer; a takeover
    /// Drop is routed through the registry to the *old* connection.
    /// Returns `(client_id, conn_id)` on success, `None` if rejected.
    async fn on_connect(
        &self,
        connect: crate::v5::packet::ConnectV5,
        tx: &mpsc::UnboundedSender<ConnMsg>,
    ) -> Option<(String, u64)> {
        let actions = { self.broker.lock().await.connect(connect) };

        let mut client_id = None;
        let mut accepted = false;
        for action in &actions {
            match action {
                Action::Drop { client_id: old, .. } => {
                    if let Some((_, old_tx)) = self.conns.lock().await.get(old) {
                        let _ = old_tx.send(ConnMsg::Close);
                    }
                }
                Action::Send { client_id: cid, packet } => {
                    if let PacketV5::ConnAck(ack) = packet {
                        client_id = Some(cid.clone());
                        accepted = ack.reason_code == crate::v5::reason::ReasonCode::Success;
                    }
                    let _ = tx.send(ConnMsg::Packet(Box::new(packet.clone())));
                }
            }
        }

        let client_id = client_id?;
        if !accepted {
            let _ = tx.send(ConnMsg::Close);
            return None;
        }
        let conn_id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        self.conns.lock().await.insert(client_id.clone(), (conn_id, tx.clone()));
        Some((client_id, conn_id))
    }

    async fn handle(&self, client_id: &str, packet: PacketV5) {
        let actions = { self.broker.lock().await.handle(client_id, packet) };
        self.dispatch(actions).await;
    }

    /// Tear down a connection. Guarded by `conn_id` so a connection that
    /// was already taken over does nothing (the new one owns the session).
    async fn disconnect(&self, client_id: &str, conn_id: u64) {
        {
            let mut conns = self.conns.lock().await;
            match conns.get(client_id) {
                Some((cid, _)) if *cid == conn_id => {
                    conns.remove(client_id);
                }
                _ => return,
            }
        }
        let actions = { self.broker.lock().await.network_disconnect(client_id) };
        self.dispatch(actions).await;
    }
}

/// Serve one byte-stream connection (TCP or TLS) to completion.
async fn serve_stream<S>(hub: Arc<Hub>, stream: S)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::unbounded_channel::<ConnMsg>();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                ConnMsg::Packet(p) => {
                    if write_packet(&mut wr, &p).await.is_err() {
                        break;
                    }
                }
                ConnMsg::Close => {
                    use tokio::io::AsyncWriteExt;
                    let _ = wr.shutdown().await;
                    break;
                }
            }
        }
    });

    let connect = match read_packet(&mut rd).await {
        Ok(Some(PacketV5::Connect(c))) => c,
        _ => {
            let _ = tx.send(ConnMsg::Close);
            let _ = writer.await;
            return;
        }
    };
    let Some((client_id, conn_id)) = hub.on_connect(connect, &tx).await else {
        let _ = writer.await;
        return;
    };

    loop {
        match read_packet(&mut rd).await {
            Ok(Some(PacketV5::Disconnect(d))) => {
                hub.handle(&client_id, PacketV5::Disconnect(d)).await;
                break;
            }
            Ok(Some(packet)) => hub.handle(&client_id, packet).await,
            Ok(None) | Err(_) => break,
        }
    }

    hub.disconnect(&client_id, conn_id).await;
    let _ = tx.send(ConnMsg::Close);
    let _ = writer.await;
}

/// The broker server. Owns shared state; each `serve_*` accept loop runs
/// until its listener errors, spawning a task per connection.
pub struct Server {
    hub: Arc<Hub>,
}

impl Server {
    pub fn new(broker: Broker) -> Self {
        Self { hub: Hub::new(broker) }
    }

    /// Accept plain MQTT-over-TCP connections (default port 1883).
    ///
    /// # Errors
    /// Returns the first fatal `accept` error from the listener.
    pub async fn serve_tcp(&self, listener: TcpListener) -> io::Result<()> {
        loop {
            let (sock, _peer) = listener.accept().await?;
            let _ = sock.set_nodelay(true);
            let hub = self.hub.clone();
            tokio::spawn(serve_stream(hub, sock));
        }
    }

    /// Accept MQTT-over-TLS connections (default port 8883).
    ///
    /// # Errors
    /// Returns the first fatal `accept` error from the listener.
    pub async fn serve_tls(&self, listener: TcpListener, acceptor: TlsAcceptor) -> io::Result<()> {
        loop {
            let (sock, _peer) = listener.accept().await?;
            let hub = self.hub.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(sock).await {
                    serve_stream(hub, tls).await;
                }
            });
        }
    }

    /// Accept MQTT-over-WebSocket connections (default port 8083).
    ///
    /// # Errors
    /// Returns the first fatal `accept` error from the listener.
    pub async fn serve_ws(&self, listener: TcpListener) -> io::Result<()> {
        loop {
            let (sock, _peer) = listener.accept().await?;
            let hub = self.hub.clone();
            tokio::spawn(ws::serve_ws(hub, sock));
        }
    }
}
