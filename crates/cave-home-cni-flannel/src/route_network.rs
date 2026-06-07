// SPDX-License-Identifier: Apache-2.0
//! The generic route-programming network — port of
//! `pkg/backend/route_network.go`.
//!
//! host-gw (and ipip / directRouting) backends do not encapsulate: they just
//! install a kernel route per peer subnet whose next-hop is the peer's public
//! IP on the shared L2. [`RouteNetwork`] is the Rust analogue of upstream's
//! `RouteNetwork` struct + `handleSubnetEvents` / `routeAdd` / `routeEqual` /
//! `checkSubnetExistInRoutes`:
//!
//! * it keeps the *tracked route list* (`n.routes`) the daemon believes it has
//!   installed;
//! * on an add it applies the `routeAdd` reconcile — if a route to the same
//!   destination already exists with a different gateway/link it is deleted and
//!   replaced, an identical one is skipped, otherwise it is added;
//! * on a remove it deletes the route and drops it from the tracked list;
//! * [`RouteNetwork::reconcile`] is the periodic `routeCheck` that re-adds any
//!   tracked route the kernel has since lost.
//!
//! All programming goes through the [`Datapath`] seam.

use crate::backend::NodeBackendData;
use crate::datapath::{Datapath, NetError, Route};
use crate::routes::PeerLease;
use crate::subnet::NodeId;
use crate::vxlan_network::LeaseEvent;

/// Two routes are "equal" for reconcile purposes.
///
/// True when destination, gateway and output link all match — upstream
/// `routeEqual` (Dst.IP + Gw + Dst.Mask + `LinkIndex`; our
/// [`crate::cidr::Cidr`] folds IP+mask into `dest`).
#[must_use]
pub fn route_equal(a: &Route, b: &Route) -> bool {
    a.dest == b.dest && a.gw == b.gw && a.oif == b.oif
}

/// A host-gw-style route network: programs one direct route per peer subnet.
#[derive(Debug, Clone)]
pub struct RouteNetwork {
    /// The local node (its own lease is skipped).
    pub local_node: NodeId,
    /// The output interface index every route uses (the external interface).
    pub link_index: i32,
    /// The tracked route list — what we believe is installed.
    routes: Vec<Route>,
}

impl RouteNetwork {
    /// A host-gw route network for `local_node` whose routes leave via
    /// `link_index` (the external interface).
    #[must_use]
    pub const fn host_gw(local_node: NodeId, link_index: i32) -> Self {
        Self {
            local_node,
            link_index,
            routes: Vec::new(),
        }
    }

    /// The route this network would install for `lease` — upstream `GetRoute`:
    /// `Dst=subnet, Gw=peer public IP, LinkIndex=ext iface`.
    #[must_use]
    pub const fn get_route(&self, lease: &PeerLease) -> Option<Route> {
        match &lease.data {
            NodeBackendData::HostGw { public_ip } => {
                Some(Route::host_gw(lease.subnet, *public_ip, self.link_index))
            }
            // A peer advertising a non-host-gw backend: not ours to route.
            _ => None,
        }
    }

    /// The currently-tracked routes (what the daemon believes it installed).
    #[must_use]
    pub fn tracked(&self) -> &[Route] {
        &self.routes
    }

    /// Handle one lease event (upstream `handleSubnetEvents` for one event).
    ///
    /// # Errors
    /// Returns [`NetError`] if a datapath operation fails.
    pub fn handle_event<D: Datapath>(
        &mut self,
        dp: &mut D,
        event: &LeaseEvent,
    ) -> Result<(), NetError> {
        match event {
            LeaseEvent::Added(lease) => {
                if lease.node == self.local_node {
                    return Ok(());
                }
                if let Some(route) = self.get_route(lease) {
                    self.route_add(dp, route)?;
                }
            }
            LeaseEvent::Removed(lease) => {
                if lease.node == self.local_node {
                    return Ok(());
                }
                if let Some(route) = self.get_route(lease) {
                    // Always drop from the tracked list, then delete.
                    self.routes.retain(|r| !route_equal(r, &route));
                    dp.route_del(&route)?;
                }
            }
        }
        Ok(())
    }

    /// Handle a batch of events in order.
    ///
    /// # Errors
    /// Returns the first [`NetError`]; preceding events are already applied.
    pub fn handle_events<D: Datapath>(
        &mut self,
        dp: &mut D,
        batch: &[LeaseEvent],
    ) -> Result<(), NetError> {
        for evt in batch {
            self.handle_event(dp, evt)?;
        }
        Ok(())
    }

