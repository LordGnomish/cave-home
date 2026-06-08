// SPDX-License-Identifier: Apache-2.0
//! CNI ADD / DEL decision and result model.
//!
//! When the container runtime invokes the CNI plugin, it issues an ADD (attach
//! a pod to the network) or a DEL (detach). The flannel CNI plugin resolves
//! the node subnet from `FLANNEL_SUBNET` env, then delegates IP assignment to
//! host-local IPAM and returns a CNI *result* describing the assigned address,
//! gateway, routes and DNS (the CNI spec result schema).
//!
//! This module models those decisions over the [`crate::ipam::PodIpam`]: ADD
//! allocates an address and builds the result; DEL frees it. The wire protocol
//! (reading stdin JSON, writing stdout JSON, the netlink veth plumbing) is the
//! deferred I/O layer; what lives here is the allocation decision and the
//! typed result the plugin would emit.

use std::fmt;
use std::net::IpAddr;

use crate::cidr::Cidr;
use crate::ipam::{IpamError, PodIpam};

/// A CNI route in a result: a destination prefix and an optional gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CniRoute {
    /// The destination CIDR (`0.0.0.0/0` for a default route).
    pub dest: Cidr,
    /// Optional explicit gateway; `None` means "via the interface".
    pub gateway: Option<IpAddr>,
}

/// The IP-config block of a CNI result: one assigned address + its gateway.
///
/// CNI's `ips[].address` is the pod's *host* address together with its subnet
/// prefix length (e.g. `10.42.5.2/24`). We keep the host [`IpAddr`] and the
/// prefix length separately because [`Cidr`] canonicalises to the network
/// address and would lose the host bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpConfig {
    /// The assigned pod host address.
    pub address: IpAddr,
    /// The subnet prefix length the address sits in.
    pub prefix_len: u8,
    /// The subnet gateway.
    pub gateway: IpAddr,
}

impl IpConfig {
    /// Render the address as CNI's `ip/prefix` string (e.g. `10.42.5.2/24`).
    #[must_use]
    pub fn address_cidr_string(&self) -> String {
        format!("{}/{}", self.address, self.prefix_len)
    }

    /// The network the address belongs to.
    ///
    /// # Errors
    /// Returns a [`crate::cidr::CidrError`] only if the prefix length is
    /// invalid for the address family (never for a result we built).
    pub fn network(&self) -> Result<Cidr, crate::cidr::CidrError> {
        Cidr::new(self.address, self.prefix_len)
    }
}

/// The result a CNI ADD returns (a subset of the CNI spec `Result` schema:
/// `ips`, `routes`, `dns`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CniResult {
    /// Assigned IP configuration.
    pub ip: IpConfig,
    /// Routes to install in the pod's namespace.
    pub routes: Vec<CniRoute>,
    /// DNS nameservers for the pod.
    pub dns: Vec<IpAddr>,
}

/// Errors from CNI ADD/DEL handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CniError {
    /// Underlying IPAM error (subnet full, out of range, etc.).
    Ipam(IpamError),
}

impl fmt::Display for CniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ipam(e) => write!(f, "IPAM error: {e}"),
        }
    }
}

impl std::error::Error for CniError {}

impl From<IpamError> for CniError {
    fn from(e: IpamError) -> Self {
        Self::Ipam(e)
    }
}

/// Handle a CNI ADD: allocate a pod IP from `ipam` and build the result.
///
/// The result carries the assigned address (as a host-prefixed CIDR, e.g.
/// `10.42.5.2/24` so the pod knows its subnet), the subnet gateway, a default
/// route via that gateway, and the supplied `dns` nameservers — mirroring what
/// the flannel CNI delegate returns.
///
/// # Errors
/// Propagates [`IpamError::SubnetFull`] if the node subnet is exhausted.
pub fn cni_add(ipam: &mut PodIpam, dns: &[IpAddr]) -> Result<CniResult, CniError> {
    let addr = ipam.allocate()?;
    Ok(build_result(ipam.subnet(), addr, ipam.gateway(), dns))
}

/// Handle a CNI ADD requesting a *specific* address.
///
/// # Errors
/// Propagates IPAM errors (reserved, out-of-range, already-assigned).
pub fn cni_add_specific(
    ipam: &mut PodIpam,
    requested: IpAddr,
    dns: &[IpAddr],
) -> Result<CniResult, CniError> {
    ipam.assign(requested)?;
    Ok(build_result(ipam.subnet(), requested, ipam.gateway(), dns))
}

/// Handle a CNI DEL: free the pod's address back to the pool.
///
/// Per the CNI spec, DEL must be idempotent: deleting an address that was
/// never assigned is *not* an error (the runtime may retry). We therefore
/// treat [`IpamError::NotAssigned`] as success.
///
/// # Errors
/// Propagates IPAM errors other than `NotAssigned` (e.g. out-of-range).
pub fn cni_del(ipam: &mut PodIpam, addr: IpAddr) -> Result<(), CniError> {
    match ipam.free(addr) {
        Ok(()) | Err(IpamError::NotAssigned(_)) => Ok(()),
        Err(e) => Err(CniError::Ipam(e)),
    }
}

