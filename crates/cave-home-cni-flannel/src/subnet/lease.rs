// SPDX-License-Identifier: Apache-2.0
//! Lease + lease event types.
//!
//! Upstream parity: `pkg/subnet/subnet.go` (the `Lease`, `LeaseAttrs`,
//! `EventType`, `Event` types) and `pkg/subnet/types.go` (the `Reservation`
//! struct used to pin a subnet to a specific public IP).

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Per-node attributes attached to a lease.
///
/// Upstream parity: `pkg/subnet/subnet.go` `LeaseAttrs` struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseAttrs {
    /// The node's externally-routable IPv4 address (used as the VXLAN tunnel
    /// endpoint).
    #[serde(rename = "PublicIP")]
    pub public_ip: Ipv4Addr,

    /// Backend kind ("vxlan", "host-gw", ...). Phase 1 always emits "vxlan".
    #[serde(rename = "BackendType")]
    pub backend_type: String,

    /// Backend-specific opaque blob — for VXLAN this is `{"VtepMAC":"..."}`.
    #[serde(rename = "BackendData", default, skip_serializing_if = "Option::is_none")]
    pub backend_data: Option<serde_json::Value>,
}

/// One per-node lease (subnet + attrs + expiry).
///
/// Upstream parity: `pkg/subnet/subnet.go` `Lease` struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lease {
    #[serde(rename = "Subnet")]
    pub subnet: Ipv4Net,
    #[serde(rename = "Attrs")]
    pub attrs: LeaseAttrs,
    /// UNIX seconds at which the lease will expire if not renewed.
    #[serde(rename = "Expiration")]
    pub expiration: u64,
}

/// Static reservation pinning a subnet to a public IP.
///
/// Upstream parity: `pkg/subnet/types.go` `Reservation`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reservation {
    pub subnet: Ipv4Net,
    pub public_ip: Ipv4Addr,
}

/// Lease lifecycle events emitted by `SubnetManager::watch_leases`.
///
/// Upstream parity: `pkg/subnet/subnet.go` `EventType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseEvent {
    pub event_type: EventType,
    pub lease: Lease,
}
