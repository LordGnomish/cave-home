// SPDX-License-Identifier: Apache-2.0
//! Linux rtnetlink wire codec — the real bytes the kernel expects.
//!
//! flannel's datapath is, underneath the `vishvananda/netlink` Go library, a
//! sequence of `RTM_*` messages written to an `AF_NETLINK`/`NETLINK_ROUTE`
//! socket: create the `flannel.<vni>` VXLAN link (`RTM_NEWLINK`), give it an
//! address (`RTM_NEWADDR`), bring it up (`RTM_SETLINK`), and per peer program a
//! route (`RTM_NEWROUTE`), an FDB entry (`RTM_NEWNEIGH` / `AF_BRIDGE`) and an
//! ARP entry (`RTM_NEWNEIGH` / `AF_INET`).
//!
//! This module builds those messages byte-for-byte against the kernel
//! rtnetlink ABI (uapi `linux/rtnetlink.h`, `linux/if_link.h`,
//! `linux/neighbour.h`, `linux/if_addr.h`). It is pure, allocation-only, safe
//! Rust — no I/O, no `unsafe` — so the exact wire image is unit-testable on
//! every platform. The privileged socket that writes these bytes lives behind
//! the [`crate::datapath`] seam.
//!
//! ## Byte order
//!
//! The netlink message header and the family structs (`ifinfomsg`, `rtmsg`,
//! `ndmsg`, `ifaddrmsg`) are *host* byte order; cave-home targets are all
//! little-endian (x86-64 / aarch64), so we encode them little-endian and the
//! tests are deterministic everywhere. Attribute payloads that carry network
//! values — IP addresses, the VXLAN UDP port — are network (big-endian) order
//! per the kernel ABI, regardless of host.

use std::net::IpAddr;

// ---------------------------------------------------------------------------
// Constants (Linux uapi rtnetlink ABI). Public ABI numbers, not source.
// ---------------------------------------------------------------------------

/// netlink message and attribute alignment (`NLMSG_ALIGNTO` / `RTA_ALIGNTO`).
pub const NLMSG_ALIGNTO: usize = 4;

// Message types we emit / parse.
/// `RTM_NEWLINK` — create / change a network link.
pub const RTM_NEWLINK: u16 = 16;
/// `RTM_DELLINK` — delete a network link.
pub const RTM_DELLINK: u16 = 17;
/// `RTM_GETLINK` — query a network link.
pub const RTM_GETLINK: u16 = 18;
/// `RTM_SETLINK` — change link flags (used to bring a link `UP`).
pub const RTM_SETLINK: u16 = 19;
/// `RTM_NEWADDR` — add an address to a link.
pub const RTM_NEWADDR: u16 = 20;
/// `RTM_DELADDR` — remove an address from a link.
pub const RTM_DELADDR: u16 = 21;
/// `RTM_NEWROUTE` — add / replace a route.
pub const RTM_NEWROUTE: u16 = 24;
/// `RTM_DELROUTE` — delete a route.
pub const RTM_DELROUTE: u16 = 25;
/// `RTM_GETROUTE` — query routes.
pub const RTM_GETROUTE: u16 = 26;
/// `RTM_NEWNEIGH` — add a neighbour (FDB / ARP) entry.
pub const RTM_NEWNEIGH: u16 = 28;
/// `RTM_DELNEIGH` — delete a neighbour entry.
pub const RTM_DELNEIGH: u16 = 29;

// netlink flags.
/// `NLM_F_REQUEST` — this is a request message.
pub const NLM_F_REQUEST: u16 = 0x0001;
/// `NLM_F_ACK` — request an ack on success.
pub const NLM_F_ACK: u16 = 0x0004;
/// `NLM_F_ROOT` — return the whole table (part of dump).
pub const NLM_F_ROOT: u16 = 0x0100;
/// `NLM_F_MATCH` — return all matching (part of dump).
pub const NLM_F_MATCH: u16 = 0x0200;
/// `NLM_F_DUMP` — `NLM_F_ROOT | NLM_F_MATCH`.
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;
/// `NLM_F_REPLACE` — replace an existing object.
pub const NLM_F_REPLACE: u16 = 0x0100;
/// `NLM_F_EXCL` — fail if the object already exists.
pub const NLM_F_EXCL: u16 = 0x0200;
/// `NLM_F_CREATE` — create the object if it does not exist.
pub const NLM_F_CREATE: u16 = 0x0400;
/// `NLM_F_APPEND` — append to the object list.
pub const NLM_F_APPEND: u16 = 0x0800;

// Address families.
/// `AF_UNSPEC`.
pub const AF_UNSPEC: u8 = 0;
/// `AF_INET`.
pub const AF_INET: u8 = 2;
/// `AF_BRIDGE` — the family FDB neighbour entries use.
pub const AF_BRIDGE: u8 = 7;
/// `AF_INET6`.
pub const AF_INET6: u8 = 10;