fn build_result(subnet: Cidr, addr: IpAddr, gateway: IpAddr, dns: &[IpAddr]) -> CniResult {
    CniResult {
        ip: IpConfig {
            address: addr,
            prefix_len: subnet.prefix_len(),
            gateway,
        },
        routes: vec![CniRoute {
            dest: default_route(addr),
            gateway: Some(gateway),
        }],
        dns: dns.to_vec(),
    }
}

/// The default route (`0.0.0.0/0` or `::/0`) for `family_of`'s family. `/0` is
/// always valid, so this never errors; the fallback keeps the function total.
fn default_route(family_of: IpAddr) -> Cidr {
    let zero = match family_of {
        IpAddr::V4(_) => IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
    };
    Cidr::new(zero, 0).unwrap_or(Cidr::V4_DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn ipam() -> PodIpam {
        PodIpam::new(Cidr::from_str("10.42.5.0/24").expect("c")).expect("ipam")
    }

    #[test]
    fn add_allocates_and_builds_result() {
        let mut ip = ipam();
        let dns = [v4("8.8.8.8")];
        let res = cni_add(&mut ip, &dns).expect("add");
        assert_eq!(res.ip.address, v4("10.42.5.2"));
        assert_eq!(res.ip.prefix_len, 24);
        assert_eq!(res.ip.address_cidr_string(), "10.42.5.2/24");
        assert_eq!(res.ip.gateway, v4("10.42.5.1"));
        assert_eq!(res.dns, vec![v4("8.8.8.8")]);
    }

    #[test]
    fn add_result_carries_default_route_via_gateway() {
        let mut ip = ipam();
        let res = cni_add(&mut ip, &[]).expect("add");
        assert_eq!(res.routes.len(), 1);
        assert_eq!(res.routes[0].dest, Cidr::from_str("0.0.0.0/0").expect("c"));
        assert_eq!(res.routes[0].gateway, Some(v4("10.42.5.1")));
    }

    #[test]
    fn add_uses_subnet_gateway_as_first_usable() {
        let mut ip = ipam();
        let res = cni_add(&mut ip, &[]).expect("add");
        // Gateway is .1, first pod is .2.
        assert_eq!(res.ip.gateway, v4("10.42.5.1"));
        assert_eq!(res.ip.network().expect("net").network(), v4("10.42.5.0"));
    }

    #[test]
    fn sequential_adds_get_distinct_addresses() {
        let mut ip = ipam();
        let a = cni_add(&mut ip, &[]).expect("a");
        let b = cni_add(&mut ip, &[]).expect("b");
        assert_eq!(a.ip.address, v4("10.42.5.2"));
        assert_eq!(b.ip.address, v4("10.42.5.3"));
    }

    #[test]
    fn add_specific_assigns_requested() {
        let mut ip = ipam();
        let res = cni_add_specific(&mut ip, v4("10.42.5.77"), &[]).expect("add");
        assert_eq!(res.ip.address, v4("10.42.5.77"));
        assert_eq!(res.ip.address_cidr_string(), "10.42.5.77/24");
        assert!(ip.is_assigned(v4("10.42.5.77")));
    }

    #[test]
    fn add_specific_reserved_errors() {
        let mut ip = ipam();
        let err = cni_add_specific(&mut ip, v4("10.42.5.1"), &[]).unwrap_err();
        assert!(matches!(err, CniError::Ipam(IpamError::Reserved(_))));
    }

    #[test]
    fn del_frees_address() {
        let mut ip = ipam();
        let res = cni_add(&mut ip, &[]).expect("add");
        let addr = res.ip.address;
        assert!(ip.is_assigned(addr));
        cni_del(&mut ip, addr).expect("del");
        assert!(!ip.is_assigned(addr));
    }

    #[test]
    fn del_is_idempotent() {
        let mut ip = ipam();
        // Deleting a never-assigned address is not an error (CNI spec).
        cni_del(&mut ip, v4("10.42.5.99")).expect("idempotent del");
    }

    #[test]
    fn del_out_of_range_errors() {
        let mut ip = ipam();
        let err = cni_del(&mut ip, v4("10.99.0.5")).unwrap_err();
        assert!(matches!(err, CniError::Ipam(IpamError::OutOfRange(_))));
    }

    #[test]
    fn add_then_del_then_readd_reuses_address() {
        let mut ip = ipam();
        let first = cni_add(&mut ip, &[]).expect("add");
        cni_del(&mut ip, first.ip.address).expect("del");
        let again = cni_add(&mut ip, &[]).expect("re-add");
        assert_eq!(first.ip.address, again.ip.address);
    }

    #[test]
    fn add_propagates_subnet_full() {
        let mut ip = PodIpam::new(Cidr::from_str("10.42.5.0/30").expect("c")).expect("ipam");
        cni_add(&mut ip, &[]).expect("a");
        cni_add(&mut ip, &[]).expect("b");
        let err = cni_add(&mut ip, &[]).unwrap_err();
        assert!(matches!(err, CniError::Ipam(IpamError::SubnetFull(_))));
    }
}
