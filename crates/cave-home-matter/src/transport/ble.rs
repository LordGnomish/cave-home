// SPDX-License-Identifier: Apache-2.0
//! BLE commissioning transport.
//!
//! # Upstream: project-chip/connectedhomeip@5bb5c9e2:src/transport/raw/BLE.cpp
//! + src/ble/BleLayer.cpp
//!
//! Phase 1 ships an **in-memory BLE channel** built on top of
//! `tokio::sync::mpsc`. The chip BLE GATT model — RX/TX/C3
//! characteristic UUIDs, MTU negotiation, btp framing — is preserved
//! in the public type surface; the actual bluez bridge via the
//! `btleplug` crate is `[[unmapped]] phase-1b` (it depends on a
//! Linux host with bluetoothd, which the workspace CI does not
//! provide).
//!
//! The in-memory transport is interoperable with itself, which is
//! sufficient for the integration tests in `commissioner.rs` that
//! drive a simulated device.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::error::{MatterError, Result};
use crate::transport::Transport;

/// chip's BLE Matter service UUID — informational here.
///
/// # Upstream: src/ble/BleUUID.cpp::CHIP_BLE_SVC_ID
pub const MATTER_SERVICE_UUID: &str = "0000FFF6-0000-1000-8000-00805F9B34FB";

/// chip's commissioning RX characteristic UUID.
///
/// # Upstream: src/ble/BleUUID.cpp::CHIP_BLE_CHAR_1_ID
pub const MATTER_RX_CHAR_UUID: &str = "18EE2EF5-263D-4559-959F-4F9C429F9D11";

/// chip's commissioning TX characteristic UUID.
///
/// # Upstream: src/ble/BleUUID.cpp::CHIP_BLE_CHAR_2_ID
pub const MATTER_TX_CHAR_UUID: &str = "18EE2EF5-263D-4559-959F-4F9C429F9D12";

/// Minimum supported MTU for BTP — chip's `CHIP_BLE_DEFAULT_MTU`.
pub const BTP_DEFAULT_MTU: u16 = 23;

/// Address of an in-memory BLE peer (analogue of a real BD_ADDR).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BleAddr(pub String);

/// In-memory BLE transport — bi-directional channel keyed by peer addr.
pub struct BleTransport {
    addr: BleAddr,
    inbox_tx: mpsc::Sender<(String, Vec<u8>)>,
    inbox_rx: tokio::sync::Mutex<mpsc::Receiver<(String, Vec<u8>)>>,
    peers: Arc<Mutex<HashMap<String, mpsc::Sender<(String, Vec<u8>)>>>>,
}

impl BleTransport {
    /// Create a freshly-advertised BLE peer.
    pub fn new(addr: impl Into<String>) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let addr = BleAddr(addr.into());
        Self {
            addr,
            inbox_tx: tx,
            inbox_rx: tokio::sync::Mutex::new(rx),
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Inspect our advertised address.
    pub fn local_addr(&self) -> &BleAddr {
        &self.addr
    }

    /// Pair two transports so they can exchange BTP frames. After
    /// `connect`, sending to the other peer's addr will arrive at its
    /// `recv`.
    pub fn connect(&self, other: &BleTransport) {
        self.peers
            .lock()
            .insert(other.addr.0.clone(), other.inbox_tx.clone());
        other
            .peers
            .lock()
            .insert(self.addr.0.clone(), self.inbox_tx.clone());
    }
}

#[async_trait]
impl Transport for BleTransport {
    async fn send(&self, peer: &str, frame: &[u8]) -> Result<()> {
        if frame.len() > usize::from(BTP_DEFAULT_MTU) * 16 {
            return Err(MatterError::Transport(format!(
                "BLE frame {} bytes > 16 * MTU ({BTP_DEFAULT_MTU})",
                frame.len()
            )));
        }
        let tx = {
            let peers = self.peers.lock();
            peers.get(peer).cloned()
        };
        let tx = tx.ok_or_else(|| {
            MatterError::Transport(format!("BLE send: not connected to {peer}"))
        })?;
        tx.send((self.addr.0.clone(), frame.to_vec()))
            .await
            .map_err(|e| MatterError::Transport(format!("BLE send: channel closed: {e}")))
    }

    async fn recv(&self) -> Result<(String, Vec<u8>)> {
        let mut rx = self.inbox_rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| MatterError::Transport("BLE recv: channel closed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ble_in_memory_round_trip() {
        let commissioner = BleTransport::new("commissioner-addr");
        let device = BleTransport::new("device-addr");
        commissioner.connect(&device);

        commissioner
            .send("device-addr", b"sigma1")
            .await
            .expect("send");
        let (peer, frame) = device.recv().await.expect("recv");
        assert_eq!(peer, "commissioner-addr");
        assert_eq!(frame, b"sigma1");
    }

    #[tokio::test]
    async fn ble_send_to_unknown_peer_errors() {
        let t = BleTransport::new("a");
        let err = t.send("b", b"x").await.expect_err("must fail");
        match err {
            MatterError::Transport(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[tokio::test]
    async fn ble_send_rejects_oversized_frame() {
        let a = BleTransport::new("a");
        let b = BleTransport::new("b");
        a.connect(&b);
        let big = vec![0u8; usize::from(BTP_DEFAULT_MTU) * 16 + 1];
        let err = a.send("b", &big).await.expect_err("oversized must fail");
        match err {
            MatterError::Transport(_) => {}
            other => panic!("unexpected error {other:?}"),
        }
    }
}
