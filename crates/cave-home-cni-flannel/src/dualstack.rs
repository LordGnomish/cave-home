// SPDX-License-Identifier: Apache-2.0
//! IPv4/IPv6 dual-stack pairing for subnet leasing and pod IPAM.
//!
//! In dual-stack mode flannel runs two parallel address plans: an IPv4 cluster
//! pod CIDR and an IPv6 one, each carved into per-node subnets. Every node
//! leases **both** an IPv4 and an IPv6 per-node subnet (a paired lease), and
//! every pod is handed **both** an IPv4 and an IPv6 address. This module is the
//! thin coordination layer over the single-family [`crate::subnet`] and
//! [`crate::ipam`] cores that keeps the two families in lock-step:
//!
//! - [`DualStackSubnetManager`] wraps a v4 + v6 [`crate::subnet::SubnetManager`]
//!   and leases both families to a node *atomically*: if the second family
//!   cannot be satisfied, the first is rolled back so a node never holds a
//!   half (single-family) lease. Allocation stays idempotent per node.
//! - [`DualStackIpam`] wraps a v4 + v6 [`crate::ipam::PodIpam`] and allocates a
//!   paired pod address atomically, with the same rollback guarantee.
//!
//! Both constructors reject a family-swapped configuration (a v6 prefix passed
//! as the v4 plan, or vice versa). As elsewhere in this crate, this is pure
//! decision logic — no kernel or store I/O.

use std::fmt;
use std::net::IpAddr;

use crate::cidr::Cidr;
use crate::ipam::{IpamError, PodIpam};
use crate::subnet::{NodeId, SubnetError, SubnetManager};

/// A node's paired dual-stack lease: one IPv4 and one IPv6 per-node subnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualStackLease {
    /// The node holding the lease.
    pub node: NodeId,
    /// The node's IPv4 per-node subnet.
    pub v4: Cidr,
    /// The node's IPv6 per-node subnet.
    pub v6: Cidr,
}

/// A pod's paired dual-stack address: one IPv4 and one IPv6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DualStackAddr {
    /// The pod's IPv4 address.
    pub v4: IpAddr,
    /// The pod's IPv6 address.
    pub v6: IpAddr,
}

/// Errors from the dual-stack coordination layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DualStackError {
    /// The IPv4 plan was not IPv4, or the IPv6 plan was not IPv6 (families
    /// swapped).
    FamilyMismatch,
    /// The IPv4 subnet manager failed.
    V4Subnet(SubnetError),
    /// The IPv6 subnet manager failed.
    V6Subnet(SubnetError),
    /// The IPv4 pod IPAM failed.
    V4Ipam(IpamError),
    /// The IPv6 pod IPAM failed.
    V6Ipam(IpamError),
}

impl fmt::Display for DualStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FamilyMismatch => {
                write!(f, "dual-stack config family mismatch (v4 plan must be IPv4, v6 plan IPv6)")
            }
            Self::V4Subnet(e) => write!(f, "IPv4 subnet: {e}"),
            Self::V6Subnet(e) => write!(f, "IPv6 subnet: {e}"),
            Self::V4Ipam(e) => write!(f, "IPv4 IPAM: {e}"),
            Self::V6Ipam(e) => write!(f, "IPv6 IPAM: {e}"),
        }
    }
}

impl std::error::Error for DualStackError {}

/// Leases an IPv4 and an IPv6 per-node subnet to every node, in lock-step.
#[derive(Debug)]
pub struct DualStackSubnetManager {
    v4: SubnetManager,
    v6: SubnetManager,
}

