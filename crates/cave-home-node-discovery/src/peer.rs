//! The discovered-peer model and grandma-friendly localisation.
//!
//! A [`DiscoveredPeer`] is cave-home's distilled view of another hub on the
//! LAN, built from its advertised [`crate::record::ServiceRecord`]: who it is
//! (node id), where it is (hostname + [`std::net::IpAddr`] list + port), what
//! it does ([`NodeRole`]), what version it runs, and the freshness bookkeeping
//! ([`DiscoveredPeer::last_seen`] + TTL) the cache uses to expire it.

use crate::compat::Version;
use std::net::IpAddr;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// The role a node plays in the cluster (ADR-005 topology: primary hub +
/// optional failover + optional ML / GPU node).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// The primary hub — the one the household interacts with.
    Primary,
    /// A failover / backup hub.
    Secondary,
    /// An ML / GPU off-load node.
    MlNode,
}

impl NodeRole {
    /// The wire token used in the `role` TXT key (stable, machine-facing — not
    /// shown to the end-user).
    #[must_use]
    pub const fn wire_token(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::MlNode => "ml-node",
        }
    }

    /// Parse a `role` TXT value back into a [`NodeRole`].
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "primary" => Some(Self::Primary),
            "secondary" => Some(Self::Secondary),
            "ml-node" => Some(Self::MlNode),
            _ => None,
        }
    }

    /// Grandma-friendly name for this role (Charter §6.3 — no cluster jargon).
    #[must_use]
    pub const fn friendly_name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Primary, Lang::En) => "Main hub",
            (Self::Primary, Lang::De) => "Haupt-Hub",
            (Self::Primary, Lang::Tr) => "Ana merkez",
            (Self::Secondary, Lang::En) => "Backup hub",
            (Self::Secondary, Lang::De) => "Reserve-Hub",
            (Self::Secondary, Lang::Tr) => "Yedek merkez",
            (Self::MlNode, Lang::En) => "Helper hub",
            (Self::MlNode, Lang::De) => "Helfer-Hub",
            (Self::MlNode, Lang::Tr) => "Yardımcı merkez",
        }
    }
}

/// Why a [`DiscoveredPeer`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerError {
    /// The node id was empty.
    EmptyNodeId,
    /// The hostname was empty.
    EmptyHostname,
    /// No addresses were supplied — a peer with nowhere to reach it is useless.
    NoAddresses,
    /// The advertised port was zero.
    ZeroPort,
    /// The TTL was zero (a record that expires immediately is invalid).
    ZeroTtl,
}

impl core::fmt::Display for PeerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyNodeId => f.write_str("peer node id is empty"),
            Self::EmptyHostname => f.write_str("peer hostname is empty"),
            Self::NoAddresses => f.write_str("peer has no addresses"),
            Self::ZeroPort => f.write_str("peer port is zero"),
            Self::ZeroTtl => f.write_str("peer TTL is zero"),
        }
    }
}

impl std::error::Error for PeerError {}

/// Another cave-home hub as seen on the LAN.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPeer {
    node_id: String,
    hostname: String,
    addresses: Vec<IpAddr>,
    port: u16,
    role: NodeRole,
    version: Version,
    /// The caller-supplied clock tick at which this peer was last observed.
    last_seen: u64,
    /// Record time-to-live, in the same tick units as `last_seen`.
    ttl: u64,
}

