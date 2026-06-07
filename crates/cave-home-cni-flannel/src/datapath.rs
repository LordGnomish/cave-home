// SPDX-License-Identifier: Apache-2.0
//! The datapath seam: typed netlink operations + a recording mock.
//!
//! flannel's backends do not call the kernel directly — they call the
//! `vishvananda/netlink` library, which marshals typed `Link` / `Route` /
//! `Neigh` / `Addr` values into the `RTM_*` messages built in [`crate::netlink`]
//! and writes them to an `AF_NETLINK` socket. We mirror that split:
//!
//! * [`VxlanLink`], [`LinkAddr`], [`Route`], [`Neigh`] are the typed operands,
//!   each with an `encode` that produces the exact wire message (faithful,
//!   unit-tested bytes).
//! * [`Datapath`] is the trait every backend programs against.
//! * [`MockDatapath`] records each call as an [`Op`] for tests and the 2-node
//!   network simulator; the real [`crate::netlink_socket::NetlinkSocket`]
//!   (Linux) encodes and writes the same operands.
//!
//! Because the backend logic only ever sees the trait, the VXLAN / host-gw
//! event handling is identical whether it runs against a mock or a live kernel.

use std::net::IpAddr;

use crate::backend::MacAddr;
use crate::cidr::Cidr;
use crate::netlink::{
    self, ifaddrmsg, ifinfomsg, ip_octets, ndmsg, rtmsg, NlMsg, AF_BRIDGE, AF_INET, AF_INET6,
    AF_UNSPEC, IFA_ADDRESS, IFA_LOCAL, IFF_UP, IFLA_ADDRESS, IFLA_IFNAME, IFLA_INFO_DATA,
    IFLA_INFO_KIND, IFLA_LINKINFO, IFLA_MTU, IFLA_VXLAN_GBP, IFLA_VXLAN_ID, IFLA_VXLAN_LEARNING,
    IFLA_VXLAN_LINK, IFLA_VXLAN_LOCAL, IFLA_VXLAN_LOCAL6, IFLA_VXLAN_PORT, NDA_DST, NDA_LLADDR,
    NLM_F_CREATE, NLM_F_EXCL, NLM_F_REPLACE, NTF_SELF, NUD_PERMANENT, RTA_DST, RTA_GATEWAY, RTA_OIF,
    RTM_DELLINK, RTM_DELNEIGH, RTM_DELROUTE, RTM_NEWADDR, RTM_NEWLINK, RTM_NEWNEIGH, RTM_NEWROUTE,
    RTM_SETLINK, RTN_UNICAST, RTNH_F_ONLINK, RTPROT_BOOT, RT_SCOPE_UNIVERSE, RT_TABLE_MAIN,
};

/// An error from programming the datapath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The kernel rejected a netlink request (errno from the ACK).
    Netlink {
        /// What we were doing.
        op: String,
        /// The errno the kernel returned.
        errno: i32,
    },
    /// A socket-level I/O failure (open / bind / send / recv).
    Io(String),
    /// The operand was malformed (e.g. mixed v4/v6).
    Invalid(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Netlink { op, errno } => {
                write!(f, "netlink {op} failed: errno {errno}")
            }
            Self::Io(m) => write!(f, "netlink socket I/O error: {m}"),
            Self::Invalid(m) => write!(f, "invalid datapath operand: {m}"),
        }
    }
}

impl std::error::Error for NetError {}

/// The VXLAN link to create (`flannel.<vni>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VxlanLink {
    /// Device name, e.g. `flannel.1`.
    pub name: String,
    /// VXLAN Network Identifier.
    pub vni: u32,
    /// Index of the underlay (VTEP) interface, or 0 for none.
    pub vtep_index: i32,
    /// Source / VTEP address on the underlay.
    pub vtep_addr: Option<IpAddr>,
    /// UDP destination port (network order on the wire).
    pub port: u16,
    /// Kernel source-learning toggle.
    pub learning: bool,
    /// Group-based-policy extension.
    pub gbp: bool,
    /// The device's own VTEP MAC.
    pub mac: MacAddr,
    /// The overlay MTU to set on the device (already underlay-minus-overhead).
    pub mtu: u32,
}

