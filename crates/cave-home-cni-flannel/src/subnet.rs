// SPDX-License-Identifier: Apache-2.0
//! Cluster subnet management: carve a cluster pod CIDR into per-node subnets,
//! hand one out per node, track the node→subnet lease, and detect exhaustion.
//!
//! This mirrors flannel's subnet-leasing model (`pkg/subnet`): a single
//! cluster-wide pod network (e.g. `10.42.0.0/16`) is divided into fixed-size
//! per-node subnets (default `/24`); each node holds exactly one lease.
//! flannel persists leases in etcd / the Kubernetes API and renews them on a
//! TTL; that durable backend is deferred (see the parity manifest). What lives
//! here is the *allocation decision core* — pick a free subnet, record the
//! lease, release it, reject conflicts — over the in-memory lease table.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

use crate::cidr::{Cidr, CidrError};

/// An opaque node identity. In a real cluster this is the node's public IP or
/// hostname; the manager only needs it to be comparable.
pub type NodeId = String;

/// A node's lease on a per-node subnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubnetLease {
    /// The node holding the lease.
    pub node: NodeId,
    /// The per-node subnet (e.g. `10.42.3.0/24`).
    pub subnet: Cidr,
}

/// Errors from the subnet manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubnetError {
    /// The requested per-node prefix is not longer than the cluster prefix, so
    /// no subnets could be carved out.
    PrefixNotLonger { cluster: u8, node: u8 },
    /// Every per-node subnet is already leased.
    Exhausted,
    /// The node already holds a lease (use the existing one, or release first).
    AlreadyLeased { node: NodeId, subnet: Cidr },
    /// A lease was requested for a subnet that is not part of the cluster CIDR,
    /// or overlaps a different node's lease.
    NotInCluster(Cidr),
    /// The requested specific subnet is already held by a different node.
    SubnetTaken { subnet: Cidr, by: NodeId },
    /// Underlying CIDR arithmetic failed.
    Cidr(CidrError),
    /// No lease exists for the node on release.
    NoSuchLease(NodeId),
}

impl fmt::Display for SubnetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixNotLonger { cluster, node } => write!(
                f,
                "per-node prefix /{node} is not longer than cluster prefix /{cluster}"
            ),
            Self::Exhausted => write!(f, "subnet pool exhausted: no free per-node subnet"),
            Self::AlreadyLeased { node, subnet } => {
                write!(f, "node '{node}' already holds lease {subnet}")
            }
            Self::NotInCluster(c) => write!(f, "subnet {c} is not inside the cluster CIDR"),
            Self::SubnetTaken { subnet, by } => {
                write!(f, "subnet {subnet} is already leased by node '{by}'")
            }
            Self::Cidr(e) => write!(f, "cidr error: {e}"),
            Self::NoSuchLease(n) => write!(f, "no lease held by node '{n}'"),
        }
    }
}

impl std::error::Error for SubnetError {}

impl From<CidrError> for SubnetError {
    fn from(e: CidrError) -> Self {
        Self::Cidr(e)
    }
}

/// Manages the cluster pod CIDR and the per-node subnet leases.
#[derive(Debug, Clone)]
pub struct SubnetManager {
    cluster: Cidr,
    node_prefix: u8,
    /// subnet network address → lease. Ordered so allocation is deterministic
    /// (lowest free subnet first), matching the predictable behaviour callers
    /// and tests expect.
    leases: BTreeMap<IpAddr, SubnetLease>,
}

impl SubnetManager {
    /// Create a manager for `cluster`, carving per-node subnets of
    /// `node_prefix` bits (e.g. `/24`).
    ///
    /// # Errors
    /// Returns [`SubnetError::PrefixNotLonger`] if `node_prefix` is not strictly
    /// longer than the cluster prefix (which would yield zero or one subnet of
    /// the wrong size), or a CIDR error if `node_prefix` exceeds the family
    /// width.
    pub const fn new(cluster: Cidr, node_prefix: u8) -> Result<Self, SubnetError> {
        if node_prefix <= cluster.prefix_len() {
            return Err(SubnetError::PrefixNotLonger {
                cluster: cluster.prefix_len(),
                node: node_prefix,
            });
        }
        // Validate the family width up front so later allocation never errors.
        if node_prefix > Cidr::max_prefix(&cluster.network()) {
            return Err(SubnetError::Cidr(CidrError::PrefixTooLong {
                prefix: node_prefix,
                max: Cidr::max_prefix(&cluster.network()),
            }));
        }
        Ok(Self {
            cluster,
            node_prefix,
            leases: BTreeMap::new(),
        })
    }

    /// The cluster pod CIDR.
    #[must_use]
    pub const fn cluster_cidr(&self) -> Cidr {
        self.cluster
    }

    /// The per-node subnet prefix length.
    #[must_use]
    pub const fn node_prefix(&self) -> u8 {
        self.node_prefix
    }

