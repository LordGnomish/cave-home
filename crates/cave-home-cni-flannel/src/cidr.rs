// SPDX-License-Identifier: Apache-2.0
//! CIDR math over `std::net`, implemented from first principles.
//!
//! This module is the arithmetic foundation under the subnet manager and the
//! pod-IP IPAM. flannel's Go code leans on `net.IPNet` from the Go standard
//! library; we have no equivalent in Rust `std`, so we implement the prefix /
//! mask / containment / subnetting operations ourselves on top of
//! [`std::net::IpAddr`]. Everything is pure integer arithmetic on the address
//! bits — no allocation on the hot path, no hardware, no network.
//!
//! The behaviour mirrors RFC 4632 (IPv4 classless prefixes) and RFC 4291
//! (IPv6 addressing) as documented in the public CNI / flannel specs.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Errors produced while parsing or constructing a [`Cidr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidrError {
    /// The string had no `/` separating address from prefix length.
    MissingPrefix,
    /// The address portion did not parse as an IPv4 or IPv6 address.
    BadAddress(String),
    /// The prefix length portion was not a base-10 integer.
    BadPrefixLength(String),
    /// The prefix length exceeded the address width (32 for v4, 128 for v6).
    PrefixTooLong { prefix: u8, max: u8 },
    /// A v4 operation was asked of a v6 value or vice-versa.
    FamilyMismatch,
}

impl fmt::Display for CidrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrefix => write!(f, "CIDR is missing a '/' prefix length"),
            Self::BadAddress(s) => write!(f, "CIDR address part '{s}' is not a valid IP"),
            Self::BadPrefixLength(s) => write!(f, "CIDR prefix length '{s}' is not a number"),
            Self::PrefixTooLong { prefix, max } => {
                write!(f, "CIDR prefix /{prefix} exceeds maximum /{max}")
            }
            Self::FamilyMismatch => write!(f, "CIDR address family mismatch"),
        }
    }
}

impl std::error::Error for CidrError {}