impl VxlanLink {
    /// Encode the `RTM_NEWLINK` request that creates this VXLAN device.
    ///
    /// Mirrors `vxlan/device.go::newVXLANDevice` → `netlink.LinkAdd` of a
    /// `netlink.Vxlan{ LinkAttrs{Name,HardwareAddr,MTU}, VxlanId, VtepDevIndex,
    /// SrcAddr, Port, Learning, GBP }` with `NLM_F_CREATE | NLM_F_EXCL`.
    #[must_use]
    pub fn encode(&self, seq: u32) -> Vec<u8> {
        let mut m = NlMsg::request(RTM_NEWLINK, NLM_F_CREATE | NLM_F_EXCL);
        m.seq = seq;
        m.body.extend_from_slice(&ifinfomsg(AF_UNSPEC, 0, 0, 0));

        let mut name = self.name.clone().into_bytes();
        name.push(0);
        netlink::push_attr(&mut m.body, IFLA_IFNAME, &name);
        netlink::push_attr(&mut m.body, IFLA_ADDRESS, &self.mac.octets());
        netlink::push_attr(&mut m.body, IFLA_MTU, &self.mtu.to_le_bytes());

        netlink::push_nested(&mut m.body, IFLA_LINKINFO, |li| {
            let mut kind = b"vxlan".to_vec();
            kind.push(0);
            netlink::push_attr(li, IFLA_INFO_KIND, &kind);
            netlink::push_nested(li, IFLA_INFO_DATA, |data| {
                netlink::push_attr(data, IFLA_VXLAN_ID, &self.vni.to_le_bytes());
                if self.vtep_index > 0 {
                    netlink::push_attr(
                        data,
                        IFLA_VXLAN_LINK,
                        &(self.vtep_index as u32).to_le_bytes(),
                    );
                }
                if let Some(addr) = self.vtep_addr {
                    let local_type = if addr.is_ipv6() {
                        IFLA_VXLAN_LOCAL6
                    } else {
                        IFLA_VXLAN_LOCAL
                    };
                    netlink::push_attr(data, local_type, &ip_octets(addr));
                }
                // IFLA_VXLAN_PORT is network (big-endian) order.
                netlink::push_attr(data, IFLA_VXLAN_PORT, &self.port.to_be_bytes());
                netlink::push_attr(data, IFLA_VXLAN_LEARNING, &[u8::from(self.learning)]);
                if self.gbp {
                    netlink::push_attr(data, IFLA_VXLAN_GBP, &[]);
                }
            });
        });
        m.serialize()
    }
}

/// An address to add to a link (`RTM_NEWADDR`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkAddr {
    /// The link index.
    pub index: i32,
    /// The address.
    pub ip: IpAddr,
    /// Prefix length.
    pub prefix: u8,
}

impl LinkAddr {
    /// Encode the `RTM_NEWADDR` request (mirrors `ip.EnsureV4AddressOnLink`).
    #[must_use]
    pub fn encode(&self, seq: u32) -> Vec<u8> {
        let family = if self.ip.is_ipv6() { AF_INET6 } else { AF_INET };
        let mut m = NlMsg::request(RTM_NEWADDR, NLM_F_CREATE | NLM_F_REPLACE);
        m.seq = seq;
        m.body.extend_from_slice(&ifaddrmsg(
            family,
            self.prefix,
            0,
            RT_SCOPE_UNIVERSE,
            self.index as u32,
        ));
        netlink::push_attr(&mut m.body, IFA_LOCAL, &ip_octets(self.ip));
        netlink::push_attr(&mut m.body, IFA_ADDRESS, &ip_octets(self.ip));
        m.serialize()
    }
}

/// A route to program (`RTM_NEWROUTE` / `RTM_DELROUTE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Destination prefix.
    pub dest: Cidr,
    /// Next-hop gateway, if any.
    pub gw: Option<IpAddr>,
    /// Output interface index (0 = let the kernel resolve via the gateway).
    pub oif: i32,
    /// Route scope.
    pub scope: u8,
    /// `RTNH_F_*` flags (e.g. `RTNH_F_ONLINK`).
    pub flags: u32,
}

impl Route {
    /// The VXLAN-encapsulated route flannel installs for a peer subnet:
    /// `Dst=subnet, Gw=subnet.IP (the peer's .0 VTEP gateway), oif=vxlan dev,
    /// scope=UNIVERSE, flags=RTNH_F_ONLINK` (`vxlan_network.go::vxlanRoute`).
    #[must_use]
    pub fn vxlan(dest: Cidr, gw: IpAddr, vxlan_index: i32) -> Self {
        Self {
            dest,
            gw: Some(gw),
            oif: vxlan_index,
            scope: RT_SCOPE_UNIVERSE,
            flags: RTNH_F_ONLINK,
        }
    }

