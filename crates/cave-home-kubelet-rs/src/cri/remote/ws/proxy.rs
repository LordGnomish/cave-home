// SPDX-License-Identifier: Apache-2.0
//! `v5.channel.k8s.io` streaming proxy for CRI exec / attach / port-forward.
//!
//! Once [`super::conn::WsConnection`] has upgraded the connection to the
//! runtime's streaming URL, the bytes are multiplexed onto numbered *channels*
//! (the remotecommand protocol): every WebSocket binary message is a single
//! channel byte followed by that channel's data.
//!
//! | channel | exec / attach | port-forward            |
//! |---------|---------------|-------------------------|
//! | 0       | stdin         | data (port-prefixed)    |
//! | 1       | stdout        | error                   |
//! | 2       | stderr        |                         |
//! | 3       | error/status  |                         |
//! | 4       | resize (JSON) |                         |
//! | 255     | close-stream  |                         |
//!
//! Channel 255 is the v5 addition: its one-byte payload names a channel to
//! half-close, which is how the kubelet signals "stdin EOF" without tearing the
//! whole connection down.

use std::io::{Error, ErrorKind, Result};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::conn::WsConnection;
use super::frame::Frame;

/// Remotecommand channel numbers.
pub mod channel {
    /// Standard input (exec/attach) or data (port-forward).
    pub const STDIN: u8 = 0;
    /// Standard output (exec/attach) or error (port-forward).
    pub const STDOUT: u8 = 1;
    /// Standard error (exec/attach).
    pub const STDERR: u8 = 2;
    /// Error / status stream (exec/attach).
    pub const ERROR: u8 = 3;
    /// Terminal-resize control (exec/attach), JSON `{Width,Height}`.
    pub const RESIZE: u8 = 4;
    /// v5 half-close control; payload is the channel byte to close.
    pub const CLOSE: u8 = 255;
}

/// Outcome of an exec/attach session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecOutcome {
    /// Raw bytes received on the error/status channel (channel 3), if any.
    /// Empty for a clean (exit-0) command.
    pub error: Option<String>,
}

/// Wrap `data` as a `v5.channel.k8s.io` message frame on `ch`.
#[must_use]
pub fn channel_frame(ch: u8, data: &[u8]) -> Frame {
    let mut payload = Vec::with_capacity(data.len() + 1);
    payload.push(ch);
    payload.extend_from_slice(data);
    Frame::binary(payload)
}

/// Split a received channel message into `(channel, data)`.
#[must_use]
pub fn split_channel(payload: &[u8]) -> Option<(u8, &[u8])> {
    payload.split_first().map(|(ch, rest)| (*ch, rest))
}

// stub — replaced in the GREEN step
/// Dial the negotiated streaming `url` over TCP and run the WebSocket
/// handshake. Only `http://` / `ws://` URLs are supported; `https`/`wss` (TLS)
/// is deferred (see the parity manifest).
///
/// # Errors
/// Fails on a malformed URL, TCP dial failure, or a rejected upgrade.
pub async fn dial(_url: &str, _subprotocols: &[&str]) -> Result<WsConnection<TcpStream>> {
    Err(Error::new(ErrorKind::Other, "unimplemented"))
}

// stub — replaced in the GREEN step
/// Drive an exec/attach session: pump `stdin` onto channel 0 and demux the
/// runtime's channel 1/2/3 onto `stdout`/`stderr`/the returned outcome.
///
/// # Errors
/// Propagates transport errors.
pub async fn run_exec<S, I, O, E>(
    _conn: WsConnection<S>,
    _stdin: Option<I>,
    _stdout: O,
    _stderr: E,
    _term_size: Option<(u16, u16)>,
) -> Result<ExecOutcome>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    I: AsyncRead + Unpin + Send,
    O: AsyncWrite + Unpin + Send,
    E: AsyncWrite + Unpin + Send,
{
    Ok(ExecOutcome::default())
}

// stub — replaced in the GREEN step
/// Bridge a local stream `io` to a single forwarded `port` over channel 0
/// (data) / channel 1 (error). The first data frame carries the 2-byte
/// little-endian port number, per the websocket port-forward protocol.
///
/// # Errors
/// Propagates transport errors.
pub async fn run_port_forward<S, IO>(_conn: WsConnection<S>, _port: u16, _io: IO) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    IO: AsyncRead + AsyncWrite + Unpin + Send,
{
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_frame_prefixes_channel_byte() {
        let f = channel_frame(channel::STDIN, b"hello");
        assert_eq!(f.payload, b"\x00hello");
    }

    #[test]
    fn split_channel_round_trips() {
        let f = channel_frame(channel::STDERR, b"oops");
        let (ch, data) = split_channel(&f.payload).unwrap();
        assert_eq!(ch, channel::STDERR);
        assert_eq!(data, b"oops");
    }

    #[test]
    fn split_channel_rejects_empty() {
        assert_eq!(split_channel(&[]), None);
    }
}