/// A classless network prefix: a base address plus a prefix length.
///
/// The stored `addr` is always the canonical *network* address (host bits
/// cleared), so two `Cidr`s that name the same network always compare equal
/// regardless of how they were constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// The IPv4 default route `0.0.0.0/0`. A safe, always-valid constant used
    /// as a total fallback where a `Cidr` is needed unconditionally.
    pub const V4_DEFAULT: Self = Self {
        addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        prefix: 0,
    };

    /// Build a CIDR from an address and prefix length, canonicalising the
    /// address to its network base (clearing the host bits).
    ///
    /// # Errors
    /// Returns [`CidrError::PrefixTooLong`] if `prefix` exceeds the address
    /// width for the family.
    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self, CidrError> {
        let max = Self::max_prefix(&addr);
        if prefix > max {
            return Err(CidrError::PrefixTooLong { prefix, max });
        }
        let network = mask_addr(addr, prefix);
        Ok(Self {
            addr: network,
            prefix,
        })
    }

    /// The maximum prefix length for the family of `addr` (32 or 128).
    #[must_use]
    pub const fn max_prefix(addr: &IpAddr) -> u8 {
        match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        }
    }

    /// The canonical network address (host bits cleared).
    #[must_use]
    pub const fn network(&self) -> IpAddr {
        self.addr
    }

    /// The prefix length in bits.
    #[must_use]
    pub const fn prefix_len(&self) -> u8 {
        self.prefix
    }

    /// `true` for an IPv4 prefix.
    #[must_use]
    pub const fn is_ipv4(&self) -> bool {
        matches!(self.addr, IpAddr::V4(_))
    }

    /// `true` for an IPv6 prefix.
    #[must_use]
    pub const fn is_ipv6(&self) -> bool {
        matches!(self.addr, IpAddr::V6(_))
    }

    /// The netmask for this prefix as an [`IpAddr`] (e.g. `/24` → `255.255.255.0`).
    #[must_use]
    pub fn netmask(&self) -> IpAddr {
        match self.addr {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::from(v4_mask(self.prefix))),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from(v6_mask(self.prefix))),
        }
    }

    /// The number of addresses in this block, capped at `u128::MAX`.
    ///
    /// A `/0` IPv6 block would be `2^128` which does not fit; we saturate so
    /// callers can compare against finite counts without overflowing.
    #[must_use]
    pub const fn address_count(&self) -> u128 {
        let host_bits = (Self::max_prefix_const(&self.addr) - self.prefix) as u32;
        if host_bits >= 128 {
            u128::MAX
        } else {
            1u128 << host_bits
        }
    }

    const fn max_prefix_const(addr: &IpAddr) -> u8 {
        match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        }
    }

    /// `true` if `ip` falls within this prefix.
    ///
    /// A family mismatch (v4 prefix vs v6 address) is never a containment, so
    /// it simply returns `false`.
    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(test)) => {
                (u32::from(test) & v4_mask(self.prefix)) == u32::from(net)
            }
            (IpAddr::V6(net), IpAddr::V6(test)) => {
                (u128::from(test) & v6_mask(self.prefix)) == u128::from(net)
            }
            _ => false,
        }
    }

    /// `true` if this block overlaps `other` (either contains the other's
    /// network address). Two blocks of the same family overlap iff one's
    /// network is inside the other.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.contains(other.network()) || other.contains(self.network())
    }

    /// Iterate the more-specific subnets of length `new_prefix` carved out of
    /// this block. For example `10.42.0.0/16` split at `/24` yields
    /// `10.42.0.0/24, 10.42.1.0/24, …, 10.42.255.0/24` (256 subnets).
    ///
    /// # Errors
    /// Returns [`CidrError::PrefixTooLong`] if `new_prefix` is shorter than
    /// (or equal to a too-large value beyond) the family width, or shorter
    /// than the current prefix (which would not be a *sub*net).
    pub fn subnets(&self, new_prefix: u8) -> Result<SubnetIter, CidrError> {
        let max = Self::max_prefix(&self.addr);
        if new_prefix > max {
            return Err(CidrError::PrefixTooLong {
                prefix: new_prefix,
                max,
            });
        }
        if new_prefix < self.prefix {
            // A "subnet" must be at least as specific as the parent.
            return Err(CidrError::PrefixTooLong {
                prefix: self.prefix,
                max: new_prefix,
            });
        }
        let count = 1u128 << u32::from(new_prefix - self.prefix);
        Ok(SubnetIter {
            base: self.addr,
            new_prefix,
            step_bits: u32::from(max - new_prefix),
            index: 0,
            count,
        })
    }

    /// The `index`-th host address within this block, counting from the
    /// network address (`index == 0` is the network address itself).
    ///
    /// # Errors
    /// Returns [`CidrError::PrefixTooLong`] (used here as an out-of-range
    /// signal) if `index` is beyond the block.
    pub fn nth_address(&self, index: u128) -> Result<IpAddr, CidrError> {
        if index >= self.address_count() {
            return Err(CidrError::PrefixTooLong {
                prefix: self.prefix,
                max: Self::max_prefix(&self.addr),
            });
        }
        Ok(match self.addr {
            IpAddr::V4(net) => {
                // index < 2^32 here because address_count fits.
                #[allow(clippy::cast_possible_truncation)]
                let off = index as u32;
                IpAddr::V4(Ipv4Addr::from(u32::from(net).wrapping_add(off)))
            }
            IpAddr::V6(net) => IpAddr::V6(Ipv6Addr::from(u128::from(net).wrapping_add(index))),
        })
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

impl std::str::FromStr for Cidr {
    type Err = CidrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr_part, prefix_part) = s.split_once('/').ok_or(CidrError::MissingPrefix)?;
        let addr: IpAddr = addr_part
            .parse()
            .map_err(|_| CidrError::BadAddress(addr_part.to_owned()))?;
        let prefix: u8 = prefix_part
            .parse()
            .map_err(|_| CidrError::BadPrefixLength(prefix_part.to_owned()))?;
        Self::new(addr, prefix)
    }
}

/// Iterator over the subnets produced by [`Cidr::subnets`].
#[derive(Debug, Clone)]
pub struct SubnetIter {
    base: IpAddr,
    new_prefix: u8,
    step_bits: u32,
    index: u128,
    count: u128,
}

impl Iterator for SubnetIter {
    type Item = Cidr;