    /// The host-gw / VXLAN-directRouting route: `Dst=subnet, Gw=peer public IP,
    /// oif=ext iface` (`hostgw.go` `GetRoute` / `vxlan_network.go::directRoute`).
    #[must_use]
    pub fn host_gw(dest: Cidr, gw: IpAddr, link_index: i32) -> Self {
        Self {
            dest,
            gw: Some(gw),
            oif: link_index,
            scope: RT_SCOPE_UNIVERSE,
            flags: 0,
        }
    }

    /// Encode an `RTM_NEWROUTE` / `RTM_DELROUTE` request.
    #[must_use]
    pub fn encode(&self, new: bool, extra_flags: u16, seq: u32) -> Vec<u8> {
        let family = if self.dest.network().is_ipv6() { AF_INET6 } else { AF_INET };
        let mtype = if new { RTM_NEWROUTE } else { RTM_DELROUTE };
        let mut m = NlMsg::request(mtype, extra_flags);
        m.seq = seq;
        m.body.extend_from_slice(&rtmsg(
            family,
            self.dest.prefix_len(),
            0,
            RT_TABLE_MAIN,
            RTPROT_BOOT,
            self.scope,
            RTN_UNICAST,
            self.flags,
        ));
        netlink::push_attr(&mut m.body, RTA_DST, &ip_octets(self.dest.network()));
        if let Some(gw) = self.gw {
            netlink::push_attr(&mut m.body, RTA_GATEWAY, &ip_octets(gw));
        }
        if self.oif > 0 {
            netlink::push_attr(&mut m.body, RTA_OIF, &(self.oif as u32).to_le_bytes());
        }
        m.serialize()
    }
}

/// A neighbour entry — either an FDB (`AF_BRIDGE`) or ARP (`AF_INET`) entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neigh {
    /// Address family: `AF_BRIDGE` for FDB, `AF_INET`/`AF_INET6` for ARP/NDP.
    pub family: u8,
    /// The device index the entry is attached to (the VXLAN device).
    pub ifindex: i32,
    /// `NUD_*` state.
    pub state: u16,
    /// `NTF_*` flags.
    pub flags: u8,
    /// Neighbour type (`RTN_UNICAST` for ARP entries, 0 for FDB).
    pub ntype: u8,
    /// The neighbour IP (`NDA_DST`): peer public IP for FDB, peer .0 for ARP.
    pub ip: IpAddr,
    /// The neighbour MAC (`NDA_LLADDR`): the peer's VTEP MAC.
    pub mac: MacAddr,
}

impl Neigh {
    /// An FDB entry: `AF_BRIDGE, NUD_PERMANENT, NTF_SELF, IP=peer public IP,
    /// MAC=peer VTEP` (`device.go::AddFDB`). Maps the peer's VTEP MAC to the
    /// underlay endpoint the encapsulated frame is sent to.
    #[must_use]
    pub fn fdb(ifindex: i32, public_ip: IpAddr, mac: MacAddr) -> Self {
        Self {
            family: AF_BRIDGE,
            ifindex,
            state: NUD_PERMANENT,
            flags: NTF_SELF,
            ntype: 0,
            ip: public_ip,
            mac,
        }
    }

    /// An ARP entry: `AF_INET(6), NUD_PERMANENT, type=RTN_UNICAST, IP=peer .0
    /// VTEP gateway, MAC=peer VTEP` (`device.go::AddARP`). Resolves the remote
    /// overlay gateway IP to the remote VTEP MAC on the VXLAN device.
    #[must_use]
    pub fn arp(ifindex: i32, gw_ip: IpAddr, mac: MacAddr) -> Self {
        let family = if gw_ip.is_ipv6() { AF_INET6 } else { AF_INET };
        Self {
            family,
            ifindex,
            state: NUD_PERMANENT,
            flags: 0,
            ntype: RTN_UNICAST,
            ip: gw_ip,
            mac,
        }
    }

