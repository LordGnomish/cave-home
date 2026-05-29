// SPDX-License-Identifier: Apache-2.0
//! Per-node IPAM: allocate and free pod IPs from a node's subnet.
//!
//! Once the [`crate::subnet::SubnetManager`] has handed a node its `/24`, the
//! CNI plugin assigns individual addresses out of it to pods. This mirrors the
//! host-local IPAM behaviour the flannel CNI delegates to: reserve the network
//! address (`.0`) and the subnet gateway (`.1`), then hand out the remaining
//! usable host addresses one at a time, refusing once the subnet is full.
//!
//! Pure decision logic over an in-memory allocation set — no kernel, no veth,
//! no network programming (that is the deferred netlink layer).

use std::collections::BTreeSet;
use std::fmt;
use std::net::IpAddr;

use crate::cidr::Cidr;

/// Errors from the per-node IPAM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpamError {
    /// No free host address remains in the subnet.
    SubnetFull(Cidr),
    /// The address is not inside this IPAM's subnet.
    OutOfRange(IpAddr),
    /// The address is reserved (network or gateway) and cannot be assigned.
    Reserved(IpAddr),
    /// The address is already assigned to a pod.
    AlreadyAssigned(IpAddr),
    /// Tried to free an address that was never assigned.
    NotAssigned(IpAddr),
    /// The subnet has no usable host addresses after reservations (too small).
    NoUsableHosts(Cidr),
}

impl fmt::Display for IpamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubnetFull(c) => write!(f, "subnet {c} has no free host address"),
            Self::OutOfRange(ip) => write!(f, "address {ip} is not inside the subnet"),
            Self::Reserved(ip) => write!(f, "address {ip} is reserved (network/gateway)"),
            Self::AlreadyAssigned(ip) => write!(f, "address {ip} is already assigned"),
            Self::NotAssigned(ip) => write!(f, "address {ip} was not assigned"),
            Self::NoUsableHosts(c) => write!(f, "subnet {c} has no usable host addresses"),
        }
    }
}

impl std::error::Error for IpamError {}

/// Allocates pod IPs from a single node subnet.
///
/// Index `0` is the network address and index `1` is the gateway; both are
/// reserved. Usable host addresses run from index `2` up to (but not
/// including) the broadcast-equivalent top of the block.
#[derive(Debug, Clone)]
pub struct PodIpam {
    subnet: Cidr,
    gateway: IpAddr,
    /// Assigned host indices within the subnet (0-based from the network addr).
    assigned: BTreeSet<u128>,
    /// First usable host index (2: skip network `.0` and gateway `.1`).
    first_host: u128,
    /// One-past-last usable host index.
    host_end: u128,
}

impl PodIpam {
    /// Build an IPAM over `subnet`, reserving the network address and the
    /// gateway (the first usable address).
    ///
    /// # Errors
    /// [`IpamError::NoUsableHosts`] if the subnet is too small to hold a pod
    /// after reserving the network and gateway addresses.
    pub fn new(subnet: Cidr) -> Result<Self, IpamError> {
        let count = subnet.address_count();
        // Need at least network(.0) + gateway(.1) + one host.
        if count < 3 {
            return Err(IpamError::NoUsableHosts(subnet));
        }
        // Gateway is the first usable address (index 1), per flannel's
        // FLANNEL_SUBNET gateway convention.
        let gateway = subnet
            .nth_address(1)
            .map_err(|_| IpamError::NoUsableHosts(subnet))?;
        Ok(Self {
            subnet,
            gateway,
            assigned: BTreeSet::new(),
            first_host: 2,
            host_end: count,
        })
    }

    /// The subnet this IPAM manages.
    #[must_use]
    pub const fn subnet(&self) -> Cidr {
        self.subnet
    }

    /// The reserved gateway address (`.1` of the subnet).
    #[must_use]
    pub const fn gateway(&self) -> IpAddr {
        self.gateway
    }

    /// Number of usable host slots in the subnet.
    #[must_use]
    pub const fn usable_capacity(&self) -> u128 {
        self.host_end - self.first_host
    }

    /// Number of pod IPs currently assigned.
    #[must_use]
    pub fn assigned_count(&self) -> usize {
        self.assigned.len()
    }

