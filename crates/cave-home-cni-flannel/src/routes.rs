// SPDX-License-Identifier: Apache-2.0
//! Route / FDB-entry computation across the node→subnet map.
//!
//! When flannel learns a peer node's lease, it programs the local node so pod
//! traffic for that peer's subnet is forwarded correctly. The *what to
//! program* is a pure function of the lease map and the backend; the *how*
//! (issuing netlink `RTM_NEWROUTE` / `RTM_NEWNEIGH` messages) is the deferred
//! datapath layer. This module computes the former.
//!
//! - **VXLAN**: each remote subnet gets a route via the VXLAN device, plus an
//!   FDB entry mapping the remote VTEP MAC → remote public IP (so the kernel
//!   knows where to send the encapsulated frame). With `directRouting`, a peer
//!   on the same underlay subnet gets a direct route instead.
//! - **host-gw**: each remote subnet gets a route whose next-hop is the peer's
//!   public IP — no encapsulation, no FDB.
//! - **`WireGuard`**: each remote subnet routes over the tunnel device; the peer
//!   mapping (public key → allowed-IPs/endpoint) is the `WireGuard` analogue of
//!   the FDB.
//!
//! A node never programs a route to its own subnet.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::backend::{BackendConfig, MacAddr, NodeBackendData};
use crate::cidr::Cidr;
use crate::subnet::NodeId;

/// How a route to a peer subnet is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextHop {
    /// Forward over the local overlay device (VXLAN / `WireGuard`) — the kernel
    /// resolves the actual endpoint via the FDB / peer table.
    Device,
    /// Forward directly to a gateway IP on a shared L2 (host-gw, or VXLAN
    /// directRouting for a same-underlay peer).
    Gateway(IpAddr),
}

/// A route entry the local node must install for one peer subnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    /// The destination pod subnet.
    pub dest: Cidr,
    /// How to reach it.
    pub via: NextHop,
}

/// An FDB / peer entry for an encapsulating backend: how to reach the peer's
/// tunnel endpoint at L2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdbEntry {
    /// The peer's VTEP MAC (VXLAN). For other backends this is the L2 handle.
    pub mac: MacAddr,
    /// The peer's underlay/public IP the encapsulated frame is sent to.
    pub endpoint: IpAddr,
}

/// The full set of datapath entries the local node must program to reach all
/// peers. Pure data — programming it is deferred to the netlink layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutePlan {
    /// Routes for peer subnets, ordered by destination.
    pub routes: Vec<RouteEntry>,
    /// FDB / peer entries for encapsulating backends, ordered by endpoint.
    pub fdb: Vec<FdbEntry>,
}

/// One peer node's advertised lease, as seen by the local node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerLease {
    /// The peer node's identity.
    pub node: NodeId,
    /// The peer's pod subnet.
    pub subnet: Cidr,
    /// The peer's advertised backend data (VTEP MAC + public IP, etc.).
    pub data: NodeBackendData,
}

