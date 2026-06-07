// SPDX-License-Identifier: Apache-2.0
//! The `flannel.<vni>` VXLAN device — port of `pkg/backend/vxlan/device.go`.
//!
//! [`VxlanDevice`] drives a [`Datapath`] to bring up and program the kernel
//! VXLAN device. It is the Rust analogue of upstream's `vxlanDevice`:
//!
//! * [`VxlanDevice::ensure`] ↔ `newVXLANDevice` / `ensureLink` — `LinkAdd` the
//!   `netlink.Vxlan` and remember its index.
//! * [`VxlanDevice::configure`] ↔ `Configure` — add the overlay address and set
//!   the link `UP`.
//! * [`VxlanDevice::add_fdb`] / [`del_fdb`](VxlanDevice::del_fdb) ↔ `AddFDB` /
//!   `DelFDB` — the `AF_BRIDGE` neighbour mapping VTEP MAC → underlay endpoint.
//! * [`VxlanDevice::add_arp`] / [`del_arp`](VxlanDevice::del_arp) ↔ `AddARP` /
//!   `DelARP` — the `AF_INET` neighbour mapping the peer's overlay gateway IP →
//!   VTEP MAC on this device.
//!
//! As in upstream, the device link MTU is the underlay MTU minus the 50-byte
//! VXLAN encapsulation overhead.

use std::net::IpAddr;

use crate::backend::{MacAddr, VxlanConfig};
use crate::datapath::{Datapath, LinkAddr, NetError, Neigh, VxlanLink};

/// The attributes needed to create the VXLAN device (upstream
/// `vxlanDeviceAttrs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanDeviceAttrs {
    /// VXLAN Network Identifier.
    pub vni: u32,
    /// Device name (e.g. `flannel.1`).
    pub name: String,
    /// The *underlay* link MTU; the device MTU is this minus the 50-byte
    /// VXLAN overhead (upstream `MTU: devAttrs.MTU - 50`).
    pub underlay_mtu: u32,
    /// Index of the underlay VTEP interface (0 = none).
    pub vtep_index: i32,
    /// The VTEP source address on the underlay.
    pub vtep_addr: Option<IpAddr>,
    /// VXLAN UDP port.
    pub vtep_port: u16,
    /// Group-based policy.
    pub gbp: bool,
    /// Kernel source-learning.
    pub learning: bool,
    /// The device's own VTEP MAC.
    pub hw_addr: MacAddr,
}

/// A peer to install an FDB / ARP entry for (upstream `neighbor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    /// The peer's VTEP MAC.
    pub mac: MacAddr,
    /// The IP for the entry: the peer's *public IP* for an FDB entry, or the
    /// peer's overlay *gateway* (`.0`) for an ARP entry.
    pub ip: IpAddr,
}

/// A live VXLAN device: a handle bound to a kernel link index, programmed via a
/// [`Datapath`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanDevice {
    /// The kernel link index assigned when the device was created.
    pub index: i32,
    /// The device's own VTEP MAC.
    pub mac: MacAddr,
    /// The device name.
    pub name: String,
    /// Whether VXLAN directRouting is enabled for this device.
    pub direct_routing: bool,
}

impl VxlanDevice {
    /// Create the VXLAN device on `dp` and return a handle to it.
    ///
    /// Port of `newVXLANDevice` → `ensureLink`: builds the `netlink.Vxlan`
    /// (link MTU = underlay MTU − [`VxlanConfig::ENCAP_OVERHEAD`]) and issues
    /// `LinkAdd`, recording the kernel-assigned index.
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the `LinkAdd`.
    pub fn ensure<D: Datapath>(dp: &mut D, attrs: &VxlanDeviceAttrs) -> Result<Self, NetError> {
        let link = VxlanLink {
            name: attrs.name.clone(),
            vni: attrs.vni,
            vtep_index: attrs.vtep_index,
            vtep_addr: attrs.vtep_addr,
            port: attrs.vtep_port,
            learning: attrs.learning,
            gbp: attrs.gbp,
            mac: attrs.hw_addr,
            mtu: attrs
                .underlay_mtu
                .saturating_sub(VxlanConfig::ENCAP_OVERHEAD),
        };
        let index = dp.link_add(&link)?;
        Ok(Self {
            index,
            mac: attrs.hw_addr,
            name: attrs.name.clone(),
            direct_routing: false,
        })
    }

    /// Add the overlay address to the device and bring it `UP`.
    ///
    /// Port of `Configure`: `EnsureV4AddressOnLink` then `LinkSetUp`. The
    /// address is the node's own subnet gateway (`.0`) carried at the subnet's
    /// prefix length, matching flannel giving `flannel.1` the subnet's `.0/len`.
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the address or the up.
    pub fn configure<D: Datapath>(
        &self,
        dp: &mut D,
        addr: IpAddr,
        prefix: u8,
    ) -> Result<(), NetError> {
        dp.addr_add(&LinkAddr {
            index: self.index,
            ip: addr,
            prefix,
        })?;
        dp.link_set_up(self.index)?;
        Ok(())
    }

    /// Add an FDB entry (`AddFDB`): map a peer VTEP MAC → its underlay endpoint.
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the neighbour set.
    pub fn add_fdb<D: Datapath>(&self, dp: &mut D, n: &Neighbor) -> Result<(), NetError> {
        dp.neigh_set(&Neigh::fdb(self.index, n.ip, n.mac))
    }