impl DualStackSubnetManager {
    /// Build a dual-stack manager from an IPv4 cluster CIDR (carved into
    /// `v4_node_prefix` subnets) and an IPv6 cluster CIDR (carved into
    /// `v6_node_prefix` subnets).
    ///
    /// # Errors
    /// - [`DualStackError::FamilyMismatch`] if `v4_cluster` is not IPv4 or
    ///   `v6_cluster` is not IPv6.
    /// - [`DualStackError::V4Subnet`] / [`DualStackError::V6Subnet`] if either
    ///   single-family manager rejects its prefix.
    pub fn new(
        v4_cluster: Cidr,
        v4_node_prefix: u8,
        v6_cluster: Cidr,
        v6_node_prefix: u8,
    ) -> Result<Self, DualStackError> {
        if !v4_cluster.is_ipv4() || !v6_cluster.is_ipv6() {
            return Err(DualStackError::FamilyMismatch);
        }
        let v4 = SubnetManager::new(v4_cluster, v4_node_prefix).map_err(DualStackError::V4Subnet)?;
        let v6 = SubnetManager::new(v6_cluster, v6_node_prefix).map_err(DualStackError::V6Subnet)?;
        Ok(Self { v4, v6 })
    }

    /// The IPv4 single-family manager.
    #[must_use]
    pub const fn v4(&self) -> &SubnetManager {
        &self.v4
    }

    /// The IPv6 single-family manager.
    #[must_use]
    pub const fn v6(&self) -> &SubnetManager {
        &self.v6
    }

    /// Lease both an IPv4 and an IPv6 per-node subnet to `node`, atomically.
    ///
    /// Idempotent: a node that already holds a paired lease gets it back. If
    /// the IPv6 family cannot be satisfied, a *newly taken* IPv4 lease is
    /// rolled back so the node never holds a half (single-family) lease.
    ///
    /// # Errors
    /// [`DualStackError::V4Subnet`] / [`DualStackError::V6Subnet`] on the first
    /// family that cannot be leased.
    pub fn allocate(&mut self, node: &str) -> Result<DualStackLease, DualStackError> {
        let v4_preexisting = self.v4.lease_for(node).is_some();
        let l4 = self.v4.allocate(node).map_err(DualStackError::V4Subnet)?;
        let l6 = match self.v6.allocate(node) {
            Ok(l6) => l6,
            Err(e) => {
                // Only undo the IPv4 lease if this call created it; an existing
                // pairing's IPv4 lease must survive.
                if !v4_preexisting {
                    let _ = self.v4.release(node);
                }
                return Err(DualStackError::V6Subnet(e));
            }
        };
        Ok(DualStackLease {
            node: node.to_owned(),
            v4: l4.subnet,
            v6: l6.subnet,
        })
    }

    /// Release both families' leases held by `node`.
    ///
    /// # Errors
    /// [`DualStackError::V4Subnet`] / [`DualStackError::V6Subnet`] if either
    /// family holds no lease for the node.
    pub fn release(&mut self, node: &str) -> Result<DualStackLease, DualStackError> {
        let l4 = self.v4.release(node).map_err(DualStackError::V4Subnet)?;
        let l6 = self.v6.release(node).map_err(DualStackError::V6Subnet)?;
        Ok(DualStackLease {
            node: node.to_owned(),
            v4: l4.subnet,
            v6: l6.subnet,
        })
    }

    /// The paired lease held by `node`, or `None` if it does not hold a
    /// complete dual-stack lease.
    #[must_use]
    pub fn lease_for(&self, node: &str) -> Option<DualStackLease> {
        let l4 = self.v4.lease_for(node)?;
        let l6 = self.v6.lease_for(node)?;
        Some(DualStackLease {
            node: node.to_owned(),
            v4: l4.subnet,
            v6: l6.subnet,
        })
    }
}

/// Allocates a paired IPv4 + IPv6 pod address from a node's two subnets.
#[derive(Debug)]
pub struct DualStackIpam {
    v4: PodIpam,
    v6: PodIpam,
}

