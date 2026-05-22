// SPDX-License-Identifier: Apache-2.0
//! Registry abstraction.
//!
//! Upstream parity: `pkg/subnet/etcdv2/registry.go` (interface) — we keep the
//! same surface so a single `SubnetManager` implementation rides over either
//! `MemRegistry` or `EtcdRegistry`.

use crate::config::NetworkConfig;
use crate::subnet::errors::Result;
use crate::subnet::lease::{Lease, LeaseAttrs, LeaseEvent};
use async_trait::async_trait;
use ipnet::Ipv4Net;
use tokio::sync::mpsc::Receiver;

#[async_trait]
pub trait Registry: Send + Sync + 'static {
    /// Read the cluster-wide network config (the JSON blob at
    /// `/coreos.com/network/config`).
    async fn get_network_config(&self) -> Result<NetworkConfig>;

    /// Persist the cluster-wide network config (idempotent set).
    async fn put_network_config(&self, cfg: &NetworkConfig) -> Result<()>;

    /// List all currently-active leases.
    async fn get_subnets(&self) -> Result<Vec<Lease>>;

    /// Create or update a lease for `subnet`. `ttl_secs` is the desired
    /// expiry-from-now window.
    async fn create_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease>;

    /// Refresh an existing lease's TTL.
    async fn update_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease>;

    /// Delete a lease (e.g. on graceful shutdown).
    async fn delete_subnet(&self, subnet: Ipv4Net) -> Result<()>;

    /// Subscribe to lease lifecycle events. Returns a receiver-side channel.
    async fn watch_subnets(&self) -> Result<Receiver<LeaseEvent>>;
}