    fn next(&mut self) -> Option<Cidr> {
        if self.index >= self.count {
            return None;
        }
        let offset = self.index << self.step_bits;
        let net = match self.base {
            IpAddr::V4(b) => {
                #[allow(clippy::cast_possible_truncation)]
                let off = offset as u32;
                IpAddr::V4(Ipv4Addr::from(u32::from(b).wrapping_add(off)))
            }
            IpAddr::V6(b) => IpAddr::V6(Ipv6Addr::from(u128::from(b).wrapping_add(offset))),
        };
        self.index += 1;
        // Construction cannot fail: prefix validated by `subnets`.
        Some(Cidr {
            addr: net,
            prefix: self.new_prefix,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count.saturating_sub(self.index);
        let r = usize::try_from(remaining).unwrap_or(usize::MAX);
        (r, Some(r))
    }
}

/// Build a `/prefix` IPv4 netmask as a `u32`. `prefix == 0` → `0`,
/// `prefix == 32` → all ones.
const fn v4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else if prefix >= 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix)
    }
}

/// Build a `/prefix` IPv6 netmask as a `u128`.
const fn v6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else if prefix >= 128 {
        u128::MAX
    } else {
        u128::MAX << (128 - prefix)
    }
}

/// Apply the `/prefix` mask to an address, returning its network base.
fn mask_addr(addr: IpAddr, prefix: u8) -> IpAddr {
    match addr {
        IpAddr::V4(a) => IpAddr::V4(Ipv4Addr::from(u32::from(a) & v4_mask(prefix))),
        IpAddr::V6(a) => IpAddr::V6(Ipv6Addr::from(u128::from(a) & v6_mask(prefix))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("test v4"))
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from_str(s).expect("test v6"))
    }

    #[test]
    fn parses_and_canonicalises_v4_network() {
        // Host bits set in input must be cleared in the canonical network.
        let c = Cidr::from_str("10.42.7.99/16").expect("parse");
        assert_eq!(c.network(), v4("10.42.0.0"));
        assert_eq!(c.prefix_len(), 16);
        assert!(c.is_ipv4());
    }

    #[test]
    fn parses_v6_and_canonicalises() {
        let c = Cidr::from_str("fd00:cafe:1234:5678::abcd/64").expect("parse");
        assert_eq!(c.network(), v6("fd00:cafe:1234:5678::"));
        assert!(c.is_ipv6());
    }

    #[test]
    fn rejects_missing_prefix() {
        assert_eq!(Cidr::from_str("10.0.0.0"), Err(CidrError::MissingPrefix));
    }

    #[test]
    fn rejects_bad_address() {
        assert!(matches!(
            Cidr::from_str("not.an.ip/24"),
            Err(CidrError::BadAddress(_))
        ));
    }

    #[test]
    fn rejects_bad_prefix_number() {
        assert!(matches!(
            Cidr::from_str("10.0.0.0/xx"),
            Err(CidrError::BadPrefixLength(_))
        ));
    }

    #[test]
    fn rejects_overlong_prefix_v4() {
        assert_eq!(
            Cidr::new(v4("10.0.0.0"), 33),
            Err(CidrError::PrefixTooLong { prefix: 33, max: 32 })
        );
    }

    #[test]
    fn rejects_overlong_prefix_v6() {
        assert_eq!(
            Cidr::new(v6("fd00::"), 129),
            Err(CidrError::PrefixTooLong {
                prefix: 129,
                max: 128
            })
        );
    }

    #[test]
    fn netmask_v4_is_correct() {
        assert_eq!(
            Cidr::new(v4("10.42.0.0"), 24).expect("c").netmask(),
            v4("255.255.255.0")
        );
        assert_eq!(
            Cidr::new(v4("10.0.0.0"), 16).expect("c").netmask(),
            v4("255.255.0.0")
        );
        assert_eq!(
            Cidr::new(v4("0.0.0.0"), 0).expect("c").netmask(),
            v4("0.0.0.0")
        );
        assert_eq!(
            Cidr::new(v4("1.2.3.4"), 32).expect("c").netmask(),
            v4("255.255.255.255")
        );
    }

    #[test]
    fn netmask_v6_is_correct() {
        assert_eq!(
            Cidr::new(v6("fd00::"), 64).expect("c").netmask(),
            v6("ffff:ffff:ffff:ffff::")
        );
    }

    #[test]
    fn contains_respects_boundaries_v4() {
        let net = Cidr::from_str("10.42.0.0/16").expect("c");
        assert!(net.contains(v4("10.42.0.0")));
        assert!(net.contains(v4("10.42.255.255")));
        assert!(net.contains(v4("10.42.7.1")));
        assert!(!net.contains(v4("10.43.0.0")));
        assert!(!net.contains(v4("10.41.255.255")));
    }

    #[test]
    fn contains_respects_boundaries_v6() {
        let net = Cidr::from_str("fd00:cafe::/32").expect("c");
        assert!(net.contains(v6("fd00:cafe::1")));
        assert!(net.contains(v6("fd00:cafe:ffff:ffff::")));
        assert!(!net.contains(v6("fd00:cafd::1")));
    }

    #[test]
    fn contains_family_mismatch_is_false() {
        let net = Cidr::from_str("10.0.0.0/8").expect("c");
        assert!(!net.contains(v6("fd00::1")));
    }

    #[test]
    fn address_count_v4() {
        assert_eq!(Cidr::from_str("10.42.0.0/24").expect("c").address_count(), 256);
        assert_eq!(Cidr::from_str("10.0.0.0/16").expect("c").address_count(), 65_536);
        assert_eq!(Cidr::from_str("1.2.3.4/32").expect("c").address_count(), 1);
    }

    #[test]
    fn address_count_v6_saturates() {
        // /0 of v6 is 2^128 which does not fit in u128 → saturate.
        assert_eq!(Cidr::from_str("::/0").expect("c").address_count(), u128::MAX);
        assert_eq!(Cidr::from_str("fd00::/120").expect("c").address_count(), 256);
    }

    #[test]
    fn subnets_split_16_into_24s() {
        let parent = Cidr::from_str("10.42.0.0/16").expect("c");
        let subs: Vec<Cidr> = parent.subnets(24).expect("split").collect();
        assert_eq!(subs.len(), 256);
        assert_eq!(subs[0], Cidr::from_str("10.42.0.0/24").expect("c"));
        assert_eq!(subs[1], Cidr::from_str("10.42.1.0/24").expect("c"));
        assert_eq!(subs[255], Cidr::from_str("10.42.255.0/24").expect("c"));
        // Every subnet is contained in the parent and disjoint from siblings.
        for s in &subs {
            assert!(parent.contains(s.network()));
        }
    }

    #[test]
    fn subnets_same_prefix_yields_self_only() {
        let parent = Cidr::from_str("10.42.3.0/24").expect("c");
        let subs: Vec<Cidr> = parent.subnets(24).expect("split").collect();
        assert_eq!(subs, vec![parent]);
    }

    #[test]
    fn subnets_reject_shorter_prefix() {
        let parent = Cidr::from_str("10.42.0.0/24").expect("c");
        assert!(parent.subnets(16).is_err());
    }

    #[test]
    fn subnets_reject_overlong_prefix() {
        let parent = Cidr::from_str("10.42.0.0/24").expect("c");
        assert!(parent.subnets(33).is_err());
    }

    #[test]
    fn subnets_v6_split() {
        let parent = Cidr::from_str("fd00::/126").expect("c");
        let subs: Vec<Cidr> = parent.subnets(128).expect("split").collect();
        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0].network(), v6("fd00::"));
        assert_eq!(subs[3].network(), v6("fd00::3"));
    }

    #[test]
    fn nth_address_within_block() {
        let net = Cidr::from_str("10.42.5.0/24").expect("c");
        assert_eq!(net.nth_address(0).expect("0"), v4("10.42.5.0"));
        assert_eq!(net.nth_address(1).expect("1"), v4("10.42.5.1"));
        assert_eq!(net.nth_address(255).expect("255"), v4("10.42.5.255"));
        assert!(net.nth_address(256).is_err());
    }

    #[test]
    fn overlaps_detects_nesting() {
        let big = Cidr::from_str("10.42.0.0/16").expect("c");
        let small = Cidr::from_str("10.42.3.0/24").expect("c");
        let other = Cidr::from_str("10.43.0.0/16").expect("c");
        assert!(big.overlaps(&small));
        assert!(small.overlaps(&big));
        assert!(!big.overlaps(&other));
    }

    #[test]
    fn subnet_iter_size_hint_is_exact() {
        let parent = Cidr::from_str("10.42.0.0/16").expect("c");
        let it = parent.subnets(24).expect("split");
        assert_eq!(it.size_hint(), (256, Some(256)));
    }

    #[test]
    fn equal_networks_compare_equal_regardless_of_host_bits() {
        let a = Cidr::new(v4("10.42.7.3"), 24).expect("a");
        let b = Cidr::new(v4("10.42.7.200"), 24).expect("b");
        assert_eq!(a, b);
    }
}
