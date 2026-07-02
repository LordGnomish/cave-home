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

/// Read from `io` until the HTTP header terminator (`\r\n\r\n`); return the
/// header text and stash any bytes that arrived after it into `rbuf` (they
/// belong to the post-upgrade frame stream).
async fn read_headers<S: AsyncRead + Unpin>(io: &mut S, rbuf: &mut Vec<u8>) -> Result<String> {
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(pos) = rbuf.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&rbuf[..pos]).into_owned();
            rbuf.drain(..pos + 4);
            return Ok(headers);
        }
        let n = io.read(&mut chunk).await?;
        if n == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "eof during ws handshake",
            ));
        }
        rbuf.extend_from_slice(&chunk[..n]);
    }
}

/// Case-insensitively fetch a header value from a raw header block.
fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

impl<S: AsyncRead + AsyncWrite + Unpin> WsConnection<S> {
    /// Perform the client opening handshake over `io`.
    ///
    /// # Errors
    /// Fails if the peer does not return `101` with a valid accept token.
    pub async fn connect(mut io: S, host: &str, path: &str, subprotocols: &[&str]) -> Result<Self> {
        // 16 random-ish bytes -> base64, per RFC 6455 §4.1. The mask seed is
        // derived from the same source; it need not be cryptographic for a
        // trusted localhost CRI streaming socket (no caching proxies in path).
        let mut seed = 0x9E37_79B9_u32;
        for byte in host.bytes().chain(path.bytes()) {
            seed = seed.wrapping_mul(31).wrapping_add(u32::from(byte));
        }
        let mut nonce = [0u8; 16];
        let mut x = seed | 1;
        for b in &mut nonce {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = (x & 0xFF) as u8;
        }
        let key = base64_encode(&nonce);

        let proto_hdr = if subprotocols.is_empty() {
            String::new()
        } else {
            format!("Sec-WebSocket-Protocol: {}\r\n", subprotocols.join(", "))
        };
        let req = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             {proto_hdr}\r\n"
        );
        io.write_all(req.as_bytes()).await?;
        io.flush().await?;

        let mut rbuf = Vec::new();
        let headers = read_headers(&mut io, &mut rbuf).await?;
        let status = headers.lines().next().unwrap_or_default();
        if !status.contains("101") {
            return Err(Error::new(
                ErrorKind::ConnectionRefused,
                format!("ws upgrade rejected: {status}"),
            ));
        }
        match header_value(&headers, "sec-websocket-accept") {
            Some(got) if got == accept_key(&key) => {}
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("bad Sec-WebSocket-Accept: {other:?}"),
                ));
            }
        }
        let subprotocol = header_value(&headers, "sec-websocket-protocol").map(str::to_owned);

        Ok(Self {
            io,
            role: Role::Client,
            rbuf,
            mask_seed: seed | 1,
            subprotocol,
        })
    }

    /// Perform the server side of the opening handshake over `io`.
    ///
    /// # Errors
    /// Fails if the request is not a valid WebSocket upgrade.
    pub async fn accept(mut io: S, subprotocol: &str) -> Result<Self> {
        let mut rbuf = Vec::new();
        let headers = read_headers(&mut io, &mut rbuf).await?;
        let key = header_value(&headers, "sec-websocket-key")
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing Sec-WebSocket-Key"))?;
        let accept = accept_key(key);

        // Echo the sub-protocol only if the client offered it.
        let offered = header_value(&headers, "sec-websocket-protocol")
            .is_some_and(|line| line.split(',').any(|p| p.trim() == subprotocol));
        let proto_hdr = if offered {
            format!("Sec-WebSocket-Protocol: {subprotocol}\r\n")
        } else {
            String::new()
        };
        let resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept}\r\n\
             {proto_hdr}\r\n"
        );
        io.write_all(resp.as_bytes()).await?;
        io.flush().await?;

        let subprotocol = offered.then(|| subprotocol.to_owned());
        Ok(Self {
            io,
            role: Role::Server,
            rbuf,
            mask_seed: 0x1234_5678,
            subprotocol,
        })
    }

    /// Next non-cryptographic masking key (xorshift32).
    const fn next_mask(&mut self) -> [u8; 4] {
        let mut x = self.mask_seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.mask_seed = x;
        x.to_be_bytes()
    }

    /// Send one message frame (masked iff this side is a client).
    ///
    /// # Errors
    /// Propagates the underlying write error.
    pub async fn send(&mut self, frame: &Frame) -> Result<()> {
        let mask = match self.role {
            Role::Client => Some(self.next_mask()),
            Role::Server => None,
        };
        let bytes = frame::encode(frame, mask);
        self.io.write_all(&bytes).await?;
        self.io.flush().await
    }

    /// Receive the next frame of any kind (including control frames).
    /// `Ok(None)` on clean EOF.
    ///
    /// # Errors
    /// Propagates read errors and frame-decode protocol errors.
    pub async fn recv_raw(&mut self) -> Result<Option<Frame>> {
        let mut chunk = [0u8; 8192];
        loop {
            match frame::decode(&self.rbuf) {
                Ok(Some((f, consumed))) => {
                    self.rbuf.drain(..consumed);
                    return Ok(Some(f));
                }
                Ok(None) => {
                    let n = self.io.read(&mut chunk).await?;
                    if n == 0 {
                        return Ok(None);
                    }
                    self.rbuf.extend_from_slice(&chunk[..n]);
                }
                Err(e) => return Err(Error::new(ErrorKind::InvalidData, e)),
            }
        }
    }

    /// Receive the next application message. `Ok(None)` on clean close/EOF.
    /// Ping frames are answered with Pong transparently.
    ///
    /// # Errors
    /// Propagates read errors and frame-decode protocol errors.
    pub async fn recv(&mut self) -> Result<Option<Frame>> {
        loop {
            match self.recv_raw().await? {
                None => return Ok(None),
                Some(f) => match f.opcode {
                    OpCode::Close => return Ok(None),
                    OpCode::Ping => {
                        self.send(&Frame {
                            fin: true,
                            opcode: OpCode::Pong,
                            payload: f.payload,
                        })
                        .await?;
                    }
                    OpCode::Pong => {} // ignore unsolicited pongs
                    _ => return Ok(Some(f)),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    async fn server(io: DuplexStream) -> WsConnection<DuplexStream> {
        WsConnection::accept(io, V5_CHANNEL_PROTOCOL)
            .await
            .expect("accept")
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

        client
            .send(&Frame::binary(b"\x00hello".to_vec()))
            .await
            .unwrap();
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
        let mut client = WsConnection::connect(c, "h", "/p", &[V5_CHANNEL_PROTOCOL])
            .await
            .unwrap();
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
            conn.send(&Frame {
                fin: true,
                opcode: OpCode::Ping,
                payload: b"hb".to_vec(),
            })
            .await
            .unwrap();
            let pong = conn.recv_raw().await.unwrap().expect("pong");
            assert_eq!(pong.opcode, OpCode::Pong);
            assert_eq!(pong.payload, b"hb");
            conn.send(&Frame::binary(b"after".to_vec())).await.unwrap();
        });
        let mut client = WsConnection::connect(c, "h", "/p", &[V5_CHANNEL_PROTOCOL])
            .await
            .unwrap();
        let data = client.recv().await.unwrap().expect("data");
        assert_eq!(data.payload, b"after");
        srv.await.unwrap();
    }
}
