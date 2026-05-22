// SPDX-License-Identifier: Apache-2.0
//! etcd v3 backed registry.
//!
//! Upstream parity: `pkg/subnet/etcdv2/registry.go`. The upstream code targets
//! the etcd v2 API (`coreos/etcd/client/v2`); we use v3 (`etcd-client` crate)
//! since v2 is removed in modern etcd. The k/v shape matches: leases live
//! under `/coreos.com/network/<network>/subnets/<subnet>` (slashes in the
//! subnet are escaped to `-`), and the network config lives at
//! `/coreos.com/network/<network>/config`.
//!
//! Linux-gated because `etcd-client` pulls TLS deps that aren't audited for
//! macOS dev. (Behaviour-equivalent test coverage is provided through the
//! `Registry` trait via `MemRegistry` — see `parity.manifest.toml`.)

#![cfg(target_os = "linux")]

use crate::config::NetworkConfig;
use crate::subnet::errors::{Result, SubnetError};
use crate::subnet::lease::{EventType, Lease, LeaseAttrs, LeaseEvent};
use crate::subnet::registry::Registry;
use async_trait::async_trait;
use etcd_client::{Client, GetOptions, PutOptions};
use ipnet::Ipv4Net;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, channel};

const PREFIX_DEFAULT: &str = "/coreos.com/network";

pub struct EtcdRegistry {
    client: Mutex<Client>,
    prefix: String,
}

impl EtcdRegistry {
    /// Connect to a list of etcd endpoints (e.g. `["http://127.0.0.1:2379"]`).
    pub async fn connect<E, S>(endpoints: E, prefix: Option<String>) -> Result<Self>
    where
        E: AsRef<[S]>,
        S: AsRef<str>,
    {
        let endpoints: Vec<String> = endpoints
            .as_ref()
            .iter()
            .map(|e| e.as_ref().to_string())
            .collect();
        let client = Client::connect(endpoints, None)
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        Ok(Self {
            client: Mutex::new(client),
            prefix: prefix.unwrap_or_else(|| PREFIX_DEFAULT.to_string()),
        })
    }

    fn config_key(&self) -> String {
        format!("{}/config", self.prefix)
    }

    fn subnet_key(&self, subnet: Ipv4Net) -> String {
        // Match upstream encoding: `/coreos.com/network/subnets/10.244.1.0-24`.
        let s = subnet.to_string().replace('/', "-");
        format!("{}/subnets/{s}", self.prefix)
    }
}

#[async_trait]
impl Registry for EtcdRegistry {
    async fn get_network_config(&self) -> Result<NetworkConfig> {
        let mut client = self.client.lock().await;
        let resp = client
            .get(self.config_key(), None)
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        let kv = resp
            .kvs()
            .first()
            .ok_or_else(|| SubnetError::registry("network config not found"))?;
        serde_json::from_slice(kv.value()).map_err(|e| SubnetError::registry(e.to_string()))
    }

    async fn put_network_config(&self, cfg: &NetworkConfig) -> Result<()> {
        let bytes = serde_json::to_vec(cfg).map_err(|e| SubnetError::registry(e.to_string()))?;
        let mut client = self.client.lock().await;
        client
            .put(self.config_key(), bytes, None)
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        Ok(())
    }

    async fn get_subnets(&self) -> Result<Vec<Lease>> {
        let mut client = self.client.lock().await;
        let resp = client
            .get(
                format!("{}/subnets/", self.prefix),
                Some(GetOptions::new().with_prefix()),
            )
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        let mut out = Vec::with_capacity(resp.kvs().len());
        for kv in resp.kvs() {
            let lease: Lease = serde_json::from_slice(kv.value())
                .map_err(|e| SubnetError::registry(e.to_string()))?;
            out.push(lease);
        }
        Ok(out)
    }

    async fn create_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease> {
        let mut client = self.client.lock().await;
        // Lease grant for TTL-based expiry — this matches the etcdv3-equivalent
        // of the upstream v2 `SetWithTTL`.
        let lease_grant = client
            .lease_grant(ttl_secs as i64, None)
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        let lease = Lease {
            subnet,
            attrs: attrs.clone(),
            // We don't ask the server for absolute time here — Phase 1 callers
            // recompute via Clock; etcd holds the authoritative TTL.
            expiration: u64::try_from(lease_grant.id()).unwrap_or(0).saturating_add(ttl_secs),
        };
        let bytes = serde_json::to_vec(&lease).map_err(|e| SubnetError::registry(e.to_string()))?;
        let opts = PutOptions::new().with_lease(lease_grant.id());
        client
            .put(self.subnet_key(subnet), bytes, Some(opts))
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        Ok(lease)
    }

    async fn update_subnet(
        &self,
        subnet: Ipv4Net,
        attrs: &LeaseAttrs,
        ttl_secs: u64,
    ) -> Result<Lease> {
        // etcd v3 doesn't have first-class "refresh"; we re-grant a lease and
        // re-put. Upstream does the same (`SetWithTTL`) via the v2 path.
        self.create_subnet(subnet, attrs, ttl_secs).await
    }

    async fn delete_subnet(&self, subnet: Ipv4Net) -> Result<()> {
        let mut client = self.client.lock().await;
        client
            .delete(self.subnet_key(subnet), None)
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        Ok(())
    }

    async fn watch_subnets(&self) -> Result<Receiver<LeaseEvent>> {
        // We bridge etcd's watch stream into a tokio mpsc so callers can stay
        // backend-agnostic. The watcher task lives until the receiver drops.
        let (tx, rx) = channel(64);
        let mut client = self.client.lock().await;
        let prefix = format!("{}/subnets/", self.prefix);
        let (_watcher, mut stream) = client
            .watch(
                prefix,
                Some(etcd_client::WatchOptions::new().with_prefix()),
            )
            .await
            .map_err(|e| SubnetError::registry(e.to_string()))?;
        drop(client);
        tokio::spawn(async move {
            while let Ok(Some(resp)) = stream.message().await {
                for ev in resp.events() {
                    let Some(kv) = ev.kv() else { continue };
                    let event_type = match ev.event_type() {
                        etcd_client::EventType::Put => EventType::Added,
                        etcd_client::EventType::Delete => EventType::Removed,
                    };
                    let Ok(lease) = serde_json::from_slice::<Lease>(kv.value()) else {
                        continue;
                    };
                    if tx.send(LeaseEvent { event_type, lease }).await.is_err() {
                        return;
                    }
                }
            }
        });
        Ok(rx)
    }
}
