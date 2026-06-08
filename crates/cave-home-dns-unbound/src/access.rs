//! Access-control model — who may query the resolver.
//!
//! First-party from Unbound's *public* `access-control` documentation: a list
//! of `<CIDR> <action>` rules where the action for a client is taken from the
//! most specific (longest-prefix) matching network. The actions cave-home
//! supports: `allow`, `refuse`, and `allow_snoop` (allow, plus permit cache
//! snooping).
//!
//! CIDR containment is implemented here against [`std::net::IpAddr`] by masking
//! and comparing the address bits — no external IP-network crate.

use crate::record::RecordError;
use std::net::IpAddr;
use std::str::FromStr;

/// What the resolver does for a matching client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessAction {
    /// Answer queries from this client normally.
    Allow,
    /// Refuse queries from this client (REFUSED).
    Refuse,
    /// Allow, and additionally permit cache-snooping queries.
    AllowSnoop,
}

impl Default for AccessAction {
    /// The safe default for an unconfigured client is to refuse.
    fn default() -> Self {
        Self::Refuse
    }
}

/// A CIDR network: a base address plus a prefix length (bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    base: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Build a CIDR, validating the prefix length against the address family
    /// (≤ 32 for IPv4, ≤ 128 for IPv6).
    ///
    /// # Errors
    /// [`RecordError::BadAddress`] when the prefix is too long for the family.
    pub const fn new(base: IpAddr, prefix: u8) -> Result<Self, RecordError> {
        let max = match base {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max {
            return Err(RecordError::BadAddress);
        }
        Ok(Self { base, prefix })
    }

    /// Parse a `"addr/prefix"` string (e.g. `"192.168.1.0/24"`,
    /// `"2001:db8::/32"`). A bare address (no `/`) is treated as a host route
    /// (`/32` or `/128`).
    ///
    /// # Errors
    /// [`RecordError::BadAddress`] on a malformed address or prefix.
    pub fn parse(text: &str) -> Result<Self, RecordError> {
        let text = text.trim();
        if let Some((addr_s, prefix_s)) = text.split_once('/') {
            let base = IpAddr::from_str(addr_s.trim()).map_err(|_| RecordError::BadAddress)?;
            let prefix: u8 = prefix_s
                .trim()
                .parse()
                .map_err(|_| RecordError::BadAddress)?;
            Self::new(base, prefix)
        } else {
            let base = IpAddr::from_str(text).map_err(|_| RecordError::BadAddress)?;
            let prefix = match base {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            Self::new(base, prefix)
        }
    }

    /// The prefix length in bits.
    #[must_use]
    pub const fn prefix(&self) -> u8 {
        self.prefix
    }

    /// Does this network contain `addr`?
    ///
    /// Both must be the same address family; a v4 network never contains a v6
    /// address (and vice-versa). Containment is a masked bit-compare over the
    /// first `prefix` bits.
    #[must_use]
    pub fn contains(&self, addr: IpAddr) -> bool {
        match (self.base, addr) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

/// Compare the first `prefix` bits of two equal-length octet arrays.
fn prefix_match(net: &[u8], ip: &[u8], prefix: u8) -> bool {
    let full = (prefix / 8) as usize;
    // Whole leading octets must match exactly.
    if net[..full] != ip[..full] {
        return false;
    }
    // The partial trailing octet (if any) is compared under its high-bit mask.
    let rem = prefix % 8;
    if rem == 0 {
        return true;
    }
    let mask = 0xFFu8 << (8 - rem);
    (net[full] & mask) == (ip[full] & mask)
}

/// One access-control rule: a network and the action for clients within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessRule {
    network: Cidr,
    action: AccessAction,
}

impl AccessRule {
    /// Build a rule.
    #[must_use]
    pub const fn new(network: Cidr, action: AccessAction) -> Self {
        Self { network, action }
    }

    /// The rule's network.
    #[must_use]
    pub const fn network(&self) -> Cidr {
        self.network
    }

    /// The rule's action.
    #[must_use]
    pub const fn action(&self) -> AccessAction {
        self.action
    }
}

/// The access-control list: decide the action for a client by longest-prefix
/// match.
#[derive(Debug, Clone, Default)]
pub struct AccessControl {
    rules: Vec<AccessRule>,
    default: AccessAction,
}

impl AccessControl {
    /// A new ACL with the given default action for clients matching no rule.
    /// Unbound's documented default is to refuse unconfigured clients, so a
    /// safe choice here is [`AccessAction::Refuse`].
    #[must_use]
    pub const fn new(default: AccessAction) -> Self {
        Self {
            rules: Vec::new(),
            default,
        }
    }

    /// Add a rule.
    pub fn add(&mut self, rule: AccessRule) {
        self.rules.push(rule);
    }

    /// Decide the action for a client address: the action of the most specific
    /// (longest-prefix) matching rule, or the default when none match.
    #[must_use]
    pub fn decide(&self, client: IpAddr) -> AccessAction {
        self.rules
            .iter()
            .filter(|r| r.network().contains(client))
            .max_by_key(|r| r.network().prefix())
            .map_or(self.default, AccessRule::action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{v4, v6};

    fn ip4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(v4(a, b, c, d))
    }

    #[test]
    fn cidr_contains_v4_on_octet_and_partial_boundaries() {
        let net = Cidr::parse("192.168.1.0/24").expect("cidr");
        assert!(net.contains(ip4(192, 168, 1, 0)));
        assert!(net.contains(ip4(192, 168, 1, 255)));
        assert!(!net.contains(ip4(192, 168, 2, 1)));

        // Partial trailing octet: /26 covers .0-.63 only.
        let small = Cidr::parse("10.0.0.0/26").expect("cidr");
        assert!(small.contains(ip4(10, 0, 0, 63)));
        assert!(!small.contains(ip4(10, 0, 0, 64)));
    }

    #[test]
    fn cidr_zero_prefix_matches_everything_in_family() {
        let all = Cidr::parse("0.0.0.0/0").expect("cidr");
        assert!(all.contains(ip4(8, 8, 8, 8)));
        assert!(all.contains(ip4(192, 168, 0, 1)));
        // ...but not a v6 client.
        assert!(!all.contains(IpAddr::V6(v6(0, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn cidr_contains_v6() {
        let net = Cidr::parse("2001:db8::/32").expect("cidr");
        assert!(net.contains(IpAddr::V6(v6(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1))));
        assert!(net.contains(IpAddr::V6(v6(0x2001, 0x0db8, 0xffff, 0, 0, 0, 0, 0))));
        assert!(!net.contains(IpAddr::V6(v6(0x2001, 0x0db9, 0, 0, 0, 0, 0, 1))));
        // Family mismatch.
        assert!(!net.contains(ip4(192, 168, 1, 1)));
    }

    #[test]
    fn host_route_without_slash_is_full_prefix() {
        let host = Cidr::parse("10.0.0.5").expect("cidr");
        assert_eq!(host.prefix(), 32);
        assert!(host.contains(ip4(10, 0, 0, 5)));
        assert!(!host.contains(ip4(10, 0, 0, 6)));
    }

    #[test]
    fn rejects_oversized_prefix() {
        assert_eq!(Cidr::parse("10.0.0.0/33"), Err(RecordError::BadAddress));
        assert_eq!(Cidr::parse("2001:db8::/129"), Err(RecordError::BadAddress));
        assert_eq!(Cidr::parse("garbage/24"), Err(RecordError::BadAddress));
    }

    #[test]
    fn access_control_longest_prefix_wins() {
        let mut acl = AccessControl::new(AccessAction::Refuse);
        // Broad allow for the LAN…
        acl.add(AccessRule::new(
            Cidr::parse("192.168.0.0/16").expect("c"),
            AccessAction::Allow,
        ));
        // …but a guest subnet within it is refused.
        acl.add(AccessRule::new(
            Cidr::parse("192.168.50.0/24").expect("c"),
            AccessAction::Refuse,
        ));
        // …and the admin host gets snoop.
        acl.add(AccessRule::new(
            Cidr::parse("192.168.1.10/32").expect("c"),
            AccessAction::AllowSnoop,
        ));

        assert_eq!(acl.decide(ip4(192, 168, 1, 5)), AccessAction::Allow);
        assert_eq!(acl.decide(ip4(192, 168, 50, 7)), AccessAction::Refuse);
        assert_eq!(acl.decide(ip4(192, 168, 1, 10)), AccessAction::AllowSnoop);
    }

    #[test]
    fn access_control_default_for_unmatched_client() {
        let acl = AccessControl::new(AccessAction::Refuse);
        assert_eq!(acl.decide(ip4(8, 8, 8, 8)), AccessAction::Refuse);
    }
}
