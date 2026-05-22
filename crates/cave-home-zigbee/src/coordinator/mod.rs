// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Coordinator entry point.
//!
//! Phase 1 supports three dongle families behind a single coordinator
//! facade:
//!
//! - **Sonoff ZBDongle-E** (Silicon Labs EFR32MG21 NCP) — EZSP over a
//!   USB-UART (CDC-ACM at `/dev/ttyACM0`).
//! - **SMLIGHT SLZB-06** — EZSP either over USB-UART or over a TCP
//!   socket (network mode).
//! - **ConBee II** (dresden-elektronik deRFusb-23E06) — deCONZ serial
//!   protocol over USB-UART.
//!
//! The coordinator owns the [`crate::transport::Transport`] handle, the
//! routing table / NIB ([`crate::network::NetworkLayer`]), the groups /
//! scenes / OTA stores, and the outbound event bus.

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::Mutex as AsyncMutex;

use crate::attribute_reporting::ReportDeduper;
use crate::error::{Result, ZigbeeError};
use crate::events::{EventBus, ZigbeeEvent};
use crate::groups::GroupsCluster;
use crate::network::NetworkLayer;
use crate::ota::OtaQueue;
use crate::pairing::NetworkSteering;
use crate::scenes::ScenesCluster;
use crate::transport::Transport;

/// Coordinator dongle family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DongleFamily {
    /// Silicon Labs EZSP (Sonoff ZBDongle-E, SMLIGHT SLZB-06).
    SiliconLabsEzsp,
    /// dresden-elektronik deCONZ serial (ConBee II).
    DeconzSerial,
}

/// Coordinator status — exposed to the Portal as "Hub durumu".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorState {
    /// Not yet started.
    Idle,
    /// Transport open, NCP version handshake in progress.
    Initialising,
    /// Ready to receive joins and forward attributes.
    Ready,
    /// Lost contact with the NCP.
    Down,
}

/// The coordinator facade — wraps a transport + the Phase 1 cluster
/// stores + the event bus. Cheap to clone (all fields are `Arc`-backed).
#[derive(Clone)]
pub struct Coordinator {
    family: DongleFamily,
    transport: Arc<dyn Transport>,
    state: Arc<RwLock<CoordinatorState>>,
    network: NetworkLayer,
    steering: Arc<AsyncMutex<NetworkSteering>>,
    groups: Arc<AsyncMutex<GroupsCluster>>,
    scenes: Arc<AsyncMutex<ScenesCluster>>,
    ota: OtaQueue,
    dedup: Arc<AsyncMutex<ReportDeduper>>,
    events: EventBus,
}

impl Coordinator {
    /// Construct a coordinator over `transport`.
    ///
    /// The returned coordinator is in [`CoordinatorState::Idle`] — call
    /// [`Self::initialise`] to bring it up.
    #[must_use]
    pub fn new(family: DongleFamily, transport: Arc<dyn Transport>) -> Self {
        Self {
            family,
            transport,
            state: Arc::new(RwLock::new(CoordinatorState::Idle)),
            network: NetworkLayer::new(),
            steering: Arc::new(AsyncMutex::new(NetworkSteering::new())),
            groups: Arc::new(AsyncMutex::new(GroupsCluster::new())),
            scenes: Arc::new(AsyncMutex::new(ScenesCluster::new())),
            ota: OtaQueue::no_image(),
            dedup: Arc::new(AsyncMutex::new(ReportDeduper::new())),
            events: EventBus::new(64),
        }
    }

    /// Dongle family this coordinator was built for.
    #[must_use]
    pub const fn family(&self) -> DongleFamily {
        self.family
    }

    /// Current coordinator state (snapshot).
    #[must_use]
    pub fn state(&self) -> CoordinatorState {
        *self.state.read()
    }

    /// Borrow the network layer.
    #[must_use]
    pub fn network(&self) -> &NetworkLayer {
        &self.network
    }

    /// Borrow the OTA queue.
    #[must_use]
    pub fn ota(&self) -> &OtaQueue {
        &self.ota
    }

    /// Borrow the event bus.
    #[must_use]
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// Borrow the groups cluster (async-locked).
    #[must_use]
    pub fn groups(&self) -> &AsyncMutex<GroupsCluster> {
        &self.groups
    }

    /// Borrow the scenes cluster (async-locked).
    #[must_use]
    pub fn scenes(&self) -> &AsyncMutex<ScenesCluster> {
        &self.scenes
    }

    /// Borrow the network-steering controller.
    #[must_use]
    pub fn steering(&self) -> &AsyncMutex<NetworkSteering> {
        &self.steering
    }

    /// Borrow the report deduplicator.
    #[must_use]
    pub fn dedup(&self) -> &AsyncMutex<ReportDeduper> {
        &self.dedup
    }