/// Compute the [`RoutePlan`] the local node (`local_node`) must install given
/// the cluster `backend` and the set of `peers` (which may include the local
/// node's own lease; it is skipped).
///
/// `local_underlay` is the local node's own underlay subnet, used only for the
/// VXLAN `directRouting` decision: a peer whose public IP is on the same
/// underlay subnet can be reached by a direct route instead of encapsulation.
#[must_use]
pub fn compute_route_plan(
    local_node: &str,
    backend: &BackendConfig,
    peers: &[PeerLease],
    local_underlay: Option<Cidr>,
) -> RoutePlan {
    let mut routes: Vec<RouteEntry> = Vec::new();
    let mut fdb: Vec<FdbEntry> = Vec::new();

    for peer in peers {
        if peer.node == local_node {
            continue; // never route to our own subnet
        }
        match (backend, &peer.data) {
            (BackendConfig::Vxlan(cfg), NodeBackendData::Vxlan { public_ip, vtep_mac }) => {
                let same_underlay = local_underlay.is_some_and(|u| u.contains(*public_ip));
                if cfg.direct_routing && same_underlay {
                    routes.push(RouteEntry {
                        dest: peer.subnet,
                        via: NextHop::Gateway(*public_ip),
                    });
                } else {
                    routes.push(RouteEntry {
                        dest: peer.subnet,
                        via: NextHop::Device,
                    });
                    fdb.push(FdbEntry {
                        mac: *vtep_mac,
                        endpoint: *public_ip,
                    });
                }
            }
            (BackendConfig::HostGw, NodeBackendData::HostGw { public_ip }) => {
                routes.push(RouteEntry {
                    dest: peer.subnet,
                    via: NextHop::Gateway(*public_ip),
                });
            }
            (BackendConfig::Wireguard(_), NodeBackendData::Wireguard { .. }) => {
                routes.push(RouteEntry {
                    dest: peer.subnet,
                    via: NextHop::Device,
                });
                // WireGuard's peer table is keyed by public key, not MAC; we do
                // not synthesise an FdbEntry for it here.
            }
            // Mismatched peer backend data: the peer advertised a different
            // backend than the cluster config. Skip — we cannot route it.
            _ => {}
        }
    }

    routes.sort_by_key(|a| a.dest);
    fdb.sort_by_key(|a| a.endpoint);
    RoutePlan { routes, fdb }
}

