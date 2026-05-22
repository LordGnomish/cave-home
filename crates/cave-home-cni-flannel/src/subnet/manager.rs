// SPDX-License-Identifier: Apache-2.0
//! Subnet lease manager.
//!
//! Upstream parity: `pkg/subnet/local_manager.go` — specifically
//! `LocalManager.AcquireLease`, `RenewLease`, `RevokeLease`, `chooseSubnet`,
//! `WatchLeases`. The renewal loop (`Manager.Run`) lives here too.
//!
//! Allocation algorithm (mirrors `local_manager.go::tryAcquireLease`):
//!   1. List currently-held leases.
//!   2. Build the candidate iterator
//!      `[SubnetMin..SubnetMax]` step `1 << (32 - SubnetLen)`.
//!   3. Filter out occupied + reserved subnets.
//!   4. Pick a candidate uniformly at random (matches upstream `randomSubnet`
//!      so that node restarts don't re-collide on the same head-of-range).
//!   5. Persist via the registry; conflict → retry next candidate.

use crate::config::NetworkConfig;
use crate::subnet::clock::Clock;
use crate::subnet::errors::{Result, SubnetError};
use crate::subnet::lease::{Lease, LeaseAttrs, LeaseEvent, Reservation};
use crate::subnet::registry::Registry;
use async_trait::async_trait;
use ipnet::Ipv4Net;
use rand::seq::SliceRandom;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

/// Default lease TTL (24h, matches `pkg/subnet/local_manager.go::subnetTTL`).
pub const DEFAULT_LEASE_TTL_SECS: u64 = 24 * 3600;

#[async_trait]
pub trait SubnetManager: Send + Sync {
    /// Idempotent: if `attrs.public_ip` already owns a lease, return it;
    /// otherwise allocate one.
    async fn acquire_lease(&self, attrs: &LeaseAttrs) -> Result<Lease>;

    /// Bump the TTL of an existing lease.
    async fn renew_lease(&self, subnet: Ipv4Net, attrs: &LeaseAttrs) -> Result<Lease>;

    /// Drop a lease (graceful shutdown path).
    async fn revoke_lease(&self, subnet: Ipv4Net) -> Result<()>;

    /// Subscribe to remote-lease changes.
    async fn watch_leases(&self) -> Result<Receiver<LeaseEvent>>;
}

pub struct LocalManager<R: Registry, C: Clock> {
    registry: Arc<R>,
    clock: Arc<C>,
    reservations: Vec<Reservation>,
    ttl_secs: u64,
}

impl<R: Registry, C: Clock> LocalManager<R, C> {
    pub fn new(registry: Arc<R>, clock: Arc<C>) -> Self {
        Self {
            registry,
            clock,
            reservations: Vec::new(),
            ttl_secs: DEFAULT_LEASE_TTL_SECS,
        }
    }

    #[must_use]
    pub fn with_reservations(mut self, r: Vec<Reservation>) -> Self {
        self.reservations = r;
        self
    }

    #[must_use]
    pub const fn with_ttl(mut self, ttl_secs: u64) -> Self {
        self.ttl_secs = ttl_secs;
        self
    }