    /// The `routeAdd` reconcile: add `route`, replacing any stale route to the
    /// same destination and skipping an identical one.
    fn route_add<D: Datapath>(&mut self, dp: &mut D, route: Route) -> Result<(), NetError> {
        // Track it (addToRouteList: no duplicates by full equality).
        if !self.routes.iter().any(|r| route_equal(r, &route)) {
            // A different route to the same destination is stale — replace it.
            if let Some(pos) = self.routes.iter().position(|r| r.dest == route.dest) {
                let stale = self.routes.remove(pos);
                dp.route_del(&stale)?;
            }
            dp.route_add(&route)?;
            self.routes.push(route);
        }
        Ok(())
    }

    /// The periodic `routeCheck` recovery: re-add any tracked route that is not
    /// present in `present` (the kernel's current route list).
    ///
    /// # Errors
    /// Returns [`NetError`] if re-adding a route fails.
    pub fn reconcile<D: Datapath>(&self, dp: &mut D, present: &[Route]) -> Result<(), NetError> {
        for route in &self.routes {
            if !present.iter().any(|r| route_equal(r, route)) {
                dp.route_add(route)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::NodeBackendData;
    use crate::cidr::Cidr;
    use crate::datapath::{MockDatapath, Op};
    use std::net::{IpAddr, Ipv4Addr};
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }
    fn peer(node: &str, subnet: &str, public_ip: &str) -> PeerLease {
        PeerLease {
            node: node.to_owned(),
            subnet: cidr(subnet),
            data: NodeBackendData::HostGw {
                public_ip: v4(public_ip),
            },
        }
    }

    #[test]
    fn add_installs_direct_route_via_public_ip() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2")))
            .expect("add");
        assert_eq!(dp.ops.len(), 1);
        match &dp.ops[0] {
            Op::RouteAdd(r) => {
                assert_eq!(r.dest, cidr("10.42.1.0/24"));
                assert_eq!(r.gw, Some(v4("192.168.1.2")));
                assert_eq!(r.oif, 3);
            }
            other => panic!("expected RouteAdd, got {other:?}"),
        }
        assert_eq!(nw.tracked().len(), 1);
    }

    #[test]
    fn adding_identical_route_twice_is_idempotent() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        let e = LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2"));
        nw.handle_event(&mut dp, &e).expect("add1");
        nw.handle_event(&mut dp, &e).expect("add2");
        // second add is a no-op (already tracked, identical).
        assert_eq!(dp.ops.len(), 1);
        assert_eq!(nw.tracked().len(), 1);
    }

    #[test]
    fn changed_gateway_replaces_stale_route() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2")))
            .expect("add");
        // Same subnet, new public IP (node re-homed) → del stale, add new.
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.9")))
            .expect("readd");
        assert!(matches!(dp.ops[1], Op::RouteDel(_)));
        assert!(matches!(dp.ops[2], Op::RouteAdd(_)));
        let eff = dp.effective_routes();
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].gw, Some(v4("192.168.1.9")));
        assert_eq!(nw.tracked().len(), 1);
    }

    #[test]
    fn remove_deletes_route_and_untracks() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2")))
            .expect("add");
        nw.handle_event(&mut dp, &LeaseEvent::Removed(peer("b", "10.42.1.0/24", "192.168.1.2")))
            .expect("remove");
        assert!(matches!(dp.ops.last().expect("op"), Op::RouteDel(_)));
        assert!(nw.tracked().is_empty());
    }

    #[test]
    fn skips_own_and_non_hostgw_peers() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("self", "10.42.0.0/24", "192.168.1.1")))
            .expect("self");
        let vx = PeerLease {
            node: "c".to_owned(),
            subnet: cidr("10.42.2.0/24"),
            data: NodeBackendData::Vxlan {
                public_ip: v4("192.168.1.3"),
                vtep_mac: crate::backend::MacAddr::new([3; 6]),
            },
        };
        nw.handle_event(&mut dp, &LeaseEvent::Added(vx)).expect("vx");
        assert!(dp.ops.is_empty());
    }

    #[test]
    fn reconcile_readds_only_missing_routes() {
        let mut dp = MockDatapath::new();
        let mut nw = RouteNetwork::host_gw("self".to_owned(), 3);
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("b", "10.42.1.0/24", "192.168.1.2")))
            .expect("add b");
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer("c", "10.42.2.0/24", "192.168.1.3")))
            .expect("add c");
        let added = dp.ops.len();
        // Kernel still has b's route but lost c's.
        let present = vec![Route::host_gw(cidr("10.42.1.0/24"), v4("192.168.1.2"), 3)];
        nw.reconcile(&mut dp, &present).expect("reconcile");
        // exactly one recovery RouteAdd (c).
        assert_eq!(dp.ops.len(), added + 1);
        match dp.ops.last().expect("op") {
            Op::RouteAdd(r) => assert_eq!(r.dest, cidr("10.42.2.0/24")),
            other => panic!("expected recovery RouteAdd, got {other:?}"),
        }
    }
}