/// Build the `peers` slice from a node→subnet map and a node→backend-data map.
///
/// Nodes present in `subnets` but missing backend data are dropped (we cannot
/// route to an endpoint we do not know).
#[must_use]
pub fn peers_from_maps(
    subnets: &BTreeMap<NodeId, Cidr>,
    backend_data: &BTreeMap<NodeId, NodeBackendData>,
) -> Vec<PeerLease> {
    subnets
        .iter()
        .filter_map(|(node, subnet)| {
            backend_data.get(node).map(|data| PeerLease {
                node: node.clone(),
                subnet: *subnet,
                data: data.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{VxlanConfig, WireguardConfig};
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }
    fn mac(n: u8) -> MacAddr {
        MacAddr::new([n, n, n, n, n, n])
    }

    fn vxlan_peer(node: &str, subnet: &str, ip: &str, m: u8) -> PeerLease {
        PeerLease {
            node: node.to_owned(),
            subnet: cidr(subnet),
            data: NodeBackendData::Vxlan {
                public_ip: v4(ip),
                vtep_mac: mac(m),
            },
        }
    }

    #[test]
    fn vxlan_emits_route_and_fdb_per_peer() {
        let peers = vec![
            vxlan_peer("self", "10.42.0.0/24", "192.168.1.1", 1),
            vxlan_peer("b", "10.42.1.0/24", "192.168.1.2", 2),
            vxlan_peer("c", "10.42.2.0/24", "192.168.1.3", 3),
        ];
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(VxlanConfig::default()),
            &peers,
            None,
        );
        // Two peers (self excluded) → two routes + two FDB entries.
        assert_eq!(plan.routes.len(), 2);
        assert_eq!(plan.fdb.len(), 2);
        assert_eq!(plan.routes[0].dest, cidr("10.42.1.0/24"));
        assert_eq!(plan.routes[0].via, NextHop::Device);
        assert_eq!(plan.fdb[0].endpoint, v4("192.168.1.2"));
        assert_eq!(plan.fdb[0].mac, mac(2));
    }

    #[test]
    fn never_routes_to_own_subnet() {
        let peers = vec![vxlan_peer("self", "10.42.0.0/24", "192.168.1.1", 1)];
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(VxlanConfig::default()),
            &peers,
            None,
        );
        assert!(plan.routes.is_empty());
        assert!(plan.fdb.is_empty());
    }

    #[test]
    fn vxlan_direct_routing_same_underlay_uses_gateway() {
        let peers = vec![vxlan_peer("b", "10.42.1.0/24", "192.168.1.2", 2)];
        let cfg = VxlanConfig {
            direct_routing: true,
            ..VxlanConfig::default()
        };
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(cfg),
            &peers,
            Some(cidr("192.168.1.0/24")),
        );
        // Same underlay subnet → direct gateway route, no FDB.
        assert_eq!(plan.routes.len(), 1);
        assert_eq!(plan.routes[0].via, NextHop::Gateway(v4("192.168.1.2")));
        assert!(plan.fdb.is_empty());
    }

    #[test]
    fn vxlan_direct_routing_different_underlay_falls_back_to_encap() {
        let peers = vec![vxlan_peer("b", "10.42.1.0/24", "10.9.9.2", 2)];
        let cfg = VxlanConfig {
            direct_routing: true,
            ..VxlanConfig::default()
        };
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(cfg),
            &peers,
            Some(cidr("192.168.1.0/24")),
        );
        // Peer is on a different underlay → must encapsulate.
        assert_eq!(plan.routes[0].via, NextHop::Device);
        assert_eq!(plan.fdb.len(), 1);
    }

    #[test]
    fn hostgw_emits_gateway_route_no_fdb() {
        let peers = vec![
            PeerLease {
                node: "b".to_owned(),
                subnet: cidr("10.42.1.0/24"),
                data: NodeBackendData::HostGw {
                    public_ip: v4("192.168.1.2"),
                },
            },
            PeerLease {
                node: "self".to_owned(),
                subnet: cidr("10.42.0.0/24"),
                data: NodeBackendData::HostGw {
                    public_ip: v4("192.168.1.1"),
                },
            },
        ];
        let plan = compute_route_plan("self", &BackendConfig::HostGw, &peers, None);
        assert_eq!(plan.routes.len(), 1);
        assert_eq!(plan.routes[0].via, NextHop::Gateway(v4("192.168.1.2")));
        assert!(plan.fdb.is_empty());
    }

    #[test]
    fn wireguard_emits_device_route_no_fdb() {
        let peers = vec![PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::Wireguard {
                public_ip: v4("192.168.1.2"),
                public_key: "key=".to_owned(),
            },
        }];
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Wireguard(WireguardConfig::default()),
            &peers,
            None,
        );
        assert_eq!(plan.routes.len(), 1);
        assert_eq!(plan.routes[0].via, NextHop::Device);
        assert!(plan.fdb.is_empty());
    }

    #[test]
    fn mismatched_backend_data_is_skipped() {
        // Cluster is VXLAN but a peer advertised host-gw data → cannot route.
        let peers = vec![PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.2"),
            },
        }];
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(VxlanConfig::default()),
            &peers,
            None,
        );
        assert!(plan.routes.is_empty());
    }

    #[test]
    fn routes_are_sorted_by_destination() {
        let peers = vec![
            vxlan_peer("c", "10.42.9.0/24", "192.168.1.9", 9),
            vxlan_peer("b", "10.42.2.0/24", "192.168.1.2", 2),
            vxlan_peer("d", "10.42.5.0/24", "192.168.1.5", 5),
        ];
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(VxlanConfig::default()),
            &peers,
            None,
        );
        let dests: Vec<_> = plan.routes.iter().map(|r| r.dest).collect();
        assert_eq!(
            dests,
            vec![cidr("10.42.2.0/24"), cidr("10.42.5.0/24"), cidr("10.42.9.0/24")]
        );
    }

    #[test]
    fn peers_from_maps_joins_subnet_and_backend_data() {
        let mut subnets = BTreeMap::new();
        subnets.insert("a".to_owned(), cidr("10.42.0.0/24"));
        subnets.insert("b".to_owned(), cidr("10.42.1.0/24"));
        let mut data = BTreeMap::new();
        data.insert(
            "a".to_owned(),
            NodeBackendData::Vxlan {
                public_ip: v4("192.168.1.1"),
                vtep_mac: mac(1),
            },
        );
        // "b" has a subnet but no backend data → dropped.
        let peers = peers_from_maps(&subnets, &data);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node, "a");
    }

    #[test]
    fn empty_cluster_yields_empty_plan() {
        let plan = compute_route_plan(
            "self",
            &BackendConfig::Vxlan(VxlanConfig::default()),
            &[],
            None,
        );
        assert_eq!(plan, RoutePlan::default());
    }
}
