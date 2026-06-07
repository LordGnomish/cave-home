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
