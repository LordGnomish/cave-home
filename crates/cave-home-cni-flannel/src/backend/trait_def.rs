// SPDX-License-Identifier: Apache-2.0
//! `Backend` trait — the surface every datapath (VXLAN, host-gw, ...) honours.
//!
//! Upstream parity: `pkg/backend/common.go` `Backend` and `Network`
//! interfaces. We split `BackendNetwork` out of `Backend` to keep the trait
//! object-safe (the upstream `Network` is returned by `RegisterNetwork`).

use crate::config::NetworkConfig;
use crate::subnet::{Lease, LeaseAttrs};
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend not supported on this platform (Phase 1 datapath is Linux-only)")]
    UnsupportedPlatform,

    #[error("netlink error: {0}")]
    Netlink(String),

    #[error("subnet error: {0}")]
    Subnet(#[from] crate::subnet::SubnetError),

    #[error("invalid backend config: {0}")]
    InvalidConfig(String),

    #[error("io error: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, BackendError>;

/// Backend factory.
///
/// Upstream parity: `pkg/backend/common.go::Backend.RegisterNetwork`.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Register a network — i.e. set up the datapath device for the local
    /// node, returning a `BackendNetwork` handle to drive lease-event
    /// reconciliation.
    async fn register_network(
        &self,
        cfg: &NetworkConfig,
        local_attrs: &LeaseAttrs,
        local_lease: &Lease,
    ) -> Result<Box<dyn BackendNetwork>>;
}

/// Per-network handle returned by `Backend::register_network`.
///
/// Upstream parity: `pkg/backend/common.go::Network`.
#[async_trait]
pub trait BackendNetwork: Send + Sync {
    /// Apply a remote-lease event (install/remove FDB+ARP+route entries).
    async fn handle_lease_event(&self, ev: &crate::subnet::LeaseEvent) -> Result<()>;

    /// MTU exposed to the CNI plugin via `subnet.env`.
    fn mtu(&self) -> u32;

    /// Tear down the datapath (delete the device).
    async fn shutdown(&self) -> Result<()>;
}