// Link interface flags (net/if.h).
/// `IFF_UP` — link administratively up.
pub const IFF_UP: u32 = 0x1;

// IFLA_* link attributes (if_link.h).
/// `IFLA_ADDRESS` — link-layer (MAC) address.
pub const IFLA_ADDRESS: u16 = 1;
/// `IFLA_IFNAME` — interface name.
pub const IFLA_IFNAME: u16 = 3;
/// `IFLA_MTU` — link MTU.
pub const IFLA_MTU: u16 = 4;
/// `IFLA_LINKINFO` — nested link-type info.
pub const IFLA_LINKINFO: u16 = 18;
/// `IFLA_INFO_KIND` — link kind string (e.g. `"vxlan"`).
pub const IFLA_INFO_KIND: u16 = 1;
/// `IFLA_INFO_DATA` — nested kind-specific data.
pub const IFLA_INFO_DATA: u16 = 2;

// IFLA_VXLAN_* attributes (if_link.h).
/// `IFLA_VXLAN_ID` — the VNI (host-order u32).
pub const IFLA_VXLAN_ID: u16 = 1;
/// `IFLA_VXLAN_LINK` — index of the underlying VTEP device.
pub const IFLA_VXLAN_LINK: u16 = 3;
/// `IFLA_VXLAN_LOCAL` — source (VTEP) IPv4 address.
pub const IFLA_VXLAN_LOCAL: u16 = 4;
/// `IFLA_VXLAN_LEARNING` — kernel source-learning toggle (u8).
pub const IFLA_VXLAN_LEARNING: u16 = 7;
/// `IFLA_VXLAN_PORT` — UDP destination port (network-order u16).
pub const IFLA_VXLAN_PORT: u16 = 15;
/// `IFLA_VXLAN_LOCAL6` — source (VTEP) IPv6 address.
pub const IFLA_VXLAN_LOCAL6: u16 = 17;
/// `IFLA_VXLAN_GBP` — group-based-policy flag (no payload).
pub const IFLA_VXLAN_GBP: u16 = 23;

// RTA_* route attributes (rtnetlink.h).
/// `RTA_DST` — destination prefix.
pub const RTA_DST: u16 = 1;
/// `RTA_OIF` — output interface index.
pub const RTA_OIF: u16 = 4;
/// `RTA_GATEWAY` — next-hop gateway.
pub const RTA_GATEWAY: u16 = 5;

// Route scope / protocol / table / type (rtnetlink.h).
/// `RT_SCOPE_UNIVERSE` — global scope.
pub const RT_SCOPE_UNIVERSE: u8 = 0;
/// `RT_SCOPE_LINK` — route valid on this link only.
pub const RT_SCOPE_LINK: u8 = 253;
/// `RT_TABLE_MAIN` — the main routing table.
pub const RT_TABLE_MAIN: u8 = 254;
/// `RTPROT_BOOT` — the protocol netlink stamps on routes added at boot.
pub const RTPROT_BOOT: u8 = 3;
/// `RTN_UNICAST` — a gatewayed unicast route / neighbour.
pub const RTN_UNICAST: u8 = 1;
/// `RTNH_F_ONLINK` — next-hop is directly reachable (no recursive lookup).
pub const RTNH_F_ONLINK: u32 = 4;

// NDA_* neighbour attributes (neighbour.h).
/// `NDA_DST` — neighbour IP.
pub const NDA_DST: u16 = 1;
/// `NDA_LLADDR` — neighbour link-layer (MAC) address.
pub const NDA_LLADDR: u16 = 2;

// NUD_* neighbour states (neighbour.h).
/// `NUD_PERMANENT` — a static, never-expiring neighbour entry.
pub const NUD_PERMANENT: u16 = 0x80;

// NTF_* neighbour flags (neighbour.h).
/// `NTF_SELF` — the entry is for this device's own FDB.
pub const NTF_SELF: u8 = 0x02;

// IFA_* address attributes (if_addr.h).
/// `IFA_ADDRESS` — the (peer) interface address.
pub const IFA_ADDRESS: u16 = 1;
/// `IFA_LOCAL` — the local interface address.
pub const IFA_LOCAL: u16 = 2;

/// Round `len` up to the netlink alignment (4 bytes).
#[must_use]
pub const fn nlmsg_align(len: usize) -> usize {
    (len + NLMSG_ALIGNTO - 1) & !(NLMSG_ALIGNTO - 1)
}

// ---------------------------------------------------------------------------
// Attribute (rtattr) encoding.
// ---------------------------------------------------------------------------