impl DualStackIpam {
    /// Build a dual-stack IPAM from a node's IPv4 and IPv6 per-node subnets.
    ///
    /// # Errors
    /// - [`DualStackError::FamilyMismatch`] if `v4_subnet` is not IPv4 or
    ///   `v6_subnet` is not IPv6.
    /// - [`DualStackError::V4Ipam`] / [`DualStackError::V6Ipam`] if either
    ///   subnet is too small to hold a usable host address.
    pub fn new(v4_subnet: Cidr, v6_subnet: Cidr) -> Result<Self, DualStackError> {
        if !v4_subnet.is_ipv4() || !v6_subnet.is_ipv6() {
            return Err(DualStackError::FamilyMismatch);
        }
        let v4 = PodIpam::new(v4_subnet).map_err(DualStackError::V4Ipam)?;
        let v6 = PodIpam::new(v6_subnet).map_err(DualStackError::V6Ipam)?;
        Ok(Self { v4, v6 })
    }

    /// The IPv4 single-family IPAM.
    #[must_use]
    pub const fn v4(&self) -> &PodIpam {
        &self.v4
    }

    /// The IPv6 single-family IPAM.
    #[must_use]
    pub const fn v6(&self) -> &PodIpam {
        &self.v6
    }

    /// Allocate one IPv4 and one IPv6 address for a pod, atomically: if the
    /// IPv6 family is full, the IPv4 address taken for the pair is freed.
    ///
    /// # Errors
    /// [`DualStackError::V4Ipam`] / [`DualStackError::V6Ipam`] on the first
    /// family with no free address.
    pub fn allocate(&mut self) -> Result<DualStackAddr, DualStackError> {
        let a4 = self.v4.allocate().map_err(DualStackError::V4Ipam)?;
        let a6 = match self.v6.allocate() {
            Ok(a6) => a6,
            Err(e) => {
                // Roll back the IPv4 address so the pair is all-or-nothing.
                let _ = self.v4.free(a4);
                return Err(DualStackError::V6Ipam(e));
            }
        };
        Ok(DualStackAddr { v4: a4, v6: a6 })
    }

