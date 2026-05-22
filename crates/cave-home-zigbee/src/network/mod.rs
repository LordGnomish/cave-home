// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Network & APS layers.
//!
//! Phase 1 implements the slice cave-home actually exercises during
//! coordinator operation: the routing table representation (Zigbee
//! §3.6.1.4), an APS data-request primitive, and the APSME getters /
//! setters required for binding management.

pub mod aps;
pub mod nwk;
pub mod routing;

pub use aps::{ApsDataRequest, ApsmePrimitive};
pub use nwk::NetworkLayer;
pub use routing::{RoutingStatus, RoutingTable, RoutingTableEntry};