/// Append one `rtattr` (type + payload, padded to 4 bytes) to `buf`.
///
/// `rta_len` counts the 4-byte header plus the payload but *not* the trailing
/// alignment padding — exactly as the kernel ABI specifies.
pub fn push_attr(buf: &mut Vec<u8>, atype: u16, payload: &[u8]) {
    let len = 4 + payload.len();
    // rta_len: u16 — must fit. flannel attrs are tiny (≤ 16 bytes).
    buf.extend_from_slice(&(len as u16).to_le_bytes());
    buf.extend_from_slice(&atype.to_le_bytes());
    buf.extend_from_slice(payload);
    let pad = nlmsg_align(len) - len;
    buf.extend(std::iter::repeat(0u8).take(pad));
}

/// Append a nested attribute whose payload is itself a sequence of attrs.
pub fn push_nested(buf: &mut Vec<u8>, atype: u16, build: impl FnOnce(&mut Vec<u8>)) {
    let mut inner = Vec::new();
    build(&mut inner);
    push_attr(buf, atype, &inner);
}

/// Encode an [`IpAddr`] as its raw network-order octets (4 or 16 bytes).
#[must_use]
pub fn ip_octets(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(a) => a.octets().to_vec(),
        IpAddr::V6(a) => a.octets().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// netlink message framing.
// ---------------------------------------------------------------------------

/// A netlink request under construction: a 16-byte `nlmsghdr` followed by a
/// family struct and attributes.
#[derive(Debug, Clone)]
pub struct NlMsg {
    /// `nlmsg_type` — the `RTM_*` message type.
    pub msg_type: u16,
    /// `nlmsg_flags`.
    pub flags: u16,
    /// `nlmsg_seq`.
    pub seq: u32,
    /// `nlmsg_pid` (0 = kernel/auto).
    pub pid: u32,
    /// The message body: the family struct followed by aligned attributes.
    pub body: Vec<u8>,
}

impl NlMsg {
    /// A request message of `msg_type` with `NLM_F_REQUEST | NLM_F_ACK | extra`.
    #[must_use]
    pub fn request(msg_type: u16, extra_flags: u16) -> Self {
        Self {
            msg_type,
            flags: NLM_F_REQUEST | NLM_F_ACK | extra_flags,
            seq: 0,
            pid: 0,
            body: Vec::new(),
        }
    }

    /// Serialise the full message: `nlmsghdr` (with the computed `nlmsg_len`)
    /// followed by the body. The 16-byte header is already 4-aligned.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let total = 16 + self.body.len();
        let mut out = Vec::with_capacity(nlmsg_align(total));
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&self.msg_type.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.pid.to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }
}

// ---------------------------------------------------------------------------
// Family structs.
// ---------------------------------------------------------------------------

/// Encode an `ifinfomsg` (16 bytes) — the link-message family struct.
#[must_use]
pub fn ifinfomsg(family: u8, index: i32, flags: u32, change: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = family;
    // b[1] = pad, b[2..4] = ifi_type (0)
    b[4..8].copy_from_slice(&index.to_le_bytes());
    b[8..12].copy_from_slice(&flags.to_le_bytes());
    b[12..16].copy_from_slice(&change.to_le_bytes());
    b
}

/// Encode an `rtmsg` (12 bytes) — the route-message family struct.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn rtmsg(
    family: u8,
    dst_len: u8,
    tos: u8,
    table: u8,
    protocol: u8,
    scope: u8,
    rtype: u8,
    flags: u32,
) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = family;
    b[1] = dst_len;
    b[2] = 0; // src_len
    b[3] = tos;
    b[4] = table;
    b[5] = protocol;
    b[6] = scope;
    b[7] = rtype;
    b[8..12].copy_from_slice(&flags.to_le_bytes());
    b
}

/// Encode an `ndmsg` (12 bytes) — the neighbour-message family struct.
#[must_use]
pub fn ndmsg(family: u8, ifindex: i32, state: u16, flags: u8, ntype: u8) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = family;
    // b[1..4] = pad
    b[4..8].copy_from_slice(&ifindex.to_le_bytes());
    b[8..10].copy_from_slice(&state.to_le_bytes());
    b[10] = flags;
    b[11] = ntype;
    b
}