    /// Free a previously allocated paired pod address (both families).
    ///
    /// # Errors
    /// [`DualStackError::V4Ipam`] / [`DualStackError::V6Ipam`] if either
    /// address was not assigned.
    pub fn free(&mut self, addr: DualStackAddr) -> Result<(), DualStackError> {
        self.v4.free(addr.v4).map_err(DualStackError::V4Ipam)?;
        self.v6.free(addr.v6).map_err(DualStackError::V6Ipam)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cidr::Cidr;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::str::FromStr;

    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }
    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from_str(s).expect("v6"))
    }

    fn mgr() -> DualStackSubnetManager {
        DualStackSubnetManager::new(cidr("10.42.0.0/16"), 24, cidr("fd00:42::/108"), 120)
            .expect("dual-stack manager")
    }

    #[test]
    fn new_pairs_v4_and_v6_clusters() {
        let m = mgr();
        assert!(m.v4().cluster_cidr().is_ipv4());
        assert!(m.v6().cluster_cidr().is_ipv6());
    }

    #[test]
    fn new_rejects_v4_cluster_that_is_ipv6() {
        let e = DualStackSubnetManager::new(cidr("fd00::/64"), 120, cidr("fd00:42::/108"), 120);
        assert_eq!(e.unwrap_err(), DualStackError::FamilyMismatch);
    }

    #[test]
    fn new_rejects_v6_cluster_that_is_ipv4() {
        let e = DualStackSubnetManager::new(cidr("10.42.0.0/16"), 24, cidr("10.43.0.0/16"), 24);
        assert_eq!(e.unwrap_err(), DualStackError::FamilyMismatch);
    }

    #[test]
    fn allocate_leases_both_families_for_a_node() {
        let mut m = mgr();
        let lease = m.allocate("node-a").expect("lease");
        assert_eq!(lease.node, "node-a");
        assert!(lease.v4.is_ipv4());
        assert!(lease.v6.is_ipv6());
        assert_eq!(lease.v4, cidr("10.42.0.0/24"));
        assert_eq!(lease.v6, cidr("fd00:42::/120"));
    }

    #[test]
    fn allocate_is_idempotent_per_node() {
        let mut m = mgr();
        let first = m.allocate("node-a").expect("first");
        let again = m.allocate("node-a").expect("again");
        assert_eq!(first, again);
    }

    #[test]
    fn distinct_nodes_get_distinct_subnets_in_both_families() {
        let mut m = mgr();
        let a = m.allocate("node-a").expect("a");
        let b = m.allocate("node-b").expect("b");
        assert_ne!(a.v4, b.v4);
        assert_ne!(a.v6, b.v6);
    }

    #[test]
    fn allocate_rolls_back_v4_when_v6_exhausted() {
        // v4 has plenty of room; v6 (/127 → two /128 subnets) runs out first.
        let mut m =
            DualStackSubnetManager::new(cidr("10.42.0.0/16"), 24, cidr("fd00::/127"), 128)
                .expect("mgr");
        m.allocate("a").expect("a");
        m.allocate("b").expect("b");
        // The third node cannot get a v6 subnet → the whole pair must fail and
        // the v4 lease it grabbed must be rolled back.
        let err = m.allocate("c").unwrap_err();
        assert!(matches!(err, DualStackError::V6Subnet(_)));
        assert!(m.v4().lease_for("c").is_none(), "v4 lease must be rolled back");
        assert!(m.v6().lease_for("c").is_none());
    }

    #[test]
    fn release_frees_both_families() {
        let mut m = mgr();
        m.allocate("node-a").expect("lease");
        let freed = m.release("node-a").expect("release");
        assert_eq!(freed.node, "node-a");
        assert!(m.lease_for("node-a").is_none());
        assert!(m.v4().lease_for("node-a").is_none());
        assert!(m.v6().lease_for("node-a").is_none());
    }

    #[test]
    fn lease_for_returns_paired_lease_or_none() {
        let mut m = mgr();
        assert!(m.lease_for("node-a").is_none());
        let allocated = m.allocate("node-a").expect("lease");
        assert_eq!(m.lease_for("node-a"), Some(allocated));
    }

    // ---- DualStackIpam ----

    #[test]
    fn ipam_new_rejects_family_mismatch() {
        let e = DualStackIpam::new(cidr("fd00:42::/120"), cidr("10.42.0.0/24"));
        assert_eq!(e.unwrap_err(), DualStackError::FamilyMismatch);
    }

    #[test]
    fn ipam_allocate_yields_one_v4_and_one_v6() {
        let mut ipam =
            DualStackIpam::new(cidr("10.42.0.0/24"), cidr("fd00:42::/120")).expect("ipam");
        let addr = ipam.allocate().expect("addr");
        // .0 network + .1 gateway reserved → first pod gets .2 in each family.
        assert_eq!(addr.v4, v4("10.42.0.2"));
        assert_eq!(addr.v6, v6("fd00:42::2"));
    }

    #[test]
    fn ipam_allocate_rolls_back_v4_when_v6_full() {
        // v6 /126 → 4 addrs, .0+.1 reserved → exactly two usable host addrs.
        let mut ipam =
            DualStackIpam::new(cidr("10.42.0.0/24"), cidr("fd00:42::/126")).expect("ipam");
        ipam.allocate().expect("1st");
        ipam.allocate().expect("2nd");
        let err = ipam.allocate().unwrap_err();
        assert!(matches!(err, DualStackError::V6Ipam(_)));
        // The v4 address grabbed for the failed pair must be returned.
        assert_eq!(ipam.v4().assigned_count(), 2, "v4 must be rolled back");
        assert_eq!(ipam.v6().assigned_count(), 2);
    }

    #[test]
    fn ipam_free_releases_both() {
        let mut ipam =
            DualStackIpam::new(cidr("10.42.0.0/24"), cidr("fd00:42::/120")).expect("ipam");
        let addr = ipam.allocate().expect("addr");
        ipam.free(addr).expect("free");
        assert_eq!(ipam.v4().assigned_count(), 0);
        assert_eq!(ipam.v6().assigned_count(), 0);
    }
}