    /// Encode an `RTM_NEWNEIGH` / `RTM_DELNEIGH` request.
    #[must_use]
    pub fn encode(&self, new: bool, seq: u32) -> Vec<u8> {
        let mtype = if new { RTM_NEWNEIGH } else { RTM_DELNEIGH };
        let extra = if new { NLM_F_CREATE | NLM_F_REPLACE } else { 0 };
        let mut m = NlMsg::request(mtype, extra);
        m.seq = seq;
        m.body
            .extend_from_slice(&ndmsg(self.family, self.ifindex, self.state, self.flags, self.ntype));
        netlink::push_attr(&mut m.body, NDA_DST, &ip_octets(self.ip));
        netlink::push_attr(&mut m.body, NDA_LLADDR, &self.mac.octets());
        m.serialize()
    }
}

/// Encode an `RTM_SETLINK` request that brings link `index` administratively
/// up (`ifinfomsg` with `IFF_UP` set in both flags and change mask) — mirrors
/// `netlink.LinkSetUp`.
#[must_use]
pub fn encode_link_set_up(index: i32, seq: u32) -> Vec<u8> {
    let mut m = NlMsg::request(RTM_SETLINK, 0);
    m.seq = seq;
    m.body.extend_from_slice(&ifinfomsg(AF_UNSPEC, index, IFF_UP, IFF_UP));
    m.serialize()
}

/// Encode an `RTM_DELLINK` request that deletes link `index` — mirrors
/// `netlink.LinkDel`.
#[must_use]
pub fn encode_link_del(index: i32, seq: u32) -> Vec<u8> {
    let mut m = NlMsg::request(RTM_DELLINK, 0);
    m.seq = seq;
    m.body.extend_from_slice(&ifinfomsg(AF_UNSPEC, index, 0, 0));
    m.serialize()
}

/// One programming action, as recorded by [`MockDatapath`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Create a VXLAN link.
    LinkAdd(VxlanLink),
    /// Delete a link by index.
    LinkDel(i32),
    /// Bring a link up.
    LinkSetUp(i32),
    /// Add an address to a link.
    AddrAdd(LinkAddr),
    /// Add / replace a route.
    RouteReplace(Route),
    /// Add a route (failing if it exists).
    RouteAdd(Route),
    /// Delete a route.
    RouteDel(Route),
    /// Add / replace a neighbour entry.
    NeighSet(Neigh),
    /// Delete a neighbour entry.
    NeighDel(Neigh),
}

/// The datapath every flannel backend programs against.
pub trait Datapath {
    /// Create the VXLAN device. Returns the new link's index.
    fn link_add(&mut self, link: &VxlanLink) -> Result<i32, NetError>;
    /// Delete a link by index.
    fn link_del(&mut self, index: i32) -> Result<(), NetError>;
    /// Bring a link administratively up.
    fn link_set_up(&mut self, index: i32) -> Result<(), NetError>;
    /// Add an address to a link.
    fn addr_add(&mut self, addr: &LinkAddr) -> Result<(), NetError>;
    /// Add or replace a route (`netlink.RouteReplace`).
    fn route_replace(&mut self, route: &Route) -> Result<(), NetError>;
    /// Add a route, failing if an identical one exists (`netlink.RouteAdd`).
    fn route_add(&mut self, route: &Route) -> Result<(), NetError>;
    /// Delete a route (`netlink.RouteDel`).
    fn route_del(&mut self, route: &Route) -> Result<(), NetError>;
    /// Add or replace a neighbour entry (`netlink.NeighSet`).
    fn neigh_set(&mut self, neigh: &Neigh) -> Result<(), NetError>;
    /// Delete a neighbour entry (`netlink.NeighDel`).
    fn neigh_del(&mut self, neigh: &Neigh) -> Result<(), NetError>;
}

/// A [`Datapath`] that records every op instead of touching the kernel.
///
/// Used by the unit tests and the 2-node network simulator. Link indices are
/// handed out sequentially starting at `next_index` so callers can correlate a
/// `LinkAdd` with the index later used in routes / neighbours.
#[derive(Debug, Default)]
pub struct MockDatapath {
    /// Every operation, in call order.
    pub ops: Vec<Op>,
    /// The next link index `link_add` will return.
    pub next_index: i32,
}