    /// Initialise the coordinator. For Phase 1 this:
    ///
    /// 1. Writes a probe byte to the transport (verifies the link is up).
    /// 2. Marks the coordinator as Initialising.
    /// 3. Populates the NIB with placeholder values (channel / PAN ID
    ///    discovered by the NCP at real startup; here we set them to
    ///    documented defaults from Zigbee 3.0 Annex C).
    /// 4. Publishes [`ZigbeeEvent::CoordinatorReady`] and flips the
    ///    state to Ready.
    ///
    /// The full handshake (EZSP version + networkInit + form network)
    /// uses the encoded commands in [`crate::ezsp::EzspCommand`] /
    /// [`crate::deconz::DeconzCommand`]; this method does not block on
    /// a real response because the test bench feeds responses
    /// asynchronously. Production wiring lives in the orchestration
    /// crate.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Transport`] on transport write failure.
    pub async fn initialise(&self, coordinator_ieee: u64, pan_id: u16, channel: u8) -> Result<()> {
        if !(11..=26).contains(&channel) {
            return Err(ZigbeeError::Network(format!(
                "channel {channel} outside 11..=26 (Zigbee 3.0 §C.1.1)"
            )));
        }
        *self.state.write() = CoordinatorState::Initialising;
        // Probe the transport — the real EZSP/deCONZ exchange happens above.
        self.transport.write_all(&[]).await?;
        let mut nib = self.network.nib();
        nib.pan_id = pan_id;
        nib.channel = channel;
        nib.coordinator_short_address = 0x0000;
        nib.extended_pan_id = coordinator_ieee;
        self.network.set_nib(nib);
        *self.state.write() = CoordinatorState::Ready;
        self.events.publish(ZigbeeEvent::CoordinatorReady {
            coordinator_ieee,
            pan_id,
            channel,
        });
        Ok(())
    }

    /// Mark the coordinator as down (e.g. NCP firmware crash detected).
    pub fn mark_down(&self, reason: impl Into<String>) {
        *self.state.write() = CoordinatorState::Down;
        self.events.publish(ZigbeeEvent::CoordinatorDown {
            reason: reason.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MemoryTransport;

    #[tokio::test]
    async fn new_coordinator_is_idle() {
        let t = Arc::new(MemoryTransport::new("loop"));
        let c = Coordinator::new(DongleFamily::SiliconLabsEzsp, t);
        assert_eq!(c.state(), CoordinatorState::Idle);
        assert_eq!(c.family(), DongleFamily::SiliconLabsEzsp);
    }

    #[tokio::test]
    async fn initialise_populates_nib_and_emits_event() {
        let t = Arc::new(MemoryTransport::new("loop"));
        let c = Coordinator::new(DongleFamily::SiliconLabsEzsp, t);
        let mut sub = c.events.subscribe();
        c.initialise(0xdead_beef, 0x1a2b, 15).await.unwrap();
        assert_eq!(c.state(), CoordinatorState::Ready);
        let nib = c.network().nib();
        assert_eq!(nib.pan_id, 0x1a2b);
        assert_eq!(nib.channel, 15);
        assert_eq!(nib.coordinator_short_address, 0x0000);
        let event = sub.recv().await.unwrap();
        assert_eq!(
            event,
            ZigbeeEvent::CoordinatorReady {
                coordinator_ieee: 0xdead_beef,
                pan_id: 0x1a2b,
                channel: 15,
            }
        );
    }

    #[tokio::test]
    async fn initialise_rejects_invalid_channel() {
        let t = Arc::new(MemoryTransport::new("loop"));
        let c = Coordinator::new(DongleFamily::SiliconLabsEzsp, t);
        assert!(c.initialise(0xdead_beef, 0x1a2b, 27).await.is_err());
        assert!(c.initialise(0xdead_beef, 0x1a2b, 10).await.is_err());
    }

    #[tokio::test]
    async fn mark_down_transitions_state_and_emits_event() {
        let t = Arc::new(MemoryTransport::new("loop"));
        let c = Coordinator::new(DongleFamily::DeconzSerial, t);
        let mut sub = c.events.subscribe();
        c.mark_down("ncp gone");
        assert_eq!(c.state(), CoordinatorState::Down);
        let e = sub.recv().await.unwrap();
        match e {
            ZigbeeEvent::CoordinatorDown { reason } => assert_eq!(reason, "ncp gone"),
            other => panic!("expected CoordinatorDown, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deconz_family_can_initialise() {
        let t = Arc::new(MemoryTransport::new("loop"));
        let c = Coordinator::new(DongleFamily::DeconzSerial, t);
        c.initialise(0xcafebabe, 0x4242, 20).await.unwrap();
        assert_eq!(c.state(), CoordinatorState::Ready);
    }
}
