// SPDX-License-Identifier: Apache-2.0
//! The host-gw backend — port of `pkg/backend/hostgw/hostgw.go`.
//!
//! host-gw is the no-encapsulation backend: every node routes a peer's pod
//! subnet directly via the peer's IP on the shared L2. It therefore requires
//! that the node's advertised public IP *equals* its interface IP — there can
//! be no NAT between nodes, because a direct route cannot traverse one. This
//! module ports the `New` NAT guard and the `RegisterNetwork` wiring that
//! builds the [`RouteNetwork`] and the lease attributes the node advertises.

use std::net::IpAddr;

use crate::backend::NodeBackendData;
use crate::datapath::NetError;
use crate::route_network::RouteNetwork;
use crate::subnet::NodeId;

/// The node's external interface (upstream `backend.ExternalInterface`): the
/// underlay NIC flannel binds the backend to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalInterface {
    /// Interface name (e.g. `eth0`).
    pub name: String,
    /// Kernel link index — the output interface for host-gw routes.
    pub index: i32,
    /// The address configured on the interface.
    pub iface_addr: IpAddr,
    /// The address the node advertises to peers (its public IP).
    pub ext_addr: IpAddr,
    /// The interface MTU.
    pub mtu: u32,
}

/// The host-gw backend bound to an external interface.
#[derive(Debug, Clone)]
pub struct HostGwBackend {
    ext: ExternalInterface,
}

impl HostGwBackend {
    /// Construct the backend, enforcing the no-NAT invariant.
    ///
    /// Port of `hostgw.New`: returns an error when `ExtAddr != IfaceAddr`,
    /// because that means the node is behind NAT and host-gw's direct routes
    /// cannot reach it.
    ///
    /// # Errors
    /// [`NetError::Invalid`] if the public IP differs from the interface IP.
    pub fn new(ext: ExternalInterface) -> Result<Self, NetError> {
        if ext.ext_addr != ext.iface_addr {
            return Err(NetError::Invalid(format!(
                "host-gw: public IP {} differs from interface IP {} (NAT not supported)",
                ext.ext_addr, ext.iface_addr
            )));
        }
        Ok(Self { ext })
    }

    /// The lease attributes this node advertises (upstream `LeaseAttrs` with
    /// `BackendType="host-gw"`, `PublicIP=ExtAddr`).
    #[must_use]
    pub const fn lease_attrs(&self) -> NodeBackendData {
        NodeBackendData::HostGw {
            public_ip: self.ext.ext_addr,
        }
    }

    /// host-gw does not encapsulate, so the overlay MTU is the link MTU.
    #[must_use]
    pub const fn mtu(&self) -> u32 {
        self.ext.mtu
    }

    /// Build the [`RouteNetwork`] for `local_node` (upstream `RegisterNetwork`):
    /// a host-gw route network whose routes leave via this interface.
    #[must_use]
    pub const fn register(&self, local_node: NodeId) -> RouteNetwork {
        RouteNetwork::host_gw(local_node, self.ext.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::NodeBackendData;
    use crate::cidr::Cidr;
    use crate::datapath::{MockDatapath, Op};
    use crate::routes::PeerLease;
    use crate::vxlan_network::LeaseEvent;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }

    fn iface(iface_addr: &str, ext_addr: &str) -> ExternalInterface {
        ExternalInterface {
            name: "eth0".to_owned(),
            index: 4,
            iface_addr: v4(iface_addr),
            ext_addr: v4(ext_addr),
            mtu: 1500,
        }
    }

    #[test]
    fn rejects_node_behind_nat() {
        let err = HostGwBackend::new(iface("10.0.0.5", "203.0.113.5")).expect_err("nat");
        assert!(matches!(err, NetError::Invalid(_)));
    }

    #[test]
    fn accepts_non_nat_node() {
        let be = HostGwBackend::new(iface("192.168.1.5", "192.168.1.5")).expect("ok");
        assert_eq!(be.mtu(), 1500); // no encap overhead
        assert_eq!(
            be.lease_attrs(),
            NodeBackendData::HostGw {
                public_ip: v4("192.168.1.5")
            }
        );
    }

    #[test]
    fn registered_network_routes_via_this_interface() {
        let be = HostGwBackend::new(iface("192.168.1.5", "192.168.1.5")).expect("ok");
        let mut nw = be.register("self".to_owned());
        let mut dp = MockDatapath::new();
        let peer = PeerLease {
            node: "b".to_owned(),
            subnet: Cidr::from_str("10.42.1.0/24").expect("cidr"),
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.6"),
            },
        };
        nw.handle_event(&mut dp, &LeaseEvent::Added(peer)).expect("add");
        match &dp.ops[0] {
            Op::RouteAdd(r) => assert_eq!(r.oif, 4), // the ext iface index
            other => panic!("expected RouteAdd, got {other:?}"),
        }
    }
}
