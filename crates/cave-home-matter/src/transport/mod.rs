// SPDX-License-Identifier: Apache-2.0
//! Matter transports — UDP (operational) + BLE (commissioning).
//!
//! # Upstream: project-chip/connectedhomeip@5af45c5c:src/transport/raw/

pub mod ble;
pub mod udp;

pub use ble::BleTransport;
pub use udp::UdpTransport;

use async_trait::async_trait;

/// Generic Matter transport — sends framed bytes to a peer.
///
/// # Upstream: src/transport/raw/Base.h::Base
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a frame to the named peer (BLE address or `ip:port`).
    async fn send(&self, peer: &str, frame: &[u8]) -> crate::error::Result<()>;

    /// Receive the next frame; blocks until one arrives.
    async fn recv(&self) -> crate::error::Result<(String, Vec<u8>)>;
}