    /// Delete an FDB entry (`DelFDB`).
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the neighbour delete.
    pub fn del_fdb<D: Datapath>(&self, dp: &mut D, n: &Neighbor) -> Result<(), NetError> {
        dp.neigh_del(&Neigh::fdb(self.index, n.ip, n.mac))
    }

    /// Add an ARP entry (`AddARP`): resolve a peer overlay gateway → VTEP MAC.
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the neighbour set.
    pub fn add_arp<D: Datapath>(&self, dp: &mut D, n: &Neighbor) -> Result<(), NetError> {
        dp.neigh_set(&Neigh::arp(self.index, n.ip, n.mac))
    }

    /// Delete an ARP entry (`DelARP`).
    ///
    /// # Errors
    /// Returns [`NetError`] if the datapath rejects the neighbour delete.
    pub fn del_arp<D: Datapath>(&self, dp: &mut D, n: &Neighbor) -> Result<(), NetError> {
        dp.neigh_del(&Neigh::arp(self.index, n.ip, n.mac))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::DEFAULT_VXLAN_PORT;
    use crate::datapath::{MockDatapath, Op};
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }

    fn attrs() -> VxlanDeviceAttrs {
        VxlanDeviceAttrs {
            vni: 1,
            name: "flannel.1".to_owned(),
            underlay_mtu: 1500,
            vtep_index: 2,
            vtep_addr: Some(v4("192.168.1.10")),
            vtep_port: DEFAULT_VXLAN_PORT,
            gbp: false,
            learning: false,
            hw_addr: MacAddr::new([0x0a, 1, 2, 3, 4, 5]),
        }
    }

    #[test]
    fn ensure_link_adds_device_with_overlay_mtu() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(&mut dp, &attrs()).expect("ensure");
        assert_eq!(dev.index, 1);
        match &dp.ops[0] {
            Op::LinkAdd(link) => {
                assert_eq!(link.name, "flannel.1");
                assert_eq!(link.vni, 1);
                // device MTU = 1500 - 50 overhead.
                assert_eq!(link.mtu, 1450);
                assert_eq!(link.port, DEFAULT_VXLAN_PORT);
            }
            other => panic!("expected LinkAdd, got {other:?}"),
        }
    }

    #[test]
    fn configure_adds_addr_then_sets_up_in_order() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(&mut dp, &attrs()).expect("ensure");
        dev.configure(&mut dp, v4("10.42.0.0"), 24).expect("configure");
        // ops: LinkAdd, AddrAdd, LinkSetUp.
        assert_eq!(dp.ops.len(), 3);
        match &dp.ops[1] {
            Op::AddrAdd(a) => {
                assert_eq!(a.index, dev.index);
                assert_eq!(a.ip, v4("10.42.0.0"));
                assert_eq!(a.prefix, 24);
            }
            other => panic!("expected AddrAdd, got {other:?}"),
        }
        assert!(matches!(dp.ops[2], Op::LinkSetUp(i) if i == dev.index));
    }

    #[test]
    fn add_fdb_uses_public_ip_and_device_index() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(&mut dp, &attrs()).expect("ensure");
        let n = Neighbor {
            mac: MacAddr::new([2; 6]),
            ip: v4("192.168.1.2"),
        };
        dev.add_fdb(&mut dp, &n).expect("fdb");
        match dp.ops.last().expect("op") {
            Op::NeighSet(neigh) => {
                assert_eq!(neigh.family, crate::netlink::AF_BRIDGE);
                assert_eq!(neigh.ifindex, dev.index);
                assert_eq!(neigh.ip, v4("192.168.1.2"));
                assert_eq!(neigh.mac, MacAddr::new([2; 6]));
            }
            other => panic!("expected NeighSet, got {other:?}"),
        }
    }

    #[test]
    fn add_arp_uses_inet_family_and_gateway_ip() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(&mut dp, &attrs()).expect("ensure");
        let n = Neighbor {
            mac: MacAddr::new([2; 6]),
            ip: v4("10.42.1.0"),
        };
        dev.add_arp(&mut dp, &n).expect("arp");
        match dp.ops.last().expect("op") {
            Op::NeighSet(neigh) => {
                assert_eq!(neigh.family, crate::netlink::AF_INET);
                assert_eq!(neigh.ip, v4("10.42.1.0"));
            }
            other => panic!("expected NeighSet, got {other:?}"),
        }
    }

    #[test]
    fn del_fdb_and_arp_emit_neigh_del() {
        let mut dp = MockDatapath::new();
        let dev = VxlanDevice::ensure(&mut dp, &attrs()).expect("ensure");
        let n = Neighbor {
            mac: MacAddr::new([2; 6]),
            ip: v4("192.168.1.2"),
        };
        dev.del_fdb(&mut dp, &n).expect("del fdb");
        dev.del_arp(
            &mut dp,
            &Neighbor {
                mac: MacAddr::new([2; 6]),
                ip: v4("10.42.1.0"),
            },
        )
        .expect("del arp");
        assert!(matches!(dp.ops[dp.ops.len() - 2], Op::NeighDel(_)));
        assert!(matches!(dp.ops[dp.ops.len() - 1], Op::NeighDel(_)));
    }
}
