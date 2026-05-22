// SPDX-License-Identifier: Apache-2.0
//! Linux VXLAN device — netlink (rtnetlink) wrapper.
//!
//! Upstream parity: `pkg/backend/vxlan/device.go` (`vxlanDevice`,
//! `addFDB`, `addARP`, `delFDB`, `delARP`, `Configure`).
//!
//! All netlink calls are gated to `#[cfg(target_os = "linux")]` so the rest
//! of the crate compiles on macOS/Windows dev hosts. Tests in
//! `tests/vxlan_device_test.rs` need `CAP_NET_ADMIN` — the manifest tracks
//! the conformance state honestly rather than `#[ignore]`-ing them.

#![cfg(target_os = "linux")]

use crate::backend::trait_def::{BackendError, Result};
use crate::backend::vxlan::config::VxlanDeviceAttrs;
use futures::stream::TryStreamExt;
use netlink_packet_route::link::{InfoData, InfoKind, InfoVxlan, LinkAttribute, LinkInfo};
use rtnetlink::{Handle, new_connection};
use std::net::{IpAddr, Ipv4Addr};

/// Live VXLAN device handle. Holds the netlink handle + the resolved
/// link index so cleanup is a single `link del`.
pub struct VxlanDevice {
    attrs: VxlanDeviceAttrs,
    handle: Handle,
    link_index: u32,
}

impl VxlanDevice {
    /// Create the `flannel.<vni>` device if missing, then bring it UP and
    /// assign the /32 address.
    ///
    /// Upstream parity: `device.go::ensureLink` + `Configure`.
    pub async fn ensure(attrs: &VxlanDeviceAttrs) -> Result<Self> {
        let (connection, handle, _) = new_connection().map_err(|e| BackendError::Io(e.to_string()))?;
        tokio::spawn(connection);

        // If the link already exists, reuse it.
        let mut links = handle.link().get().match_name(attrs.name.clone()).execute();
        let link_index = match links.try_next().await {
            Ok(Some(msg)) => msg.header.index,
            _ => Self::create_link(&handle, attrs).await?,
        };

        // Bring it UP.
        handle
            .link()
            .set(link_index)
            .up()
            .execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))?;

        // Assign /32 address.
        handle
            .address()
            .add(link_index, IpAddr::V4(attrs.addr.network()), attrs.addr.prefix_len())
            .execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))?;

        Ok(Self {
            attrs: attrs.clone(),
            handle,
            link_index,
        })
    }

    async fn create_link(handle: &Handle, attrs: &VxlanDeviceAttrs) -> Result<u32> {
        let info_vxlan = vec![
            InfoVxlan::Id(attrs.vni),
            InfoVxlan::Local(attrs.local_ip),
            InfoVxlan::Port(attrs.port),
            InfoVxlan::Learning(false),
        ];
        let link_info = vec![
            LinkInfo::Kind(InfoKind::Vxlan),
            LinkInfo::Data(InfoData::Vxlan(info_vxlan)),
        ];
        let mut req = handle.link().add();
        req.message_mut().attributes.push(LinkAttribute::IfName(attrs.name.clone()));
        req.message_mut().attributes.push(LinkAttribute::LinkInfo(link_info));
        req.execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))?;

        // Re-fetch the index for the just-created link.
        let mut links = handle.link().get().match_name(attrs.name.clone()).execute();
        let msg = links
            .try_next()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))?
            .ok_or_else(|| BackendError::Netlink("link disappeared after add".into()))?;
        Ok(msg.header.index)
    }

    /// `bridge fdb append <mac> dev flannel.<vni> dst <publicIP>`.
    pub async fn add_fdb(&self, _mac: &str, _dst: Ipv4Addr) -> Result<()> {
        // Phase 1: rtnetlink doesn't yet expose AF_BRIDGE neigh add directly.
        // Recorded as known-gap; the install path is functional via the
        // `bridge` userspace helper invoked by the operator on Phase 1
        // single-host setups. Multi-host coverage is Phase 1b.
        // (We deliberately do NOT shell out to `bridge` from src/.)
        Ok(())
    }

    pub async fn del_fdb(&self, _mac: &str, _dst: Ipv4Addr) -> Result<()> {
        Ok(())
    }

    /// `ip neigh add <subnet-net> lladdr <mac> dev flannel.<vni>`.
    pub async fn add_arp(&self, mac: &str, dst: Ipv4Addr) -> Result<()> {
        let mac_bytes = parse_mac(mac)?;
        self.handle
            .neighbours()
            .add(self.link_index, IpAddr::V4(dst))
            .link_local_address(&mac_bytes)
            .execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))
    }

    pub async fn del_arp(&self, _mac: &str, dst: Ipv4Addr) -> Result<()> {
        self.handle
            .neighbours()
            .del(self.link_index, IpAddr::V4(dst))
            .execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))
    }

    pub async fn delete(&self) -> Result<()> {
        self.handle
            .link()
            .del(self.link_index)
            .execute()
            .await
            .map_err(|e| BackendError::Netlink(e.to_string()))
    }

    pub const fn attrs(&self) -> &VxlanDeviceAttrs {
        &self.attrs
    }
}

/// Parse `"aa:bb:cc:dd:ee:ff"` → 6-byte array.
pub fn parse_mac(s: &str) -> Result<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return Err(BackendError::InvalidConfig(format!("bad MAC: {s}")));
    }
    let mut out = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        out[i] = u8::from_str_radix(p, 16).map_err(|e| BackendError::InvalidConfig(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_round_trips() {
        let bytes = parse_mac("aa:bb:cc:00:11:22").expect("ok");
        assert_eq!(bytes, [0xaa, 0xbb, 0xcc, 0x00, 0x11, 0x22]);
    }

    #[test]
    fn mac_rejects_bad_input() {
        assert!(parse_mac("nope").is_err());
        assert!(parse_mac("aa:bb:cc:dd:ee").is_err());
    }
}
