// SPDX-License-Identifier: Apache-2.0
//! ARP / NDP proxy-neighbor computation for the VXLAN overlay device.
//!
//! flannel's VXLAN backend runs the `flannel.<vni>` device in L3 (connected)
//! mode: rather than flooding L2-miss events to user space, flannel installs a
//! *permanent* neighbor entry for every peer up front, so the kernel can
//! resolve a remote pod subnet's VTEP IP to its VTEP MAC without an `L3MISS`
//! round-trip. Each peer therefore needs **two** entries programmed (both
//! `RTM_NEWNEIGH`, `NUD_PERMANENT`):
//!
//! - a **bridge FDB** entry mapping the peer VTEP MAC → peer underlay/public IP
//!   — "where do I send the encapsulated frame" — computed in [`crate::routes`];
//! - an **ARP/NDP neighbor** entry mapping the peer's VTEP IP → its VTEP MAC
//!   — "what MAC owns that overlay address" — computed *here*.
//!
//! The peer's VTEP IP is the network base of its pod subnet (flannel assigns
//! `flannel.<vni>` the `.0` of the node subnet, e.g. `10.42.1.0` for
//! `10.42.1.0/24`). The resolution protocol follows the subnet family: IPv4
//! peers get an **ARP** entry, IPv6 peers an **NDP** entry. This is the
//! "ARP/NDP proxy" datapath input; issuing the actual `RTM_NEWNEIGH` netlink
//! messages is the deferred privileged layer (see `parity.manifest.toml`).
//!
//! Only VXLAN needs these neighbors. host-gw routes over a shared L2 where the
//! kernel does ordinary ARP/NDP, and `WireGuard`'s peer table is keyed by
//! public key, not MAC — both yield an empty neighbor plan. A node never
//! installs a neighbor for its own subnet, and a VXLAN `directRouting` peer on
//! the same underlay (which is reached by a direct route, not over the overlay
//! device — see [`crate::routes::compute_route_plan`]) needs no overlay
//! neighbor either.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendConfig, MacAddr, NodeBackendData, VxlanConfig, WireguardConfig};
    use crate::cidr::Cidr;
    use crate::routes::PeerLease;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from_str(s).expect("v6"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }
    fn mac(n: u8) -> MacAddr {
        MacAddr::new([n, n, n, n, n, n])
    }
    fn vxlan_peer(node: &str, subnet: &str, ip: IpAddr, m: u8) -> PeerLease {
        PeerLease {
            node: node.to_owned(),
            subnet: cidr(subnet),
            data: NodeBackendData::Vxlan {
                public_ip: ip,
                vtep_mac: mac(m),
            },
        }
    }
    fn vxlan() -> BackendConfig {
        BackendConfig::Vxlan(VxlanConfig::default())
    }

    #[test]
    fn vxlan_emits_one_permanent_neighbor_per_peer() {
        let peers = vec![
            vxlan_peer("self", "10.42.0.0/24", v4("192.168.1.1"), 1),
            vxlan_peer("b", "10.42.1.0/24", v4("192.168.1.2"), 2),
            vxlan_peer("c", "10.42.2.0/24", v4("192.168.1.3"), 3),
        ];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        // self excluded → two neighbor entries.
        assert_eq!(plan.entries.len(), 2);
        // First entry: peer "b", ARP for its VTEP IP (subnet base) → its MAC.
        assert_eq!(plan.entries[0].ip, v4("10.42.1.0"));
        assert_eq!(plan.entries[0].mac, mac(2));
        assert_eq!(plan.entries[0].family, NeighborFamily::Arp);
    }

    #[test]
    fn neighbor_ip_is_peer_subnet_network_base() {
        let peers = vec![vxlan_peer("b", "10.42.5.0/24", v4("192.168.1.5"), 5)];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        assert_eq!(plan.entries[0].ip, v4("10.42.5.0"));
    }

    #[test]
    fn never_emits_neighbor_for_own_subnet() {
        let peers = vec![vxlan_peer("self", "10.42.0.0/24", v4("192.168.1.1"), 1)];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn hostgw_emits_no_neighbors() {
        let peers = vec![PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.2"),
            },
        }];
        let plan = compute_neighbor_plan("self", &BackendConfig::HostGw, &peers, None);
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn wireguard_emits_no_neighbors() {
        let peers = vec![PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::Wireguard {
                public_ip: v4("192.168.1.2"),
                public_key: "key=".to_owned(),
            },
        }];
        let plan = compute_neighbor_plan(
            "self",
            &BackendConfig::Wireguard(WireguardConfig::default()),
            &peers,
            None,
        );
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn mismatched_backend_data_is_skipped() {
        // VXLAN cluster but a peer advertised host-gw data → cannot resolve.
        let peers = vec![PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.2"),
            },
        }];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn ipv6_peer_uses_ndp_family() {
        let peers = vec![vxlan_peer("b", "fd00:42:1::/64", v6("fd00:1::2"), 2)];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].ip, v6("fd00:42:1::"));
        assert_eq!(plan.entries[0].family, NeighborFamily::Ndp);
    }

    #[test]
    fn directrouting_same_underlay_skips_neighbor() {
        // directRouting + same underlay → routed directly, not over the overlay,
        // so no overlay neighbor is needed (mirrors compute_route_plan).
        let peers = vec![vxlan_peer("b", "10.42.1.0/24", v4("192.168.1.2"), 2)];
        let cfg = VxlanConfig {
            direct_routing: true,
            ..VxlanConfig::default()
        };
        let plan = compute_neighbor_plan(
            "self",
            &BackendConfig::Vxlan(cfg),
            &peers,
            Some(cidr("192.168.1.0/24")),
        );
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn directrouting_different_underlay_still_emits_neighbor() {
        let peers = vec![vxlan_peer("b", "10.42.1.0/24", v4("10.9.9.2"), 2)];
        let cfg = VxlanConfig {
            direct_routing: true,
            ..VxlanConfig::default()
        };
        let plan = compute_neighbor_plan(
            "self",
            &BackendConfig::Vxlan(cfg),
            &peers,
            Some(cidr("192.168.1.0/24")),
        );
        // Peer on a different underlay → encapsulated → needs the overlay neighbor.
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].ip, v4("10.42.1.0"));
    }

    #[test]
    fn neighbors_sorted_by_ip() {
        let peers = vec![
            vxlan_peer("c", "10.42.9.0/24", v4("192.168.1.9"), 9),
            vxlan_peer("b", "10.42.2.0/24", v4("192.168.1.2"), 2),
            vxlan_peer("d", "10.42.5.0/24", v4("192.168.1.5"), 5),
        ];
        let plan = compute_neighbor_plan("self", &vxlan(), &peers, None);
        let ips: Vec<IpAddr> = plan.entries.iter().map(|e| e.ip).collect();
        assert_eq!(ips, vec![v4("10.42.2.0"), v4("10.42.5.0"), v4("10.42.9.0")]);
    }

    #[test]
    fn empty_cluster_yields_empty_plan() {
        let plan = compute_neighbor_plan("self", &vxlan(), &[], None);
        assert_eq!(plan, NeighborPlan::default());
    }

    #[test]
    fn neighbor_family_for_ip_classifies_v4_arp_v6_ndp() {
        assert_eq!(NeighborFamily::for_ip(v4("10.0.0.1")), NeighborFamily::Arp);
        assert_eq!(NeighborFamily::for_ip(v6("fd00::1")), NeighborFamily::Ndp);
        assert!(NeighborFamily::Arp.is_arp());
        assert!(NeighborFamily::Ndp.is_ndp());
    }
}