impl MockDatapath {
    /// A fresh recorder whose first created link gets index 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            next_index: 1,
        }
    }

    /// All routes installed (via `RouteReplace` or `RouteAdd`) and not later
    /// deleted — the effective route table this datapath would hold.
    #[must_use]
    pub fn effective_routes(&self) -> Vec<Route> {
        let mut routes: Vec<Route> = Vec::new();
        for op in &self.ops {
            match op {
                Op::RouteReplace(r) | Op::RouteAdd(r) => {
                    routes.retain(|e| e.dest != r.dest);
                    routes.push(r.clone());
                }
                Op::RouteDel(r) => routes.retain(|e| e.dest != r.dest),
                _ => {}
            }
        }
        routes
    }
}

impl Datapath for MockDatapath {
    fn link_add(&mut self, link: &VxlanLink) -> Result<i32, NetError> {
        let idx = self.next_index;
        self.next_index += 1;
        self.ops.push(Op::LinkAdd(link.clone()));
        Ok(idx)
    }
    fn link_del(&mut self, index: i32) -> Result<(), NetError> {
        self.ops.push(Op::LinkDel(index));
        Ok(())
    }
    fn link_set_up(&mut self, index: i32) -> Result<(), NetError> {
        self.ops.push(Op::LinkSetUp(index));
        Ok(())
    }
    fn addr_add(&mut self, addr: &LinkAddr) -> Result<(), NetError> {
        self.ops.push(Op::AddrAdd(addr.clone()));
        Ok(())
    }
    fn route_replace(&mut self, route: &Route) -> Result<(), NetError> {
        self.ops.push(Op::RouteReplace(route.clone()));
        Ok(())
    }
    fn route_add(&mut self, route: &Route) -> Result<(), NetError> {
        self.ops.push(Op::RouteAdd(route.clone()));
        Ok(())
    }
    fn route_del(&mut self, route: &Route) -> Result<(), NetError> {
        self.ops.push(Op::RouteDel(route.clone()));
        Ok(())
    }
    fn neigh_set(&mut self, neigh: &Neigh) -> Result<(), NetError> {
        self.ops.push(Op::NeighSet(neigh.clone()));
        Ok(())
    }
    fn neigh_del(&mut self, neigh: &Neigh) -> Result<(), NetError> {
        self.ops.push(Op::NeighDel(neigh.clone()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlink::{nlmsg_align, AF_INET as F_INET};
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }

    #[test]
    fn vxlan_route_encodes_dst_gw_oif_onlink() {
        let r = Route::vxlan(cidr("10.42.1.0/24"), v4("10.42.1.0"), 7);
        let bytes = r.encode(true, NLM_F_REPLACE | NLM_F_CREATE, 3);
        // header
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_NEWROUTE);
        // rtmsg starts at offset 16.
        assert_eq!(bytes[16], F_INET); // family
        assert_eq!(bytes[17], 24); // dst_len
        assert_eq!(bytes[16 + 6], RT_SCOPE_UNIVERSE); // scope
        assert_eq!(
            u32::from_le_bytes(bytes[16 + 8..16 + 12].try_into().unwrap()),
            RTNH_F_ONLINK
        );
        // total length is self-consistent and 4-aligned.
        let total = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert_eq!(total, bytes.len());
        assert_eq!(nlmsg_align(total), total);
        // attrs present: RTA_DST(10.42.1.0), RTA_GATEWAY(10.42.1.0), RTA_OIF(7).
        let attrs = &bytes[16 + 12..];
        // first attr: len=8, type=RTA_DST, payload 10.42.1.0
        assert_eq!(attrs[2], RTA_DST as u8);
        assert_eq!(&attrs[4..8], &[10, 42, 1, 0]);
    }

    #[test]
    fn fdb_neigh_uses_bridge_family_self_flag() {
        let n = Neigh::fdb(7, v4("192.168.1.2"), MacAddr::new([1, 2, 3, 4, 5, 6]));
        assert_eq!(n.family, AF_BRIDGE);
        assert_eq!(n.flags, NTF_SELF);
        assert_eq!(n.state, NUD_PERMANENT);
        let bytes = n.encode(true, 1);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_NEWNEIGH);
        // ndmsg at 16: family, then ifindex at +4.
        assert_eq!(bytes[16], AF_BRIDGE);
        assert_eq!(i32::from_le_bytes(bytes[20..24].try_into().unwrap()), 7);
        // NDA_DST then NDA_LLADDR.
        let attrs = &bytes[16 + 12..];
        assert_eq!(attrs[2], NDA_DST as u8);
        assert_eq!(&attrs[4..8], &[192, 168, 1, 2]);
    }

