// SPDX-License-Identifier: Apache-2.0
//! UDP transport.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/transport/raw/UDP.cpp
//!
//! Phase 1 wraps `tokio::net::UdpSocket` and exposes the chip-side
//! Send/Receive surface verbatim. The Matter framing (message header
//! + counter + nonce) is the responsibility of higher layers
//! (`MessageHeader.cpp`), which Phase 1 leaves to the secure-channel
//! crypto in `pase.rs` / `case.rs`.

use std::net::SocketAddr;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::error::{MatterError, Result};
use crate::transport::Transport;

/// Matter operational port — `kCHIPPort`.
///
/// # Upstream: src/lib/core/CHIPConfig.h::kChipPort
pub const MATTER_PORT: u16 = 5540;

/// Default Matter multicast address for IPv6 commissioner discovery.
///
/// # Upstream: src/transport/raw/UDP.h::kIPv6AllNodesAddress
pub const MATTER_MCAST_V6: &str = "ff05::1";

/// UDP transport.
pub struct UdpTransport {
    socket: UdpSocket,
    last_peer: Mutex<Option<SocketAddr>>,
}

impl UdpTransport {
    /// Bind to `local_addr` (typically `[::]:5540`).
    ///
    /// # Upstream: src/transport/raw/UDP.cpp::UDP::Init
    pub async fn bind(local_addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(local_addr)
            .await
            .map_err(|e| MatterError::Transport(format!("UDP bind {local_addr}: {e}")))?;
        Ok(Self {
            socket,
            last_peer: Mutex::new(None),
        })
    }

    /// Local bound address.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket
            .local_addr()
            .map_err(|e| MatterError::Transport(format!("UDP local_addr: {e}")))
    }
}

#[async_trait]
impl Transport for UdpTransport {
    async fn send(&self, peer: &str, frame: &[u8]) -> Result<()> {
        let addr: SocketAddr = peer.parse().map_err(|e| {
            MatterError::Transport(format!("UDP send: bad peer {peer:?}: {e}"))
        })?;
        self.socket
            .send_to(frame, addr)
            .await
            .map(|_| ())
            .map_err(|e| MatterError::Transport(format!("UDP send: {e}")))
    }

    async fn recv(&self) -> Result<(String, Vec<u8>)> {
        let mut buf = vec![0u8; 1280]; // Matter MaxMessageSize.
        let (n, peer) = self
            .socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| MatterError::Transport(format!("UDP recv: {e}")))?;
        buf.truncate(n);
        *self.last_peer.lock().await = Some(peer);
        Ok((peer.to_string(), buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn udp_send_recv_round_trip() {
        let server = UdpTransport::bind("127.0.0.1:0").await.expect("bind server");
        let client = UdpTransport::bind("127.0.0.1:0").await.expect("bind client");
        let server_addr = server.local_addr().expect("local").to_string();
        let payload = b"hello matter";
        client.send(&server_addr, payload).await.expect("send");
        let (peer, got) = server.recv().await.expect("recv");
        assert_eq!(got, payload);
        assert!(peer.contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn udp_send_rejects_bad_addr() {
        let s = UdpTransport::bind("127.0.0.1:0").await.expect("bind");
        let err = s.send("not-an-addr", b"x").await.expect_err("must fail");
        match err {
            MatterError::Transport(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }
}
