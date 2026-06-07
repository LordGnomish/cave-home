// SPDX-License-Identifier: Apache-2.0
//! VXLAN subnet-event handling — port of
//! `pkg/backend/vxlan/vxlan_network.go::handleSubnetEvents`.
//!
//! This is where a *lease event* becomes real kernel state. When a peer node's
//! lease is added, the local node must, in this exact order (upstream is
//! deliberate — the route is installed last so the kernel does not ARP for the
//! gateway before the neighbour entry exists):
//!
//! 1. **ARP** — map the peer's overlay gateway (`subnet.0`) → the peer's VTEP
//!    MAC, on the `flannel.<vni>` device (`AddARP`).
//! 2. **FDB** — map the peer's VTEP MAC → the peer's underlay public IP, so the
//!    kernel knows where to send the encapsulated frame (`AddFDB`).
//! 3. **route** — `subnet` via the VXLAN device, gateway `subnet.0`, on-link
//!    (`RouteReplace` of `vxlanRoute`).
//!
//! With `directRouting` and a peer on the same underlay, none of the overlay is
//! needed: a single direct route (`subnet` via the peer's public IP) replaces
//! the ARP+FDB+route trio. Removal reverses the add (delete ARP, FDB, route).
//!
//! [`VxlanNetwork::handle_event`] drives the [`Datapath`] seam, so identical
//! logic runs against the mock and a live kernel.

use crate::backend::{NodeBackendData, VxlanConfig};
use crate::cidr::Cidr;
use crate::datapath::{Datapath, NetError, Route};
use crate::device::{Neighbor, VxlanDevice};
use crate::routes::PeerLease;
use crate::subnet::NodeId;

/// A subnet-lease event the network reacts to (upstream `lease.Event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseEvent {
    /// A peer lease appeared — program the path to it.
    Added(PeerLease),
    /// A peer lease was withdrawn — tear the path down.
    Removed(PeerLease),
}

/// The running VXLAN network for the local node: the device plus the context
/// needed to react to peer lease events.
#[derive(Debug, Clone)]
pub struct VxlanNetwork {
    /// The local `flannel.<vni>` device.
    pub dev: VxlanDevice,
    /// The cluster VXLAN backend config.
    pub backend: VxlanConfig,
    /// The local node's identity (its own lease is skipped).
    pub local_node: NodeId,
    /// The local node's underlay subnet, for the `directRouting` decision.
    pub local_underlay: Option<Cidr>,
}

impl VxlanNetwork {
    /// Handle one lease event, programming or tearing down the datapath.
    ///
    /// Mirrors `handleSubnetEvents` for a single v4 event: skips the local
    /// node's own lease and any peer whose advertised backend is not VXLAN,
    /// then runs the ARP→FDB→route add (or the reverse delete), or the
    /// `directRouting` single-route fast path.
    ///
    /// # Errors
    /// Returns [`NetError`] if any datapath operation fails. On an add error
    /// upstream attempts cleanup; here the first error short-circuits and the
    /// caller (the daemon `Run` loop) logs and continues to the next event.
    pub fn handle_event<D: Datapath>(
        &self,
        dp: &mut D,
        event: &LeaseEvent,
    ) -> Result<(), NetError> {
        let (lease, added) = match event {
            LeaseEvent::Added(l) => (l, true),
            LeaseEvent::Removed(l) => (l, false),
        };

        if lease.node == self.local_node {
            return Ok(()); // never program a path to ourselves
        }
        let (public_ip, vtep_mac) = match &lease.data {
            NodeBackendData::Vxlan { public_ip, vtep_mac } => (*public_ip, *vtep_mac),
            // A peer advertising a non-VXLAN backend in a VXLAN cluster: ignore.
            _ => return Ok(()),
        };

        let subnet = lease.subnet;
        let gw = subnet.network(); // the peer's .0 overlay gateway
        let direct_ok = self.dev.direct_routing
            && self.local_underlay.is_some_and(|u| u.contains(public_ip));

        if direct_ok {
            // Same underlay → skip encapsulation, just a direct route.
            let direct = Route::host_gw(subnet, public_ip, 0);
            if added {
                dp.route_replace(&direct)?;
            } else {
                dp.route_del(&direct)?;
            }
            return Ok(());
        }

        let arp = Neighbor { mac: vtep_mac, ip: gw };
        let fdb = Neighbor { mac: vtep_mac, ip: public_ip };
        let route = Route::vxlan(subnet, gw, self.dev.index);

        if added {
            // Order matters: ARP, then FDB, then route last.
            self.dev.add_arp(dp, &arp)?;
            self.dev.add_fdb(dp, &fdb)?;
            dp.route_replace(&route)?;
        } else {
            // Tear down in any order; upstream best-efforts each.
            self.dev.del_arp(dp, &arp)?;
            self.dev.del_fdb(dp, &fdb)?;
            dp.route_del(&route)?;
        }
        Ok(())
    }

