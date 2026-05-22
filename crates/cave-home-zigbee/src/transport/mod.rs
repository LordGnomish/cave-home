// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Transport abstraction.
//!
//! A Zigbee coordinator is reached either over a local USB-UART
//! (Sonoff ZBDongle-E, ConBee II) or over a TCP socket (SMLIGHT
//! SLZB-06 in network mode; also ESPHome `zigbee_*` gateway boards).
//! This module abstracts both behind a single async byte-stream trait
//! so the EZSP / deCONZ layers above don't care.
//!
//! Phase 1 ships:
//! - [`Transport`]      — async byte-stream trait (read / write / flush).
//! - [`MemoryTransport`] — in-memory loopback for tests + admin tooling.
//! - [`TcpTransport`]    — TCP/IP transport (works for SLZB-06 network mode).
//!
//! USB-UART is wired through `tokio::fs::File` (a CDC-ACM device file at
//! `/dev/ttyACM0` / `/dev/ttyUSB0`) — see [`UartTransport`]. We do not
//! pull in the `serialport` crate; raw line-discipline is enough at the
//! frame-byte level (EZSP / deCONZ both do their own framing on top).

use std::sync::Arc;

use async_trait::async_trait;
use bytes::BytesMut;
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{Result, ZigbeeError};

/// Asynchronous byte-stream transport to a Zigbee coordinator.
///
/// Implementations must be cheap to clone / share by `Arc` and must
/// serialise concurrent writers (since EZSP / deCONZ framing is
/// byte-stream oriented).
#[async_trait]
pub trait Transport: Send + Sync {
    /// Write all bytes — must be atomic with respect to other writers.
    async fn write_all(&self, bytes: &[u8]) -> Result<()>;

    /// Read up to `buf.len()` bytes. Returns the number actually read
    /// (which may be 0 only on EOF / closed transport).
    async fn read(&self, buf: &mut [u8]) -> Result<usize>;

    /// Flush any pending write buffers.
    async fn flush(&self) -> Result<()>;

    /// Best-effort identifier (e.g. `/dev/ttyACM0`, `tcp://1.2.3.4:6638`).
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// MemoryTransport — in-memory loopback for tests
// ---------------------------------------------------------------------------

/// In-memory loopback transport.
///
/// `write_all` appends to an outbound buffer; the test harness consumes
/// it via [`MemoryTransport::take_written`]. `read` is fed by
/// [`MemoryTransport::push_to_read`].
#[derive(Clone)]
pub struct MemoryTransport {
    name: String,
    written: Arc<Mutex<BytesMut>>,
    to_read: Arc<Mutex<BytesMut>>,
}

impl MemoryTransport {
    /// Create a new loopback transport tagged with `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            written: Arc::new(Mutex::new(BytesMut::new())),
            to_read: Arc::new(Mutex::new(BytesMut::new())),
        }
    }

    /// Drain every byte written so far.
    #[must_use]
    pub fn take_written(&self) -> Vec<u8> {
        let mut guard = self.written.lock();
        let bytes = guard.split().to_vec();
        bytes
    }

    /// Push bytes that will be returned by subsequent `read` calls.
    pub fn push_to_read(&self, bytes: &[u8]) {
        self.to_read.lock().extend_from_slice(bytes);
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn write_all(&self, bytes: &[u8]) -> Result<()> {
        self.written.lock().extend_from_slice(bytes);
        Ok(())
    }

    async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut guard = self.to_read.lock();
        let n = guard.len().min(buf.len());
        if n == 0 {
            return Ok(0);
        }
        let chunk = guard.split_to(n);
        buf[..n].copy_from_slice(&chunk);
        Ok(n)
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// TcpTransport — IP socket transport (SLZB-06 network mode)
// ---------------------------------------------------------------------------

/// TCP transport for network-mode coordinators (e.g. SLZB-06).
pub struct TcpTransport {
    name: String,
    stream: AsyncMutex<TcpStream>,
}

impl TcpTransport {
    /// Connect to `host:port` and return a ready transport.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Transport`] if the connect fails.
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("tcp connect {addr}: {e}")))?;
        stream
            .set_nodelay(true)
            .map_err(|e| ZigbeeError::Transport(format!("nodelay: {e}")))?;
        Ok(Self {
            name: format!("tcp://{addr}"),
            stream: AsyncMutex::new(stream),
        })
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn write_all(&self, bytes: &[u8]) -> Result<()> {
        let mut guard = self.stream.lock().await;
        guard
            .write_all(bytes)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("tcp write: {e}")))
    }

    async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut guard = self.stream.lock().await;
        guard
            .read(buf)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("tcp read: {e}")))
    }

    async fn flush(&self) -> Result<()> {
        let mut guard = self.stream.lock().await;
        guard
            .flush()
            .await
            .map_err(|e| ZigbeeError::Transport(format!("tcp flush: {e}")))
    }

    fn name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// UartTransport — CDC-ACM serial via tokio::fs::File
// ---------------------------------------------------------------------------

/// UART transport over a CDC-ACM device file (e.g. `/dev/ttyACM0`).
///
/// On modern Linux 7.1+ a USB CDC-ACM dongle appears as a character
/// device that supports byte-level I/O. We open it in read+write mode
/// and rely on the EZSP / deCONZ framers above to recover packet
/// boundaries.
pub struct UartTransport {
    name: String,
    file: AsyncMutex<tokio::fs::File>,
}

impl UartTransport {
    /// Open `path` (e.g. `/dev/ttyACM0`) as a UART transport.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Transport`] if open fails.
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path_ref)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("uart open {}: {e}", path_ref.display())))?;
        Ok(Self {
            name: path_ref.display().to_string(),
            file: AsyncMutex::new(file),
        })
    }
}

#[async_trait]
impl Transport for UartTransport {
    async fn write_all(&self, bytes: &[u8]) -> Result<()> {
        let mut guard = self.file.lock().await;
        guard
            .write_all(bytes)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("uart write: {e}")))
    }

    async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut guard = self.file.lock().await;
        guard
            .read(buf)
            .await
            .map_err(|e| ZigbeeError::Transport(format!("uart read: {e}")))
    }

    async fn flush(&self) -> Result<()> {
        let mut guard = self.file.lock().await;
        guard
            .flush()
            .await
            .map_err(|e| ZigbeeError::Transport(format!("uart flush: {e}")))
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_loopback_round_trips() {
        let t = MemoryTransport::new("loop");
        t.write_all(b"hello").await.expect("write");
        assert_eq!(t.take_written(), b"hello");

        t.push_to_read(b"world");
        let mut buf = [0u8; 16];
        let n = t.read(&mut buf).await.expect("read");
        assert_eq!(&buf[..n], b"world");
    }

    #[tokio::test]
    async fn memory_read_on_empty_returns_zero() {
        let t = MemoryTransport::new("loop");
        let mut buf = [0u8; 4];
        let n = t.read(&mut buf).await.expect("read");
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn memory_write_take_is_drained() {
        let t = MemoryTransport::new("loop");
        t.write_all(b"abc").await.unwrap();
        assert_eq!(t.take_written(), b"abc");
        // Second take returns empty.
        assert!(t.take_written().is_empty());
    }

    #[tokio::test]
    async fn memory_name_is_preserved() {
        let t = MemoryTransport::new("the-name");
        assert_eq!(t.name(), "the-name");
    }
}
