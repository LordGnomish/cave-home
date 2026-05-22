// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Network (NWK) layer — Zigbee 3.0 §3.2 / §3.4.
//!
//! Phase 1 provides the NWK information base (NIB) attributes the
//! coordinator actually needs to expose to the application layer and a
//! handle to the routing table. The on-air NWK frame encode/decode is
//! handled by the NCP / coordinator firmware below us — cave-home talks
//! to that NCP through EZSP or deCONZ, so the NWK frame layer itself is
//! firmware-internal.

use std::sync::Arc;

use parking_lot::RwLock;

use super::routing::RoutingTable;

/// Network Information Base — §3.5.2.
#[derive(Clone, Copy, Debug)]
pub struct NetworkInformationBase {
    /// PAN ID (network-wide identifier for the local network).
    pub pan_id: u16,
    /// Extended PAN ID (64-bit, immutable for the lifetime of the network).
    pub extended_pan_id: u64,
    /// Current 802.15.4 channel (11..=26).
    pub channel: u8,
    /// nwkUpdateID — bumped by the coordinator after a channel switch.
    pub update_id: u8,
    /// Coordinator's short address (always 0x0000 in centralised mode).
    pub coordinator_short_address: u16,
}

impl Default for NetworkInformationBase {
    fn default() -> Self {
        Self {
            pan_id: 0,
            extended_pan_id: 0,
            channel: 0,
            update_id: 0,
            coordinator_short_address: 0x0000,
        }
    }
}

/// Network layer — exposes the NIB + routing table.
///
/// Held by [`crate::coordinator::Coordinator`]. Cheap to clone because
/// the inner state is `Arc<RwLock<…>>`.
#[derive(Clone)]
pub struct NetworkLayer {
    nib: Arc<RwLock<NetworkInformationBase>>,
    routing: Arc<RwLock<RoutingTable>>,
}

impl Default for NetworkLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkLayer {
    /// Construct with default NIB + empty routing table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nib: Arc::new(RwLock::new(NetworkInformationBase::default())),
            routing: Arc::new(RwLock::new(RoutingTable::new())),
        }
    }

    /// Read-only snapshot of the NIB.
    #[must_use]
    pub fn nib(&self) -> NetworkInformationBase {
        *self.nib.read()
    }

    /// Mutator — overwrite the NIB.
    pub fn set_nib(&self, nib: NetworkInformationBase) {
        *self.nib.write() = nib;
    }

    /// Borrow the routing table mutably (locked guard).
    #[must_use]
    pub fn routing(&self) -> Arc<RwLock<RoutingTable>> {
        Arc::clone(&self.routing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::routing::{RoutingStatus, RoutingTableEntry};

    #[test]
    fn default_nib_has_coordinator_short_address_zero() {
        let n = NetworkLayer::new();
        assert_eq!(n.nib().coordinator_short_address, 0x0000);
    }

    #[test]
    fn set_nib_round_trips() {
        let n = NetworkLayer::new();
        let new_nib = NetworkInformationBase {
            pan_id: 0xface,
            extended_pan_id: 0xdead_beef_cafe_babe,
            channel: 15,
            update_id: 1,
            coordinator_short_address: 0x0000,
        };
        n.set_nib(new_nib);
        let got = n.nib();
        assert_eq!(got.pan_id, 0xface);
        assert_eq!(got.channel, 15);
    }

    #[test]
    fn routing_can_be_mutated_through_handle() {
        let n = NetworkLayer::new();
        n.routing().write().upsert(RoutingTableEntry {
            destination: 0x1000,
            next_hop: 0x0001,
            status: RoutingStatus::Active,
            no_route_cache: false,
            many_to_one: false,
        });
        assert!(n.routing().read().lookup(0x1000).is_some());
    }
}