    /// Handle a batch of events in order (upstream processes a `[]lease.Event`).
    ///
    /// # Errors
    /// Returns the first [`NetError`]; preceding events are already applied.
    pub fn handle_events<D: Datapath>(
        &self,
        dp: &mut D,
        batch: &[LeaseEvent],
    ) -> Result<(), NetError> {
        for evt in batch {
            self.handle_event(dp, evt)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{MacAddr, VxlanConfig};
    use crate::datapath::{MockDatapath, Op};
    use crate::device::{VxlanDevice, VxlanDeviceAttrs};
    use std::net::{IpAddr, Ipv4Addr};
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }

    fn local_network(direct_routing: bool, underlay: Option<Cidr>) -> (MockDatapath, VxlanNetwork) {
        let mut dp = MockDatapath::new();
        let mut dev = VxlanDevice::ensure(
            &mut dp,
            &VxlanDeviceAttrs {
                vni: 1,
                name: "flannel.1".to_owned(),
                underlay_mtu: 1500,
                vtep_index: 2,
                vtep_addr: Some(v4("192.168.1.1")),
                vtep_port: 8472,
                gbp: false,
                learning: false,
                hw_addr: MacAddr::new([0x0a, 0, 0, 0, 0, 1]),
            },
        )
        .expect("ensure");
        dev.direct_routing = direct_routing;
        let nw = VxlanNetwork {
            dev,
            backend: VxlanConfig {
                direct_routing,
                ..VxlanConfig::default()
            },
            local_node: "self".to_owned(),
            local_underlay: underlay,
        };
        (dp, nw)
    }

    fn peer(node: &str, subnet: &str, public_ip: &str, mac: u8) -> PeerLease {
        PeerLease {
            node: node.to_owned(),
            subnet: cidr(subnet),
            data: NodeBackendData::Vxlan {
                public_ip: v4(public_ip),
                vtep_mac: MacAddr::new([mac; 6]),
            },
        }
    }

    #[test]
    fn add_peer_programs_arp_then_fdb_then_route_in_order() {
        let (mut dp, nw) = local_network(false, None);
        let start = dp.ops.len(); // skip the device-setup ops
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2", 2)))
            .expect("add");
        let ops = &dp.ops[start..];
        assert_eq!(ops.len(), 3);
        // 1. ARP: gateway 10.42.1.0 -> peer VTEP MAC, AF_INET.
        match &ops[0] {
            Op::NeighSet(n) => {
                assert_eq!(n.family, crate::netlink::AF_INET);
                assert_eq!(n.ip, v4("10.42.1.0"));
                assert_eq!(n.mac, MacAddr::new([2; 6]));
            }
            other => panic!("expected ARP NeighSet, got {other:?}"),
        }
        // 2. FDB: peer VTEP MAC -> public IP, AF_BRIDGE.
        match &ops[1] {
            Op::NeighSet(n) => {
                assert_eq!(n.family, crate::netlink::AF_BRIDGE);
                assert_eq!(n.ip, v4("192.168.1.2"));
            }
            other => panic!("expected FDB NeighSet, got {other:?}"),
        }
        // 3. route: 10.42.1.0/24 via vxlan dev, gw 10.42.1.0, on-link.
        match &ops[2] {
            Op::RouteReplace(r) => {
                assert_eq!(r.dest, cidr("10.42.1.0/24"));
                assert_eq!(r.gw, Some(v4("10.42.1.0")));
                assert_eq!(r.oif, nw.dev.index);
                assert_eq!(r.flags, crate::netlink::RTNH_F_ONLINK);
            }
            other => panic!("expected RouteReplace, got {other:?}"),
        }
    }

    #[test]
    fn remove_peer_deletes_arp_fdb_and_route() {
        let (mut dp, nw) = local_network(false, None);
        let start = dp.ops.len();
        nw.handle_event(
            &mut dp,
            &LeaseEvent::Removed(peer("b", "10.42.1.0/24", "192.168.1.2", 2)),
        )
        .expect("remove");
        let ops = &dp.ops[start..];
        assert_eq!(ops.len(), 3);
        assert!(matches!(ops[0], Op::NeighDel(_)));
        assert!(matches!(ops[1], Op::NeighDel(_)));
        assert!(matches!(ops[2], Op::RouteDel(_)));
    }

    #[test]
    fn skips_own_lease() {
        let (mut dp, nw) = local_network(false, None);
        let start = dp.ops.len();
        nw.handle_event(
            &mut dp,
            &LeaseEvent::Added(peer("self", "10.42.0.0/24", "192.168.1.1", 1)),
        )
        .expect("self");
        assert_eq!(dp.ops.len(), start, "no ops for our own lease");
    }

    #[test]
    fn skips_non_vxlan_peer_data() {
        let (mut dp, nw) = local_network(false, None);
        let start = dp.ops.len();
        let l = PeerLease {
            node: "b".to_owned(),
            subnet: cidr("10.42.1.0/24"),
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.2"),
            },
        };
        nw.handle_event(&mut dp, &LeaseEvent::Added(l)).expect("skip");
        assert_eq!(dp.ops.len(), start);
    }

