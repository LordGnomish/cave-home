//! Forward-zone / stub-zone routing.
//!
//! First-party from Unbound's *public* `forward-zone` / `stub-zone`
//! documentation: a zone name is mapped to one or more upstream resolvers, and
//! a query is routed to the most specific (longest matching suffix) configured
//! zone. The root forward-zone (`.`) is the catch-all default route.
//!
//! This is a routing-*decision* model: it picks the upstream(s); the actual
//! query I/O (UDP/TCP/DoT/DoH) is Phase 1b (see the parity manifest).

use crate::name::DnsName;
use std::net::IpAddr;

/// Whether a zone is forwarded (recursion handed wholesale to the upstream) or
/// stubbed (the upstream is treated as authoritative for the zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    /// `forward-zone`: send recursive queries for the zone to the upstream.
    Forward,
    /// `stub-zone`: the upstream is authoritative for the zone.
    Stub,
}

/// One configured forward/stub zone: a name and its upstream resolvers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardZone {
    zone: DnsName,
    kind: RouteKind,
    upstreams: Vec<IpAddr>,
}

impl ForwardZone {
    /// Build a forward/stub zone. `upstreams` should be non-empty in practice;
    /// an empty list yields a route that matches but offers no target.
    #[must_use]
    pub const fn new(zone: DnsName, kind: RouteKind, upstreams: Vec<IpAddr>) -> Self {
        Self {
            zone,
            kind,
            upstreams,
        }
    }

    /// The zone name.
    #[must_use]
    pub const fn zone(&self) -> &DnsName {
        &self.zone
    }

    /// The route kind.
    #[must_use]
    pub const fn kind(&self) -> RouteKind {
        self.kind
    }

    /// The configured upstream resolvers.
    #[must_use]
    pub fn upstreams(&self) -> &[IpAddr] {
        &self.upstreams
    }
}

/// A routing table: choose the most specific forward/stub zone for a query.
#[derive(Debug, Clone, Default)]
pub struct ForwardTable {
    zones: Vec<ForwardZone>,
}

impl ForwardTable {
    /// An empty table (no routes — everything would recurse locally).
    #[must_use]
    pub const fn new() -> Self {
        Self { zones: Vec::new() }
    }

    /// Add a forward/stub zone.
    pub fn add(&mut self, zone: ForwardZone) {
        self.zones.push(zone);
    }

    /// Route a query name to its most specific matching zone (longest suffix).
    ///
    /// A zone matches when the query is the zone name or a subdomain of it.
    /// Among all matches the one with the most labels wins; the root zone
    /// (`.`, zero labels) is the default catch-all. Returns `None` when no
    /// zone — not even a root default — matches.
    #[must_use]
    pub fn route(&self, query: &DnsName) -> Option<&ForwardZone> {
        self.zones
            .iter()
            .filter(|z| query.is_within(z.zone()))
            .max_by_key(|z| z.zone().label_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::v4;

    fn name(n: &str) -> DnsName {
        DnsName::parse(n).expect("name")
    }

    fn fz(zone: &str, ip: [u8; 4]) -> ForwardZone {
        ForwardZone::new(
            name(zone),
            RouteKind::Forward,
            vec![IpAddr::V4(v4(ip[0], ip[1], ip[2], ip[3]))],
        )
    }

    #[test]
    fn root_zone_is_the_default_route() {
        let mut t = ForwardTable::new();
        t.add(fz(".", [9, 9, 9, 9]));
        let r = t.route(&name("example.com")).expect("default route");
        assert_eq!(r.upstreams()[0], IpAddr::V4(v4(9, 9, 9, 9)));
    }

    #[test]
    fn longest_suffix_wins_over_default() {
        let mut t = ForwardTable::new();
        t.add(fz(".", [9, 9, 9, 9]));
        t.add(fz("example.com", [1, 1, 1, 1]));
        t.add(fz("corp.example.com", [10, 0, 0, 1]));

        // Most specific match.
        assert_eq!(
            t.route(&name("host.corp.example.com")).expect("r").upstreams()[0],
            IpAddr::V4(v4(10, 0, 0, 1))
        );
        // Falls to the next-most-specific.
        assert_eq!(
            t.route(&name("www.example.com")).expect("r").upstreams()[0],
            IpAddr::V4(v4(1, 1, 1, 1))
        );
        // Falls to the default.
        assert_eq!(
            t.route(&name("wikipedia.org")).expect("r").upstreams()[0],
            IpAddr::V4(v4(9, 9, 9, 9))
        );
    }

    #[test]
    fn no_route_without_a_default() {
        let mut t = ForwardTable::new();
        t.add(fz("example.com", [1, 1, 1, 1]));
        assert!(t.route(&name("wikipedia.org")).is_none());
        assert!(t.route(&name("example.com")).is_some());
    }

    #[test]
    fn stub_and_forward_kinds_are_carried_through() {
        let mut t = ForwardTable::new();
        t.add(ForwardZone::new(
            name("home.arpa"),
            RouteKind::Stub,
            vec![IpAddr::V4(v4(192, 168, 1, 1))],
        ));
        let r = t.route(&name("nas.home.arpa")).expect("r");
        assert_eq!(r.kind(), RouteKind::Stub);
    }

    #[test]
    fn suffix_match_is_label_aligned_not_string() {
        let mut t = ForwardTable::new();
        t.add(fz("example.com", [1, 1, 1, 1]));
        // notexample.com must NOT match example.com.
        assert!(t.route(&name("notexample.com")).is_none());
    }
}
