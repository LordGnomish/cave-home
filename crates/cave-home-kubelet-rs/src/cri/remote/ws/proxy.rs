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

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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

/// Dial the negotiated streaming `url` over TCP and run the WebSocket
/// handshake. Only `http://` / `ws://` URLs are supported; `https`/`wss` (TLS)
/// is deferred (see the parity manifest).
///
/// # Errors
/// Fails on a malformed URL, TCP dial failure, or a rejected upgrade.
pub async fn dial(url: &str, subprotocols: &[&str]) -> Result<WsConnection<TcpStream>> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("ws://"))
        .ok_or_else(|| {
            Error::new(ErrorKind::InvalidInput, format!("unsupported streaming URL: {url}"))
        })?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_owned()),
    };
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    let addr = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };

    let stream = TcpStream::connect(&addr).await?;
    WsConnection::connect(stream, host, &path, subprotocols).await
}

/// Demux a received channel message into the output sinks / outcome.
/// Returns `true` when the session should end.
async fn dispatch_output<O, E>(
    payload: &[u8],
    stdout: &mut O,
    stderr: &mut E,
    error: &mut Option<String>,
) -> Result<()>
where
    O: AsyncWrite + Unpin,
    E: AsyncWrite + Unpin,
{
    if let Some((ch, data)) = split_channel(payload) {
        match ch {
            channel::STDOUT => stdout.write_all(data).await?,
            channel::STDERR => stderr.write_all(data).await?,
            channel::ERROR => {
                *error = Some(String::from_utf8_lossy(data).into_owned());
            }
            _ => {} // resize/close/unknown from the runtime: ignored
        }
    }
    Ok(())
}

/// Drive an exec/attach session.
///
/// Pumps `stdin` onto channel 0 and demultiplexes the runtime's channel 1/2/3
/// onto `stdout` / `stderr` / the returned [`ExecOutcome`]. `term_size` sends an
/// initial resize on channel 4.
///
/// # Errors
/// Propagates transport errors.
pub async fn run_exec<S, I, O, E>(
    mut conn: WsConnection<S>,
    mut stdin: Option<I>,
    mut stdout: O,
    mut stderr: E,
    term_size: Option<(u16, u16)>,
) -> Result<ExecOutcome>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    I: AsyncRead + Unpin + Send,
    O: AsyncWrite + Unpin + Send,
    E: AsyncWrite + Unpin + Send,
{
    if let Some((w, h)) = term_size {
        let json = format!("{{\"Width\":{w},\"Height\":{h}}}");
        conn.send(&channel_frame(channel::RESIZE, json.as_bytes())).await?;
    }

    let mut error = None;
    let mut buf = vec![0u8; 8192];

    loop {
        // `stdin` is taken to `None` on EOF, which also flips off the input arm.
        if let Some(src) = stdin.as_mut() {
            tokio::select! {
                read = src.read(&mut buf) => {
                    let n = read?;
                    if n == 0 {
                        // v5 half-close: tell the runtime stdin is done.
                        conn.send(&channel_frame(channel::CLOSE, &[channel::STDIN])).await?;
                        stdin = None;
                    } else {
                        conn.send(&channel_frame(channel::STDIN, &buf[..n])).await?;
                    }
                }
                msg = conn.recv() => {
                    match msg? {
                        None => break,
                        Some(f) => dispatch_output(&f.payload, &mut stdout, &mut stderr, &mut error).await?,
                    }
                }
            }
        } else {
            match conn.recv().await? {
                None => break,
                Some(f) => {
                    dispatch_output(&f.payload, &mut stdout, &mut stderr, &mut error).await?;
                }
            }
        }
    }

    stdout.flush().await?;
    stderr.flush().await?;
    Ok(ExecOutcome { error })
}

/// Bridge a local stream `io` to a single forwarded `port`.
///
/// Data rides channel 0 and errors channel 1; the first data frame carries the
/// 2-byte little-endian port number, per the websocket port-forward protocol.
/// Multi-port forwarding (channels `2*i` / `2*i+1`) is out of scope here; this
/// drives one local connection to one container port.
///
/// # Errors
/// Propagates transport errors.
pub async fn run_port_forward<S, IO>(mut conn: WsConnection<S>, port: u16, io: IO) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
    IO: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (mut rd, mut wr) = tokio::io::split(io);
    let mut buf = vec![0u8; 8192];
    let mut first = true;
    let mut rd_open = true;

    loop {
        if rd_open {
            tokio::select! {
                read = rd.read(&mut buf) => {
                    let n = read?;
                    if n == 0 {
                        conn.send(&channel_frame(channel::CLOSE, &[channel::STDIN])).await?;
                        rd_open = false;
                    } else {
                        let data = if first {
                            first = false;
                            let mut v = port.to_le_bytes().to_vec();
                            v.extend_from_slice(&buf[..n]);
                            v
                        } else {
                            buf[..n].to_vec()
                        };
                        conn.send(&channel_frame(channel::STDIN, &data)).await?;
                    }
                }
                msg = conn.recv() => {
                    match msg? {
                        None => break,
                        Some(f) => {
                            if let Some((channel::STDIN, data)) = split_channel(&f.payload) {
                                wr.write_all(data).await?;
                            }
                        }
                    }
                }
            }
        } else {
            match conn.recv().await? {
                None => break,
                Some(f) => {
                    if let Some((channel::STDIN, data)) = split_channel(&f.payload) {
                        wr.write_all(data).await?;
                    }
                }
            }
        }
    }
    wr.flush().await?;
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