    #[test]
    fn direct_routing_same_underlay_uses_single_direct_route() {
        let (mut dp, nw) = local_network(true, Some(cidr("192.168.1.0/24")));
        let start = dp.ops.len();
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2", 2)))
            .expect("add");
        let ops = &dp.ops[start..];
        assert_eq!(ops.len(), 1, "direct routing programs only one route");
        match &ops[0] {
            Op::RouteReplace(r) => {
                assert_eq!(r.gw, Some(v4("192.168.1.2"))); // via peer public IP
                assert_eq!(r.oif, 0); // no vxlan device
                assert_eq!(r.flags, 0); // not on-link encap
            }
            other => panic!("expected direct RouteReplace, got {other:?}"),
        }
    }

    #[test]
    fn direct_routing_different_underlay_falls_back_to_encap() {
        let (mut dp, nw) = local_network(true, Some(cidr("192.168.1.0/24")));
        let start = dp.ops.len();
        // Peer on a different underlay (10.9.x) → must encapsulate.
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "10.9.9.2", 2)))
            .expect("add");
        assert_eq!(dp.ops[start..].len(), 3, "falls back to ARP+FDB+route");
    }

    #[test]
    fn batch_processes_all_events() {
        let (mut dp, nw) = local_network(false, None);
        let start = dp.ops.len();
        let batch = vec![
            LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2", 2)),
            LeaseEvent::Added(peer("c", "10.42.2.0/24", "192.168.1.3", 3)),
        ];
        nw.handle_events(&mut dp, &batch).expect("batch");
        // two peers × (ARP+FDB+route) = 6 ops.
        assert_eq!(dp.ops[start..].len(), 6);
        // and two effective routes installed.
        assert_eq!(dp.effective_routes().len(), 2);
    }
}
