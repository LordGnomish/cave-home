// SPDX-License-Identifier: Apache-2.0
//! In-memory registry — the deterministic backing store used in tests and in
//! single-node deployments.
//!
//! Upstream parity: there is no in-memory registry upstream; flannel ships
//! etcd + kube backends only. We add this so the `Registry` trait surface can
//! be exercised without a network. Behaviour mirrors `etcdv2.Registry` for the
//! operations Phase 1 cares about.

use crate::config::NetworkConfig;
use crate::subnet::clock::Clock;
use crate::subnet::errors::{Result, SubnetError};
use crate::subnet::lease::{EventType, Lease, LeaseAttrs, LeaseEvent};
use crate::subnet::registry::Registry;
use async_trait::async_trait;
use ipnet::Ipv4Net;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender, channel};

#[derive(Default)]
struct Inner {
    config: Option<NetworkConfig>,
    leases: HashMap<Ipv4Net, Lease>,
    watchers: Vec<Sender<LeaseEvent>>,
}

#[derive(Clone)]
pub struct MemRegistry<C: Clock> {
    inner: Arc<Mutex<Inner>>,
    clock: Arc<C>,
}

impl<C: Clock> MemRegistry<C> {
    pub fn new(clock: C) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            clock: Arc::new(clock),
        }
    }

    /// Pre-seed a config (handy for tests that don't want to call
    /// `put_network_config` first).
    pub fn with_config(self, cfg: NetworkConfig) -> Self {
        self.inner.lock().config = Some(cfg);
        self
    }

    fn broadcast(inner: &mut Inner, ev: LeaseEvent) {
        // Drop closed receivers.
        inner.watchers.retain(|w| {
            // try_send is fine — bounded channel, drop on full to avoid
            // blocking under test pressure.
            w.try_send(ev.clone()).is_ok() || !w.is_closed()
        });
    }
}

#[async_trait]
impl<C: Clock> Registry for MemRegistry<C> {
    async fn get_network_config(&self) -> Result<NetworkConfig> {
        self.inner
            .lock()
            .config
            .clone()
            .ok_or_else(|| SubnetError::registry("network config not initialised"))
    }

    async fn put_network_config(&self, cfg: &NetworkConfig) -> Result<()> {
        self.inner.lock().config = Some(cfg.clone());
        Ok(())
    }

    async fn get_subnets(&self) -> Result<Vec<Lease>> {
        Ok(self.inner.lock().leases.values().cloned().collect())
    }

    async fn create_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease> {
        let now = self.clock.now();
        let lease = Lease {
            subnet,
            attrs: attrs.clone(),
            expiration: now.saturating_add(ttl_secs),
        };
        let mut inner = self.inner.lock();
        if let Some(existing) = inner.leases.get(&subnet)
            && existing.expiration > now
            && existing.attrs.public_ip != attrs.public_ip
        {
            return Err(SubnetError::SubnetConflict(subnet));
        }
        inner.leases.insert(subnet, lease.clone());
        Self::broadcast(
            &mut inner,
            LeaseEvent {
                event_type: EventType::Added,
                lease: lease.clone(),
            },
        );
        Ok(lease)
    }

    async fn update_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease> {
        let now = self.clock.now();
        let mut inner = self.inner.lock();
        let Some(existing) = inner.leases.get_mut(&subnet) else {
            return Err(SubnetError::LeaseNotFound(subnet));
        };
        existing.attrs = attrs.clone();
        existing.expiration = now.saturating_add(ttl_secs);
        Ok(existing.clone())
    }

    async fn delete_subnet(&self, subnet: Ipv4Net) -> Result<()> {
        let mut inner = self.inner.lock();
        if let Some(lease) = inner.leases.remove(&subnet) {
            Self::broadcast(
                &mut inner,
                LeaseEvent {
                    event_type: EventType::Removed,
                    lease,
                },
            );
        }
        Ok(())
    }

    async fn watch_subnets(&self) -> Result<Receiver<LeaseEvent>> {
        let (tx, rx) = channel(64);
        // Replay current state as Added events so new subscribers are
        // immediately consistent with steady-state — mirrors etcdv2 watch
        // semantics where the caller first Get()s then Watch()es.
        let snapshot: Vec<Lease> = self.inner.lock().leases.values().cloned().collect();
        for lease in snapshot {
            let _ = tx
                .send(LeaseEvent {
                    event_type: EventType::Added,
                    lease,
                })
                .await;
        }
        self.inner.lock().watchers.push(tx);
        Ok(rx)
    }
}