    /// Total number of per-node subnets the cluster CIDR can hold.
    #[must_use]
    pub fn capacity(&self) -> u128 {
        1u128 << u32::from(self.node_prefix - self.cluster.prefix_len())
    }

    /// Number of subnets currently leased.
    #[must_use]
    pub fn leased_count(&self) -> usize {
        self.leases.len()
    }

    /// `true` if every per-node subnet is leased.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.leased_count() as u128 >= self.capacity()
    }

    /// The lease held by `node`, if any.
    #[must_use]
    pub fn lease_for(&self, node: &str) -> Option<&SubnetLease> {
        self.leases.values().find(|l| l.node == node)
    }

    /// All current leases, ordered by subnet network address.
    pub fn leases(&self) -> impl Iterator<Item = &SubnetLease> {
        self.leases.values()
    }

    /// Allocate the lowest free per-node subnet to `node`.
    ///
    /// Idempotent: if the node already holds a lease, that existing lease is
    /// returned unchanged rather than allocating a second one.
    ///
    /// # Errors
    /// [`SubnetError::Exhausted`] if no free subnet remains.
    pub fn allocate(&mut self, node: &str) -> Result<SubnetLease, SubnetError> {
        if let Some(existing) = self.lease_for(node) {
            return Ok(existing.clone());
        }
        let free = self
            .cluster
            .subnets(self.node_prefix)?
            .find(|s| !self.leases.contains_key(&s.network()))
            .ok_or(SubnetError::Exhausted)?;
        let lease = SubnetLease {
            node: node.to_owned(),
            subnet: free,
        };
        self.leases.insert(free.network(), lease.clone());
        Ok(lease)
    }

    /// Lease a *specific* subnet to `node` (e.g. honouring a static
    /// reservation). The subnet must be a valid per-node block inside the
    /// cluster CIDR and not already held by another node.
    ///
    /// # Errors
    /// - [`SubnetError::NotInCluster`] if the subnet is not a per-node block
    ///   inside the cluster CIDR.
    /// - [`SubnetError::SubnetTaken`] if a different node already holds it.
    /// - [`SubnetError::AlreadyLeased`] if this node already holds a different
    ///   subnet.
    pub fn reserve(&mut self, node: &str, subnet: Cidr) -> Result<SubnetLease, SubnetError> {
        if subnet.prefix_len() != self.node_prefix || !self.cluster.contains(subnet.network()) {
            return Err(SubnetError::NotInCluster(subnet));
        }
        if let Some(holder) = self.leases.get(&subnet.network()) {
            if holder.node == node {
                return Ok(holder.clone());
            }
            return Err(SubnetError::SubnetTaken {
                subnet,
                by: holder.node.clone(),
            });
        }
        if let Some(existing) = self.lease_for(node) {
            return Err(SubnetError::AlreadyLeased {
                node: node.to_owned(),
                subnet: existing.subnet,
            });
        }
        let lease = SubnetLease {
            node: node.to_owned(),
            subnet,
        };
        self.leases.insert(subnet.network(), lease.clone());
        Ok(lease)
    }

    /// Release the lease held by `node`.
    ///
    /// # Errors
    /// [`SubnetError::NoSuchLease`] if the node holds no lease.
    pub fn release(&mut self, node: &str) -> Result<SubnetLease, SubnetError> {
        let key = self
            .leases
            .iter()
            .find(|(_, l)| l.node == node)
            .map(|(k, _)| *k)
            .ok_or_else(|| SubnetError::NoSuchLease(node.to_owned()))?;
        // Key was just located, so removal always succeeds.
        self.leases
            .remove(&key)
            .ok_or_else(|| SubnetError::NoSuchLease(node.to_owned()))
    }

    /// A snapshot of the node→subnet map, suitable for feeding the route
    /// computation in [`crate::routes`].
    #[must_use]
    pub fn node_subnet_map(&self) -> BTreeMap<NodeId, Cidr> {
        self.leases
            .values()
            .map(|l| (l.node.clone(), l.subnet))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn mgr() -> SubnetManager {
        SubnetManager::new(Cidr::from_str("10.42.0.0/16").expect("c"), 24).expect("mgr")
    }

    #[test]
    fn rejects_non_longer_node_prefix() {
        let cluster = Cidr::from_str("10.42.0.0/24").expect("c");
        assert_eq!(
            SubnetManager::new(cluster, 24).err(),
            Some(SubnetError::PrefixNotLonger { cluster: 24, node: 24 })
        );
        assert!(matches!(
            SubnetManager::new(cluster, 16),
            Err(SubnetError::PrefixNotLonger { .. })
        ));
    }

    #[test]
    fn capacity_is_number_of_subnets() {
        assert_eq!(mgr().capacity(), 256);
        let m = SubnetManager::new(Cidr::from_str("10.0.0.0/8").expect("c"), 16).expect("m");
        assert_eq!(m.capacity(), 256);
    }

    #[test]
    fn allocate_hands_out_lowest_free_subnet() {
        let mut m = mgr();
        let a = m.allocate("node-a").expect("a");
        let b = m.allocate("node-b").expect("b");
        assert_eq!(a.subnet, Cidr::from_str("10.42.0.0/24").expect("c"));
        assert_eq!(b.subnet, Cidr::from_str("10.42.1.0/24").expect("c"));
        assert_eq!(m.leased_count(), 2);
    }

    #[test]
    fn allocate_is_idempotent_per_node() {
        let mut m = mgr();
        let first = m.allocate("node-a").expect("first");
        let again = m.allocate("node-a").expect("again");
        assert_eq!(first, again);
        assert_eq!(m.leased_count(), 1);
    }

    #[test]
    fn release_frees_the_subnet_for_reuse() {
        let mut m = mgr();
        let a = m.allocate("node-a").expect("a");
        m.allocate("node-b").expect("b");
        let released = m.release("node-a").expect("rel");
        assert_eq!(released, a);
        assert_eq!(m.leased_count(), 1);
        // The freed lowest subnet is handed back out to the next node.
        let c = m.allocate("node-c").expect("c");
        assert_eq!(c.subnet, a.subnet);
    }

    #[test]
    fn release_unknown_node_errors() {
        let mut m = mgr();
        assert_eq!(
            m.release("ghost"),
            Err(SubnetError::NoSuchLease("ghost".to_owned()))
        );
    }

    #[test]
    fn detects_exhaustion() {
        // Tiny cluster: /30 carved into /32 → 4 subnets.
        let mut m = SubnetManager::new(Cidr::from_str("10.99.0.0/30").expect("c"), 32).expect("m");
        assert_eq!(m.capacity(), 4);
        for i in 0..4 {
            m.allocate(&format!("node-{i}")).expect("alloc");
        }
        assert!(m.is_exhausted());
        assert_eq!(m.allocate("node-5"), Err(SubnetError::Exhausted));
    }

    #[test]
    fn exhaustion_clears_after_release() {
        let mut m = SubnetManager::new(Cidr::from_str("10.99.0.0/31").expect("c"), 32).expect("m");
        assert_eq!(m.capacity(), 2);
        m.allocate("a").expect("a");
        m.allocate("b").expect("b");
        assert!(m.is_exhausted());
        m.release("a").expect("rel");
        assert!(!m.is_exhausted());
        let c = m.allocate("c").expect("c");
        assert_eq!(c.subnet.network(), m.cluster_cidr().network());
    }

    #[test]
    fn reserve_pins_a_specific_subnet() {
        let mut m = mgr();
        let want = Cidr::from_str("10.42.50.0/24").expect("c");
        let lease = m.reserve("node-a", want).expect("reserve");
        assert_eq!(lease.subnet, want);
        // A later free allocation must skip the reserved subnet.
        let other = m.allocate("node-b").expect("b");
        assert_eq!(other.subnet, Cidr::from_str("10.42.0.0/24").expect("c"));
    }

    #[test]
    fn reserve_is_idempotent_for_same_node() {
        let mut m = mgr();
        let want = Cidr::from_str("10.42.7.0/24").expect("c");
        let a = m.reserve("node-a", want).expect("a");
        let b = m.reserve("node-a", want).expect("b");
        assert_eq!(a, b);
    }

    #[test]
    fn reserve_rejects_subnet_held_by_other() {
        let mut m = mgr();
        let want = Cidr::from_str("10.42.7.0/24").expect("c");
        m.reserve("node-a", want).expect("a");
        assert_eq!(
            m.reserve("node-b", want),
            Err(SubnetError::SubnetTaken {
                subnet: want,
                by: "node-a".to_owned()
            })
        );
    }

    #[test]
    fn reserve_rejects_subnet_outside_cluster() {
        let mut m = mgr();
        let bad = Cidr::from_str("10.43.0.0/24").expect("c");
        assert_eq!(m.reserve("node-a", bad), Err(SubnetError::NotInCluster(bad)));
    }

    #[test]
    fn reserve_rejects_wrong_prefix_length() {
        let mut m = mgr();
        let bad = Cidr::from_str("10.42.0.0/25").expect("c");
        assert_eq!(m.reserve("node-a", bad), Err(SubnetError::NotInCluster(bad)));
    }

    #[test]
    fn node_subnet_map_reflects_leases() {
        let mut m = mgr();
        m.allocate("node-a").expect("a");
        m.allocate("node-b").expect("b");
        let map = m.node_subnet_map();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("node-a"),
            Some(&Cidr::from_str("10.42.0.0/24").expect("c"))
        );
    }

    #[test]
    fn lease_for_returns_held_subnet() {
        let mut m = mgr();
        m.allocate("node-a").expect("a");
        assert!(m.lease_for("node-a").is_some());
        assert!(m.lease_for("node-z").is_none());
    }
}
