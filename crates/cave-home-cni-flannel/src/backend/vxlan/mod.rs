// SPDX-License-Identifier: Apache-2.0
//! VXLAN backend.
//!
//! Upstream parity: `pkg/backend/vxlan/`. The Linux datapath module
//! (`device.rs`) creates `flannel.<vni>`, sets MAC/VNI/port/UP, and installs
//! FDB+ARP entries on lease events. The `network.rs` module wires it to the
//! lease-event stream.
//!
//! On non-Linux, [`VxlanBackend::register_network`] returns
//! [`BackendError::UnsupportedPlatform`] so the trait surface still compiles.

pub mod config;
pub mod network;

#[cfg(target_os = "linux")]
pub mod device;

use crate::backend::trait_def::{Backend, BackendError, BackendNetwork, Result};
use crate::config::{BackendConfig, NetworkConfig};
use crate::subnet::{Lease, LeaseAttrs};
use async_trait::async_trait;

pub use network::VxlanNetwork;

#[derive(Debug, Default, Clone)]
pub struct VxlanBackend;

impl VxlanBackend {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Backend for VxlanBackend {
    async fn register_network(
        &self,
        cfg: &NetworkConfig,
        local_attrs: &LeaseAttrs,
        local_lease: &Lease,
    ) -> Result<Box<dyn BackendNetwork>> {
        let BackendConfig::Vxlan(vxlan_cfg) = &cfg.backend;
        VxlanNetwork::register(cfg, vxlan_cfg, local_attrs, local_lease)
            .await
            .map(|n| Box::new(n) as Box<dyn BackendNetwork>)
    }
}

/// Helper: construct an `UnsupportedPlatform` error — used by callers that
/// want to short-circuit on non-Linux without dragging in the netlink modules.
#[must_use]
pub const fn unsupported_platform_error() -> BackendError {
    BackendError::UnsupportedPlatform
}