/// Encode an `ifaddrmsg` (8 bytes) — the address-message family struct.
#[must_use]
pub fn ifaddrmsg(family: u8, prefixlen: u8, flags: u8, scope: u8, index: u32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0] = family;
    b[1] = prefixlen;
    b[2] = flags;
    b[3] = scope;
    b[4..8].copy_from_slice(&index.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn alignment_rounds_up_to_four() {
        assert_eq!(nlmsg_align(0), 0);
        assert_eq!(nlmsg_align(1), 4);
        assert_eq!(nlmsg_align(4), 4);
        assert_eq!(nlmsg_align(5), 8);
        assert_eq!(nlmsg_align(16), 16);
    }

    #[test]
    fn attr_has_len_type_header_then_payload() {
        // A 4-byte u32 attribute: rta_len = 8, no padding.
        let mut buf = Vec::new();
        push_attr(&mut buf, IFLA_MTU, &1500u32.to_le_bytes());
        assert_eq!(
            buf,
            vec![
                0x08, 0x00, // rta_len = 8
                0x04, 0x00, // rta_type = IFLA_MTU (4)
                0xdc, 0x05, 0x00, 0x00, // 1500 LE
            ]
        );
    }

    #[test]
    fn attr_pads_payload_to_four_bytes() {
        // IFNAME "flannel.1" is 9 bytes (+1 NUL = 10) → rta_len = 14, padded 16.
        let mut buf = Vec::new();
        let mut name = b"flannel.1".to_vec();
        name.push(0); // NUL-terminated as the kernel expects for strings
        push_attr(&mut buf, IFLA_IFNAME, &name);
        // rta_len excludes padding: 4 + 10 = 14.
        assert_eq!(buf[0], 14);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[2], IFLA_IFNAME as u8);
        // total buffer padded to nlmsg_align(14) = 16.
        assert_eq!(buf.len(), 16);
        // the two pad bytes are zero.
        assert_eq!(&buf[14..16], &[0u8, 0u8]);
    }

    #[test]
    fn nested_attr_wraps_inner_attrs() {
        // IFLA_LINKINFO { IFLA_INFO_KIND = "vxlan" }
        let mut buf = Vec::new();
        push_nested(&mut buf, IFLA_LINKINFO, |inner| {
            let mut kind = b"vxlan".to_vec();
            kind.push(0);
            push_attr(inner, IFLA_INFO_KIND, &kind);
        });
        // inner: rta(len=4+6=10, type=1, "vxlan\0", pad 2) → aligned 12.
        // outer rta_len = 4 + 12 = 16.
        assert_eq!(buf[0], 16);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[2], IFLA_LINKINFO as u8);
        assert_eq!(buf[4], 10); // inner rta_len
        assert_eq!(buf[6], IFLA_INFO_KIND as u8);
        assert_eq!(&buf[8..13], b"vxlan");
        assert_eq!(buf.len(), 16);
    }

    #[test]
    fn message_header_carries_total_length() {
        let mut m = NlMsg::request(RTM_NEWLINK, NLM_F_CREATE | NLM_F_EXCL);
        m.body.extend_from_slice(&ifinfomsg(AF_UNSPEC, 0, 0, 0));
        let bytes = m.serialize();
        // nlmsg_len = 16 (hdr) + 16 (ifinfomsg) = 32.
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 32);
        assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), RTM_NEWLINK);
        let flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        assert_eq!(flags, NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL);
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn ifinfomsg_layout() {
        let b = ifinfomsg(AF_UNSPEC, 7, IFF_UP, IFF_UP);
        assert_eq!(b[0], AF_UNSPEC);
        assert_eq!(i32::from_le_bytes(b[4..8].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(b[8..12].try_into().unwrap()), IFF_UP);
        assert_eq!(u32::from_le_bytes(b[12..16].try_into().unwrap()), IFF_UP);
    }

    #[test]
    fn rtmsg_layout() {
        let b = rtmsg(
            AF_INET,
            24,
            0,
            RT_TABLE_MAIN,
            RTPROT_BOOT,
            RT_SCOPE_UNIVERSE,
            RTN_UNICAST,
            RTNH_F_ONLINK,
        );
        assert_eq!(b[0], AF_INET);
        assert_eq!(b[1], 24); // dst_len
        assert_eq!(b[4], RT_TABLE_MAIN);
        assert_eq!(b[5], RTPROT_BOOT);
        assert_eq!(b[6], RT_SCOPE_UNIVERSE);
        assert_eq!(b[7], RTN_UNICAST);
        assert_eq!(u32::from_le_bytes(b[8..12].try_into().unwrap()), RTNH_F_ONLINK);
    }

    #[test]
    fn ndmsg_layout_for_fdb() {
        let b = ndmsg(AF_BRIDGE, 7, NUD_PERMANENT, NTF_SELF, 0);
        assert_eq!(b[0], AF_BRIDGE);
        assert_eq!(i32::from_le_bytes(b[4..8].try_into().unwrap()), 7);
        assert_eq!(u16::from_le_bytes(b[8..10].try_into().unwrap()), NUD_PERMANENT);
        assert_eq!(b[10], NTF_SELF);
    }

    #[test]
    fn ifaddrmsg_layout() {
        let b = ifaddrmsg(AF_INET, 24, 0, RT_SCOPE_UNIVERSE, 7);
        assert_eq!(b[0], AF_INET);
        assert_eq!(b[1], 24);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 7);
    }

    #[test]
    fn ip_octets_v4_is_network_order() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(ip_octets(ip), vec![192, 168, 1, 2]);
    }
}