    /// `true` if no usable host address remains.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.assigned.len() as u128 >= self.usable_capacity()
    }

    /// `true` if `ip` is the network address or the gateway.
    #[must_use]
    pub fn is_reserved(&self, ip: IpAddr) -> bool {
        ip == self.subnet.network() || ip == self.gateway
    }

    fn index_of(&self, ip: IpAddr) -> Option<u128> {
        if !self.subnet.contains(ip) {
            return None;
        }
        match (self.subnet.network(), ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                Some(u128::from(u32::from(addr).wrapping_sub(u32::from(net))))
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                Some(u128::from(addr).wrapping_sub(u128::from(net)))
            }
            _ => None,
        }
    }

    /// Allocate the lowest free pod IP.
    ///
    /// # Errors
    /// [`IpamError::SubnetFull`] when every usable host address is assigned.
    pub fn allocate(&mut self) -> Result<IpAddr, IpamError> {
        let idx = (self.first_host..self.host_end)
            .find(|i| !self.assigned.contains(i))
            .ok_or(IpamError::SubnetFull(self.subnet))?;
        self.assigned.insert(idx);
        self.subnet
            .nth_address(idx)
            .map_err(|_| IpamError::SubnetFull(self.subnet))
    }

    /// Assign a *specific* pod IP (e.g. honouring a requested address).
    ///
    /// # Errors
    /// - [`IpamError::OutOfRange`] if `ip` is not in the subnet.
    /// - [`IpamError::Reserved`] if `ip` is the network or gateway address.
    /// - [`IpamError::AlreadyAssigned`] if `ip` is already in use.
    pub fn assign(&mut self, ip: IpAddr) -> Result<(), IpamError> {
        let idx = self.index_of(ip).ok_or(IpamError::OutOfRange(ip))?;
        if idx < self.first_host {
            return Err(IpamError::Reserved(ip));
        }
        if self.assigned.contains(&idx) {
            return Err(IpamError::AlreadyAssigned(ip));
        }
        self.assigned.insert(idx);
        Ok(())
    }

    /// Free a previously assigned pod IP.
    ///
    /// # Errors
    /// - [`IpamError::OutOfRange`] if `ip` is not in the subnet.
    /// - [`IpamError::NotAssigned`] if `ip` was not assigned.
    pub fn free(&mut self, ip: IpAddr) -> Result<(), IpamError> {
        let idx = self.index_of(ip).ok_or(IpamError::OutOfRange(ip))?;
        if self.assigned.remove(&idx) {
            Ok(())
        } else {
            Err(IpamError::NotAssigned(ip))
        }
    }

    /// `true` if `ip` is currently assigned to a pod.
    #[must_use]
    pub fn is_assigned(&self, ip: IpAddr) -> bool {
        self.index_of(ip)
            .is_some_and(|idx| self.assigned.contains(&idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn ipam24() -> PodIpam {
        PodIpam::new(Cidr::from_str("10.42.5.0/24").expect("c")).expect("ipam")
    }

    #[test]
    fn reserves_network_and_gateway() {
        let ip = ipam24();
        assert_eq!(ip.gateway(), v4("10.42.5.1"));
        assert!(ip.is_reserved(v4("10.42.5.0")));
        assert!(ip.is_reserved(v4("10.42.5.1")));
        assert!(!ip.is_reserved(v4("10.42.5.2")));
    }

    #[test]
    fn usable_capacity_excludes_reserved() {
        // /24 = 256 addresses; minus network + gateway = 254 usable.
        assert_eq!(ipam24().usable_capacity(), 254);
    }

    #[test]
    fn allocate_starts_at_first_host() {
        let mut ip = ipam24();
        assert_eq!(ip.allocate().expect("a"), v4("10.42.5.2"));
        assert_eq!(ip.allocate().expect("b"), v4("10.42.5.3"));
        assert_eq!(ip.assigned_count(), 2);
    }

    #[test]
    fn allocate_skips_reserved_gateway() {
        let mut ip = ipam24();
        let first = ip.allocate().expect("a");
        assert_ne!(first, ip.gateway());
        assert_ne!(first, ip.subnet().network());
    }

    #[test]
    fn free_returns_address_to_pool() {
        let mut ip = ipam24();
        let a = ip.allocate().expect("a");
        ip.allocate().expect("b");
        ip.free(a).expect("free");
        assert_eq!(ip.assigned_count(), 1);
        // The freed lowest address is handed back out next.
        assert_eq!(ip.allocate().expect("c"), a);
    }

    #[test]
    fn free_unassigned_errors() {
        let mut ip = ipam24();
        assert_eq!(
            ip.free(v4("10.42.5.50")),
            Err(IpamError::NotAssigned(v4("10.42.5.50")))
        );
    }

    #[test]
    fn assign_specific_address() {
        let mut ip = ipam24();
        ip.assign(v4("10.42.5.100")).expect("assign");
        assert!(ip.is_assigned(v4("10.42.5.100")));
    }

    #[test]
    fn assign_reserved_rejected() {
        let mut ip = ipam24();
        assert_eq!(ip.assign(v4("10.42.5.0")), Err(IpamError::Reserved(v4("10.42.5.0"))));
        assert_eq!(ip.assign(v4("10.42.5.1")), Err(IpamError::Reserved(v4("10.42.5.1"))));
    }

    #[test]
    fn assign_out_of_range_rejected() {
        let mut ip = ipam24();
        assert_eq!(
            ip.assign(v4("10.42.6.5")),
            Err(IpamError::OutOfRange(v4("10.42.6.5")))
        );
    }

    #[test]
    fn assign_duplicate_rejected() {
        let mut ip = ipam24();
        ip.assign(v4("10.42.5.7")).expect("first");
        assert_eq!(
            ip.assign(v4("10.42.5.7")),
            Err(IpamError::AlreadyAssigned(v4("10.42.5.7")))
        );
    }

    #[test]
    fn detects_full_subnet() {
        // /29 = 8 addresses, minus network + gateway = 6 usable.
        let mut ip = PodIpam::new(Cidr::from_str("10.42.5.0/29").expect("c")).expect("ipam");
        assert_eq!(ip.usable_capacity(), 6);
        for _ in 0..6 {
            ip.allocate().expect("alloc");
        }
        assert!(ip.is_full());
        assert_eq!(ip.allocate(), Err(IpamError::SubnetFull(ip.subnet())));
    }

    #[test]
    fn full_clears_after_free() {
        let mut ip = PodIpam::new(Cidr::from_str("10.42.5.0/30").expect("c")).expect("ipam");
        // /30 = 4 addresses, minus network + gateway = 2 usable.
        assert_eq!(ip.usable_capacity(), 2);
        let a = ip.allocate().expect("a");
        ip.allocate().expect("b");
        assert!(ip.is_full());
        ip.free(a).expect("free");
        assert!(!ip.is_full());
        assert_eq!(ip.allocate().expect("c"), a);
    }

    #[test]
    fn subnet_too_small_for_hosts() {
        // /31 = 2 addresses: network + gateway, no room for a pod.
        assert!(matches!(
            PodIpam::new(Cidr::from_str("10.42.5.0/31").expect("c")),
            Err(IpamError::NoUsableHosts(_))
        ));
    }

    #[test]
    fn top_of_block_is_usable() {
        // flannel host-local treats the last address (.255 in a /24) as a
        // usable host; only .0 and the gateway are reserved.
        let mut ip = PodIpam::new(Cidr::from_str("10.42.5.248/29").expect("c")).expect("ipam");
        let mut last = ip.subnet().network();
        for _ in 0..ip.usable_capacity() {
            last = ip.allocate().expect("alloc");
        }
        assert_eq!(last, v4("10.42.5.255"));
    }

    #[test]
    fn works_for_v6_subnet() {
        let mut ip = PodIpam::new(Cidr::from_str("fd00::/120").expect("c")).expect("ipam");
        // /120 = 256 addresses, minus network + gateway = 254 usable.
        assert_eq!(ip.usable_capacity(), 254);
        let first = ip.allocate().expect("a");
        assert_eq!(first, IpAddr::from_str("fd00::2").expect("v6"));
    }
}
