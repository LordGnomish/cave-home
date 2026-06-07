// SPDX-License-Identifier: Apache-2.0
//! Async WebSocket connection: the RFC 6455 opening handshake plus message
//! framing over any [`AsyncRead`] + [`AsyncWrite`] transport.
//!
//! [`WsConnection`] is role-aware (RFC 6455 §5.3): a [`Role::Client`] masks the
//! frames it sends and a [`Role::Server`] does not. [`WsConnection::recv`]
//! transparently answers Pings with Pongs and surfaces Close as `Ok(None)`, so
//! callers only ever see application messages. [`WsConnection::recv_raw`]
//! exposes every frame (including control frames) for tests/diagnostics.

use std::io::{Error, ErrorKind, Result};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::frame::{self, Frame, OpCode};
use super::handshake::{accept_key, base64_encode};

/// The CRI streaming channel sub-protocol negotiated over the WebSocket.
pub const V5_CHANNEL_PROTOCOL: &str = "v5.channel.k8s.io";

/// Which side of the connection this endpoint is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Initiating side — masks outgoing frames.
    Client,
    /// Accepting side — sends unmasked frames.
    Server,
}

/// A framed WebSocket connection over `io`.
#[derive(Debug)]
pub struct WsConnection<S> {
    io: S,
    role: Role,
    rbuf: Vec<u8>,
    mask_seed: u32,
    /// Sub-protocol the peer selected (`Sec-WebSocket-Protocol`), if any.
    pub subprotocol: Option<String>,
}

// stub — replaced in the GREEN step
impl<S: AsyncRead + AsyncWrite + Unpin> WsConnection<S> {
    /// Perform the client opening handshake over `io`.
    ///
    /// # Errors
    /// Fails if the peer does not return `101` with a valid accept token.
    pub async fn connect(
        _io: S,
        _host: &str,
        _path: &str,
        _subprotocols: &[&str],
    ) -> Result<Self> {
        Err(Error::new(ErrorKind::Other, "unimplemented"))
    }

    /// Perform the server side of the opening handshake over `io`.
    ///
    /// # Errors
    /// Fails if the request is not a valid WebSocket upgrade.
    pub async fn accept(_io: S, _subprotocol: &str) -> Result<Self> {
        Err(Error::new(ErrorKind::Other, "unimplemented"))
    }

    /// Send one message frame (masked iff this side is a client).
    ///
    /// # Errors
    /// Propagates the underlying write error.
    pub async fn send(&mut self, _frame: &Frame) -> Result<()> {
        Ok(())
    }

    /// Receive the next frame of any kind (including control frames).
    /// `Ok(None)` on clean EOF.
    ///
    /// # Errors
    /// Propagates read errors and frame-decode protocol errors.
    pub async fn recv_raw(&mut self) -> Result<Option<Frame>> {
        Ok(None)
    }

    /// Receive the next application message. `Ok(None)` on clean close/EOF.
    /// Ping frames are answered with Pong transparently.
    ///
    /// # Errors
    /// Propagates read errors and frame-decode protocol errors.
    pub async fn recv(&mut self) -> Result<Option<Frame>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    async fn server(io: DuplexStream) -> WsConnection<DuplexStream> {
        WsConnection::accept(io, V5_CHANNEL_PROTOCOL).await.expect("accept")
    }

    #[tokio::test]
    async fn handshake_negotiates_subprotocol_and_echoes() {
        let (c, s) = tokio::io::duplex(64 * 1024);
        let srv = tokio::spawn(async move {
            let mut conn = server(s).await;
            // Echo the first binary message back, then close.
            let msg = conn.recv().await.unwrap().expect("a frame");
            conn.send(&Frame::binary(msg.payload)).await.unwrap();
            conn.send(&Frame::close()).await.unwrap();
        });

        let mut client =
            WsConnection::connect(c, "runtime.local", "/exec/tok", &[V5_CHANNEL_PROTOCOL])
                .await
                .expect("client handshake");
        assert_eq!(client.subprotocol.as_deref(), Some(V5_CHANNEL_PROTOCOL));

        client.send(&Frame::binary(b"\x00hello".to_vec())).await.unwrap();
        let echo = client.recv().await.unwrap().expect("echo");
        assert_eq!(echo.payload, b"\x00hello");

        // The server's close yields a clean end-of-stream.
        assert_eq!(client.recv().await.unwrap(), None);
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn large_payload_survives_chunked_reads() {
        let (c, s) = tokio::io::duplex(8 * 1024); // small pipe forces fragmentation
        let big = vec![0x5Au8; 200_000];
        let expect = big.clone();
        let srv = tokio::spawn(async move {
            let mut conn = server(s).await;
            let msg = conn.recv().await.unwrap().expect("frame");
            conn.send(&Frame::binary(msg.payload)).await.unwrap();
        });
        let mut client =
            WsConnection::connect(c, "h", "/p", &[V5_CHANNEL_PROTOCOL]).await.unwrap();
        client.send(&Frame::binary(big)).await.unwrap();
        let echo = client.recv().await.unwrap().unwrap();
        assert_eq!(echo.payload, expect);
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn ping_is_answered_with_pong_then_data_delivered() {
        let (c, s) = tokio::io::duplex(64 * 1024);
        let srv = tokio::spawn(async move {
            let mut conn = server(s).await;
            // Server pings, then expects the client's transparent pong, then
            // sends a data frame the client must surface.
            conn.send(&Frame { fin: true, opcode: OpCode::Ping, payload: b"hb".to_vec() })
                .await
                .unwrap();
            let pong = conn.recv_raw().await.unwrap().expect("pong");
            assert_eq!(pong.opcode, OpCode::Pong);
            assert_eq!(pong.payload, b"hb");
            conn.send(&Frame::binary(b"after".to_vec())).await.unwrap();
        });
        let mut client =
            WsConnection::connect(c, "h", "/p", &[V5_CHANNEL_PROTOCOL]).await.unwrap();
        let data = client.recv().await.unwrap().expect("data");
        assert_eq!(data.payload, b"after");
        srv.await.unwrap();
    }
}