impl DiscoveredPeer {
    /// Build a peer, validating that it is actually reachable and identifiable.
    ///
    /// Addresses are de-duplicated (a hub commonly advertises the same IP in
    /// more than one A/AAAA record) while preserving first-seen order.
    ///
    /// # Errors
    /// [`PeerError`] when the node id / hostname is empty, no addresses are
    /// given, the port is zero, or the TTL is zero.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: impl Into<String>,
        hostname: impl Into<String>,
        addresses: Vec<IpAddr>,
        port: u16,
        role: NodeRole,
        version: Version,
        last_seen: u64,
        ttl: u64,
    ) -> Result<Self, PeerError> {
        let node_id = node_id.into();
        let hostname = hostname.into();
        if node_id.is_empty() {
            return Err(PeerError::EmptyNodeId);
        }
        if hostname.is_empty() {
            return Err(PeerError::EmptyHostname);
        }
        if addresses.is_empty() {
            return Err(PeerError::NoAddresses);
        }
        if port == 0 {
            return Err(PeerError::ZeroPort);
        }
        if ttl == 0 {
            return Err(PeerError::ZeroTtl);
        }
        let mut deduped: Vec<IpAddr> = Vec::with_capacity(addresses.len());
        for a in addresses {
            if !deduped.contains(&a) {
                deduped.push(a);
            }
        }
        Ok(Self {
            node_id,
            hostname,
            addresses: deduped,
            port,
            role,
            version,
            last_seen,
            ttl,
        })
    }

    #[must_use]
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    #[must_use]
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    #[must_use]
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    #[must_use]
    pub const fn role(&self) -> NodeRole {
        self.role
    }

    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    #[must_use]
    pub const fn last_seen(&self) -> u64 {
        self.last_seen
    }

    #[must_use]
    pub const fn ttl(&self) -> u64 {
        self.ttl
    }

    /// Whether this peer has expired given the caller's current `now` tick.
    ///
    /// A peer is alive up to and including `last_seen + ttl`; strictly after
    /// that it is expired. The clock is supplied by the caller so the model
    /// stays pure (no wall-clock reads).
    #[must_use]
    pub const fn is_expired(&self, now: u64) -> bool {
        now > self.last_seen.saturating_add(self.ttl)
    }

    /// Produce a copy refreshed to `now` (resets the freshness window). Used
    /// by the registry when it re-observes an unchanged peer.
    #[must_use]
    pub fn refreshed_at(&self, now: u64) -> Self {
        Self { last_seen: now, ..self.clone() }
    }

    /// The grandma-friendly "we found a hub" message for a notification
    /// (Charter §6.3 — no protocol terms).
    #[must_use]
    pub const fn found_message(&self, lang: Lang) -> &'static str {
        match (self.role, lang) {
            // A backup hub coming online gets its own reassuring phrasing.
            (NodeRole::Secondary, Lang::En) => "Backup hub connected.",
            (NodeRole::Secondary, Lang::De) => "Reserve-Hub verbunden.",
            (NodeRole::Secondary, Lang::Tr) => "Yedek merkez bağlandı.",
            (_, Lang::En) => "Found another hub.",
            (_, Lang::De) => "Ein weiterer Hub gefunden.",
            (_, Lang::Tr) => "Başka bir merkez bulundu.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn ver() -> Version {
        Version { major: 1, minor: 4, patch: 0 }
    }

    fn peer() -> DiscoveredPeer {
        DiscoveredPeer::new(
            "hub-kitchen",
            "kitchen.local",
            vec![ip(192, 168, 1, 10)],
            8123,
            NodeRole::Primary,
            ver(),
            100,
            120,
        )
        .expect("valid peer")
    }

    #[test]
    fn builds_valid_peer() {
        let p = peer();
        assert_eq!(p.node_id(), "hub-kitchen");
        assert_eq!(p.port(), 8123);
        assert_eq!(p.role(), NodeRole::Primary);
    }

    #[test]
    fn rejects_empty_node_id() {
        let e = DiscoveredPeer::new(
            "",
            "h.local",
            vec![ip(10, 0, 0, 1)],
            8123,
            NodeRole::Primary,
            ver(),
            0,
            60,
        );
        assert_eq!(e, Err(PeerError::EmptyNodeId));
    }

    #[test]
    fn rejects_no_addresses() {
        let e = DiscoveredPeer::new(
            "n", "h", vec![], 8123, NodeRole::Primary, ver(), 0, 60,
        );
        assert_eq!(e, Err(PeerError::NoAddresses));
    }

    #[test]
    fn rejects_zero_port_and_ttl() {
        assert_eq!(
            DiscoveredPeer::new("n", "h", vec![ip(1, 1, 1, 1)], 0, NodeRole::Primary, ver(), 0, 60),
            Err(PeerError::ZeroPort)
        );
        assert_eq!(
            DiscoveredPeer::new("n", "h", vec![ip(1, 1, 1, 1)], 1, NodeRole::Primary, ver(), 0, 0),
            Err(PeerError::ZeroTtl)
        );
    }

    #[test]
    fn dedupes_addresses_preserving_order() {
        let p = DiscoveredPeer::new(
            "n",
            "h",
            vec![
                ip(192, 168, 1, 10),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
                ip(192, 168, 1, 10),
            ],
            8123,
            NodeRole::Primary,
            ver(),
            0,
            60,
        )
        .expect("valid");
        assert_eq!(
            p.addresses(),
            &[ip(192, 168, 1, 10), IpAddr::V6(Ipv6Addr::LOCALHOST)]
        );
    }

    #[test]
    fn expiry_uses_supplied_clock() {
        let p = peer(); // last_seen=100, ttl=120 -> alive through tick 220.
        assert!(!p.is_expired(220));
        assert!(p.is_expired(221));
        assert!(!p.is_expired(150));
    }

    #[test]
    fn refresh_resets_window() {
        let p = peer();
        let r = p.refreshed_at(500);
        assert_eq!(r.last_seen(), 500);
        assert!(!r.is_expired(600));
    }

    #[test]
    fn role_wire_round_trip() {
        for role in [NodeRole::Primary, NodeRole::Secondary, NodeRole::MlNode] {
            assert_eq!(NodeRole::from_wire(role.wire_token()), Some(role));
        }
        assert_eq!(NodeRole::from_wire("nonsense"), None);
    }

    #[test]
    fn found_message_localised_and_role_aware() {
        let primary = peer();
        assert_eq!(primary.found_message(Lang::En), "Found another hub.");
        let backup = DiscoveredPeer::new(
            "n", "h", vec![ip(1, 1, 1, 1)], 8123, NodeRole::Secondary, ver(), 0, 60,
        )
        .expect("valid");
        assert_eq!(backup.found_message(Lang::De), "Reserve-Hub verbunden.");
        assert_eq!(backup.found_message(Lang::Tr), "Yedek merkez bağlandı.");
    }

    #[test]
    fn every_role_has_three_language_names_and_message() {
        for role in [NodeRole::Primary, NodeRole::Secondary, NodeRole::MlNode] {
            let p = DiscoveredPeer::new(
                "n", "h", vec![ip(1, 1, 1, 1)], 8123, role, ver(), 0, 60,
            )
            .expect("valid");
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!role.friendly_name(lang).is_empty());
                assert!(!p.found_message(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol / cluster terms.
        const BANNED: &[&str] = &[
            "mDNS", "SRV", "PTR", "TXT", "DNS-SD", "_cavehome", "_tcp",
            "224.0.0.251", "5353", "multicast", "node", "TTL", "Bonjour",
            "primary", "secondary", "ml-node", "K3s", "cluster",
        ];
        for role in [NodeRole::Primary, NodeRole::Secondary, NodeRole::MlNode] {
            let p = DiscoveredPeer::new(
                "n", "h", vec![ip(1, 1, 1, 1)], 8123, role, ver(), 0, 60,
            )
            .expect("valid");
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = format!("{} {}", role.friendly_name(lang), p.found_message(lang));
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "role {role:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
