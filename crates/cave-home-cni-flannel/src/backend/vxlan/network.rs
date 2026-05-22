// SPDX-License-Identifier: Apache-2.0
//! VXLAN per-network handle.
//!
//! Upstream parity: `pkg/backend/vxlan/network.go`. The handle owns the local
//! `flannel.<vni>` device and reconciles FDB+ARP entries for remote leases.

use crate::backend::trait_def::{BackendError, BackendNetwork, Result};
use crate::backend::vxlan::config::{VtepBackendData, VxlanDeviceAttrs};
use crate::config::{NetworkConfig, VxlanBackendConfig};
use crate::subnet::lease::EventType;
use crate::subnet::{Lease, LeaseAttrs, LeaseEvent};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashSet;

#[cfg(target_os = "linux")]
use crate::backend::vxlan::device::VxlanDevice;

/// Per-network handle. `device` is `Some` only on Linux; on non-Linux the
/// trait surface still works so unit tests of the event-fanout logic compile
/// everywhere.
pub struct VxlanNetwork {
    attrs: VxlanDeviceAttrs,
    /// Public IPs we've already programmed FDB+ARP for — used so a duplicate
    /// `Added` event is a no-op.
    installed: Mutex<HashSet<std::net::Ipv4Addr>>,
    #[cfg(target_os = "linux")]
    device: Option<VxlanDevice>,
}

impl VxlanNetwork {
    pub async fn register(
        cfg: &NetworkConfig,
        vxlan_cfg: &VxlanBackendConfig,
        local_attrs: &LeaseAttrs,
        local_lease: &Lease,
    ) -> Result<Self> {
        let _ = cfg; // reserved for IPv6 / multi-network in Phase 1b
        // Underlay MTU defaults to 1500 if we can't probe — the CNI plugin
        // surfaces the value via subnet.env so misconfiguration is visible.
        let underlay_mtu = 1500;
        let attrs = VxlanDeviceAttrs::from_config(
            vxlan_cfg,
            local_attrs.public_ip,
            local_lease.subnet,
            underlay_mtu,
        );

        #[cfg(target_os = "linux")]
        {
            let device = VxlanDevice::ensure(&attrs).await?;
            return Ok(Self {
                attrs,
                installed: Mutex::new(HashSet::new()),
                device: Some(device),
            });
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Non-Linux dev path: surface the platform error per the contract.
            // `attrs` is unused on non-Linux, so silence dead-code via a `_`.
            let _ = attrs;
            Err(BackendError::UnsupportedPlatform)
        }
    }

    #[must_use]
    pub const fn attrs(&self) -> &VxlanDeviceAttrs {
        &self.attrs
    }

    /// Decode the VTEP MAC payload from a remote lease (returns `None` if the
    /// peer didn't publish one — host-gw peers in mixed clusters, for ex.).
    #[must_use]
    pub fn vtep_mac_from(lease: &Lease) -> Option<VtepBackendData> {
        lease
            .attrs
            .backend_data
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[async_trait]
impl BackendNetwork for VxlanNetwork {
    async fn handle_lease_event(&self, ev: &LeaseEvent) -> Result<()> {
        match ev.event_type {
            EventType::Added => {
                // Skip self-events (the local lease is published just like
                // remote ones; we'd otherwise install an FDB entry pointing
                // to ourselves).
                if ev.lease.attrs.public_ip == self.attrs.local_ip {
                    return Ok(());
                }
                let already = !self.installed.lock().insert(ev.lease.attrs.public_ip);
                if already {
                    return Ok(());
                }
                #[cfg(target_os = "linux")]
                if let Some(d) = &self.device {
                    let mac = Self::vtep_mac_from(&ev.lease)
                        .ok_or_else(|| {
                            BackendError::InvalidConfig("remote lease missing VtepMAC".into())
                        })?;
                    d.add_fdb(&mac.vtep_mac, ev.lease.attrs.public_ip).await?;
                    d.add_arp(&mac.vtep_mac, ev.lease.subnet.network()).await?;
                }
                Ok(())
            }
            EventType::Removed => {
                let was_installed = self.installed.lock().remove(&ev.lease.attrs.public_ip);
                if !was_installed {
                    return Ok(());
                }
                #[cfg(target_os = "linux")]
                if let Some(d) = &self.device {
                    let mac = Self::vtep_mac_from(&ev.lease);
                    if let Some(m) = mac {
                        d.del_fdb(&m.vtep_mac, ev.lease.attrs.public_ip).await?;
                        d.del_arp(&m.vtep_mac, ev.lease.subnet.network()).await?;
                    }
                }
                Ok(())
            }
        }
    }

    fn mtu(&self) -> u32 {
        self.attrs.mtu
    }

    async fn shutdown(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        if let Some(d) = &self.device {
            d.delete().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipnet::Ipv4Net;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn lease(public_ip: &str, subnet: &str, mac: Option<&str>) -> Lease {
        let backend_data = mac.map(|m| serde_json::json!({"VtepMAC": m}));
        Lease {
            subnet: Ipv4Net::from_str(subnet).expect("test subnet"),
            attrs: LeaseAttrs {
                public_ip: public_ip.parse().expect("test ip"),
                backend_type: "vxlan".into(),
                backend_data,
            },
            expiration: u64::MAX,
        }
    }

    #[test]
    fn vtep_mac_decodes() {
        let l = lease("10.0.0.2", "10.244.1.0/24", Some("aa:bb:cc:dd:ee:ff"));
        let m = VxlanNetwork::vtep_mac_from(&l).expect("mac");
        assert_eq!(m.vtep_mac, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn vtep_mac_absent_on_non_vxlan_peer() {
        let l = lease("10.0.0.2", "10.244.1.0/24", None);
        assert!(VxlanNetwork::vtep_mac_from(&l).is_none());
    }

    #[test]
    fn skips_self_lease() {
        // We can't construct a real VxlanNetwork here without a kernel, so
        // this test asserts the local-vs-remote check via the public-IP path
        // by using a hand-built network struct.
        let attrs = VxlanDeviceAttrs {
            name: "flannel.1".into(),
            vni: 1,
            port: 8472,
            local_ip: Ipv4Addr::new(10, 0, 0, 1),
            addr: Ipv4Net::from_str("10.244.0.0/32").expect("addr"),
            mtu: 1450,
            gbp: false,
            mac: None,
        };
        let n = VxlanNetwork {
            attrs,
            installed: Mutex::new(HashSet::new()),
            #[cfg(target_os = "linux")]
            device: None,
        };
        let self_lease = lease("10.0.0.1", "10.244.0.0/24", Some("aa:bb:cc:dd:ee:ff"));
        let ev = LeaseEvent {
            event_type: EventType::Added,
            lease: self_lease,
        };
        // tokio::test isn't available in unit tests without #[tokio::test];
        // we resolve the future with a current-thread runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("rt");
        rt.block_on(async {
            n.handle_lease_event(&ev).await.expect("self event");
        });
        // installed set must remain empty — the self-lease was skipped.
        assert!(n.installed.lock().is_empty());
    }
}