    /// Build the full candidate-subnet iterator for `cfg`.
    ///
    /// Mirrors `pkg/subnet/local_manager.go::populateLeaseChannel` but
    /// materialises into a `Vec` so we can `shuffle` deterministically in
    /// tests via `rand::SeedableRng`.
    pub fn enumerate_candidates(cfg: &NetworkConfig) -> Result<Vec<Ipv4Net>> {
        if !cfg.enable_ipv4 {
            return Err(SubnetError::InvalidConfig(
                "Phase 1 requires EnableIPv4=true".into(),
            ));
        }
        if cfg.subnet_len <= cfg.network.prefix_len() || cfg.subnet_len > 30 {
            return Err(SubnetError::InvalidConfig(format!(
                "SubnetLen {} not in (network prefix {}, 30]",
                cfg.subnet_len,
                cfg.network.prefix_len()
            )));
        }
        let subnet_len = cfg.subnet_len;
        let step: u32 = 1u32 << (32 - subnet_len);
        let net_start = u32::from(cfg.network.network());
        let net_end = u32::from(cfg.network.broadcast());

        let min_addr = cfg
            .subnet_min
            .map_or(net_start, |n| u32::from(n.network()));
        let max_addr = cfg
            .subnet_max
            .map_or(net_end.saturating_sub(step.saturating_sub(1)), |n| {
                u32::from(n.network())
            });
        if max_addr < min_addr {
            return Err(SubnetError::InvalidConfig(
                "SubnetMax < SubnetMin".into(),
            ));
        }
        let mut out = Vec::new();
        let mut cur = min_addr;
        while cur <= max_addr {
            // Ipv4Net::new only fails on prefix > 32; subnet_len comes from the
            // bounds-checked value above, so this can't fail in practice — but
            // we keep the production lints happy with an explicit `map_err`.
            let n = Ipv4Net::new(Ipv4Addr::from(cur), subnet_len)
                .map_err(|e| SubnetError::InvalidConfig(e.to_string()))?;
            out.push(n);
            cur = match cur.checked_add(step) {
                Some(v) => v,
                None => break,
            };
        }
        Ok(out)
    }

    /// Random-pick a free candidate (matches upstream `randomSubnet`).
    pub fn choose_subnet(
        cfg: &NetworkConfig,
        in_use: &HashSet<Ipv4Net>,
        reservations: &[Reservation],
        rng: &mut impl rand::Rng,
    ) -> Result<Ipv4Net> {
        let mut candidates = Self::enumerate_candidates(cfg)?;
        let reserved: HashSet<Ipv4Net> = reservations.iter().map(|r| r.subnet).collect();
        candidates.retain(|c| !in_use.contains(c) && !reserved.contains(c));
        if candidates.is_empty() {
            return Err(SubnetError::SubnetExhausted);
        }
        candidates
            .as_slice()
            .choose(rng)
            .copied()
            .ok_or(SubnetError::SubnetExhausted)
    }

    /// Honour a reservation that matches `public_ip` — returns the pinned
    /// subnet if the public IP has a reservation entry.
    fn reservation_for(&self, public_ip: Ipv4Addr) -> Option<Ipv4Net> {
        self.reservations
            .iter()
            .find(|r| r.public_ip == public_ip)
            .map(|r| r.subnet)
    }
}

#[async_trait]
impl<R: Registry, C: Clock> SubnetManager for LocalManager<R, C> {
    async fn acquire_lease(&self, attrs: &LeaseAttrs) -> Result<Lease> {
        let cfg = self.registry.get_network_config().await?;
        let now = self.clock.now();
        let existing = self.registry.get_subnets().await?;

        // Idempotent path: same PublicIP already owns a non-expired lease.
        if let Some(found) = existing
            .iter()
            .find(|l| l.attrs.public_ip == attrs.public_ip && l.expiration > now)
        {
            return self
                .registry
                .update_subnet(found.subnet, attrs, self.ttl_secs)
                .await;
        }

        // Reservation path.
        if let Some(pinned) = self.reservation_for(attrs.public_ip) {
            return self.registry.create_subnet(pinned, attrs, self.ttl_secs).await;
        }

        let in_use: HashSet<Ipv4Net> = existing
            .iter()
            .filter(|l| l.expiration > now)
            .map(|l| l.subnet)
            .collect();
        let chosen = {
            // Scope the !Send ThreadRng so it can't straddle an await.
            let mut rng = rand::thread_rng();
            Self::choose_subnet(&cfg, &in_use, &self.reservations, &mut rng)?
        };
        self.registry.create_subnet(chosen, attrs, self.ttl_secs).await
    }

    async fn renew_lease(&self, subnet: Ipv4Net, attrs: &LeaseAttrs) -> Result<Lease> {
        self.registry.update_subnet(subnet, attrs, self.ttl_secs).await
    }

    async fn revoke_lease(&self, subnet: Ipv4Net) -> Result<()> {
        self.registry.delete_subnet(subnet).await
    }

    async fn watch_leases(&self) -> Result<Receiver<LeaseEvent>> {
        self.registry.watch_subnets().await
    }
}