    #[test]
    fn arp_neigh_uses_inet_family_unicast_type() {
        let n = Neigh::arp(7, v4("10.42.1.0"), MacAddr::new([1, 2, 3, 4, 5, 6]));
        assert_eq!(n.family, AF_INET);
        assert_eq!(n.ntype, RTN_UNICAST);
        assert_eq!(n.flags, 0);
    }

    #[test]
    fn vxlan_link_encodes_kind_and_vni() {
        let link = VxlanLink {
            name: "flannel.1".to_owned(),
            vni: 1,
            vtep_index: 2,
            vtep_addr: Some(v4("192.168.1.10")),
            port: 8472,
            learning: false,
            gbp: false,
            mac: MacAddr::new([0x0a, 1, 2, 3, 4, 5]),
            mtu: 1450,
        };
        let bytes = link.encode(5);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_NEWLINK);
        let flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        assert_ne!(flags & NLM_F_CREATE, 0);
        assert_ne!(flags & NLM_F_EXCL, 0);
        // "vxlan" and "flannel.1" appear in the attribute stream.
        let s = bytes.windows(5).any(|w| w == b"vxlan");
        assert!(s, "kind 'vxlan' must be present");
        let n = bytes.windows(9).any(|w| w == b"flannel.1");
        assert!(n, "ifname 'flannel.1' must be present");
        // VXLAN port must be encoded big-endian (8472 = 0x2118 -> 0x21,0x18).
        let be = bytes.windows(2).any(|w| w == [0x21, 0x18]);
        assert!(be, "vxlan port must be network-order");
    }

    #[test]
    fn link_set_up_sets_iff_up_in_flags_and_change() {
        let bytes = encode_link_set_up(7, 2);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_SETLINK);
        // ifinfomsg at 16: index at +4, flags at +8, change at +12.
        assert_eq!(i32::from_le_bytes(bytes[20..24].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), IFF_UP);
        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), IFF_UP);
    }

    #[test]
    fn link_del_targets_index() {
        let bytes = encode_link_del(9, 4);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_DELLINK);
        assert_eq!(i32::from_le_bytes(bytes[20..24].try_into().unwrap()), 9);
    }

    #[test]
    fn mock_records_ops_and_hands_out_indices() {
        let mut dp = MockDatapath::new();
        let link = VxlanLink {
            name: "flannel.1".to_owned(),
            vni: 1,
            vtep_index: 0,
            vtep_addr: None,
            port: 8472,
            learning: false,
            gbp: false,
            mac: MacAddr::new([0x0a, 1, 2, 3, 4, 5]),
            mtu: 1450,
        };
        let idx = dp.link_add(&link).expect("add");
        assert_eq!(idx, 1);
        dp.link_set_up(idx).expect("up");
        dp.neigh_set(&Neigh::fdb(idx, v4("192.168.1.2"), MacAddr::new([1; 6])))
            .expect("fdb");
        assert_eq!(dp.ops.len(), 3);
        assert!(matches!(dp.ops[0], Op::LinkAdd(_)));
        assert!(matches!(dp.ops[1], Op::LinkSetUp(1)));
        assert!(matches!(dp.ops[2], Op::NeighSet(_)));
    }

    #[test]
    fn effective_routes_reflects_replace_and_del() {
        let mut dp = MockDatapath::new();
        let r1 = Route::vxlan(cidr("10.42.1.0/24"), v4("10.42.1.0"), 1);
        let r2 = Route::vxlan(cidr("10.42.2.0/24"), v4("10.42.2.0"), 1);
        dp.route_replace(&r1).unwrap();
        dp.route_replace(&r2).unwrap();
        // replacing r1 with a new gw keeps a single entry for that dest.
        let r1b = Route::host_gw(cidr("10.42.1.0/24"), v4("192.168.1.2"), 9);
        dp.route_replace(&r1b).unwrap();
        assert_eq!(dp.effective_routes().len(), 2);
        dp.route_del(&r2).unwrap();
        let eff = dp.effective_routes();
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].dest, cidr("10.42.1.0/24"));
        assert_eq!(eff[0].gw, Some(v4("192.168.1.2")));
    }
}
