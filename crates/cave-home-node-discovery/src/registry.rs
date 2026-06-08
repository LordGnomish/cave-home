//! The peer registry / cache.
//!
//! As advertisements arrive (from the deferred multicast listener, or from a
//! test), [`PeerRegistry::observe`] folds each [`crate::record::ServiceRecord`]
//! into a deduped-by-node-id cache of [`DiscoveredPeer`]s. It is **pure over a
//! caller-supplied clock**: every call takes a `now` tick, and expiry is driven
//! by [`PeerRegistry::expire`] rather than any wall-clock read, so the whole
//! cache is deterministically testable.
//!
//! `observe` reports what changed via [`ObserveEvent`] so the Portal can raise
//! the right grandma-friendly notification (new hub found, hub moved, …).

use crate::peer::{DiscoveredPeer, PeerError};
use crate::record::ServiceRecord;
use std::collections::BTreeMap;
use std::net::IpAddr;

/// What observing a record did to the registry.
#[derive(Debug, Clone, PartialEq)]
pub enum ObserveEvent {
    /// A node id not seen before was added.
    NewPeer,
    /// An existing peer was refreshed with no change to its addresses.
    Refreshed,
    /// An existing peer's address set changed (it moved / re-homed). Carries
    /// the previous addresses for the caller to log or react to.
    AddressChanged { previous: Vec<IpAddr> },
    /// The record could not be turned into a valid peer; carries the cause.
    /// The registry is left unchanged.
    Rejected(PeerError),
}

impl ObserveEvent {
    /// Convenience for tests / callers: was this a brand-new peer?
    #[must_use]
    pub const fn is_new(&self) -> bool {
        matches!(self, Self::NewPeer)
    }

    /// Did the observation succeed (any non-rejection outcome)?
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        !matches!(self, Self::Rejected(_))
    }
}

/// A clock-driven cache of discovered cave-home peers, keyed by node id.
#[derive(Debug, Default, Clone)]
pub struct PeerRegistry {
    peers: BTreeMap<String, DiscoveredPeer>,
}

impl PeerRegistry {
    /// A new, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of peers currently held (including not-yet-expired stale ones;
    /// call [`PeerRegistry::expire`] first to drop dead entries).
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether the registry holds no peers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Borrow a peer by node id.
    #[must_use]
    pub fn get(&self, node_id: &str) -> Option<&DiscoveredPeer> {
        self.peers.get(node_id)
    }

    /// All peers, in node-id order.
    #[must_use]
    pub fn peers(&self) -> Vec<&DiscoveredPeer> {
        self.peers.values().collect()
    }

    /// Fold an advertised record into the cache at clock tick `now`.
    ///
    /// Dedupes by node id: a record for a known node refreshes that peer
    /// rather than adding a duplicate. If the address set differs from what we
    /// held, the peer is updated and [`ObserveEvent::AddressChanged`] is
    /// returned. A record that cannot become a valid peer is rejected and the
    /// registry is left unchanged.
    pub fn observe(&mut self, record: &ServiceRecord, now: u64) -> ObserveEvent {
        let txt = &record.txt;
        let candidate = match DiscoveredPeer::new(
            txt.node_id.clone(),
            record.hostname(),
            record.addresses(),
            record.port(),
            txt.role,
            txt.version,
            now,
            u64::from(record.ttl),
        ) {
            Ok(p) => p,
            Err(e) => return ObserveEvent::Rejected(e),
        };

        match self.peers.get(candidate.node_id()) {
            None => {
                self.peers.insert(candidate.node_id().to_owned(), candidate);
                ObserveEvent::NewPeer
            }
            Some(existing) => {
                let previous = existing.addresses().to_vec();
                let moved = previous != candidate.addresses();
                self.peers.insert(candidate.node_id().to_owned(), candidate);
                if moved {
                    ObserveEvent::AddressChanged { previous }
                } else {
                    ObserveEvent::Refreshed
                }
            }
        }
    }

    /// Drop every peer whose freshness window closed at or before `now`,
    /// returning the node ids removed (in node-id order).
    pub fn expire(&mut self, now: u64) -> Vec<String> {
        let dead: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, p)| p.is_expired(now))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &dead {
            self.peers.remove(id);
        }
        dead
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::announce::announce_self;
    use crate::peer::NodeRole;
    use std::net::Ipv4Addr;

    fn record_with(node_id: &str, addr: u8) -> ServiceRecord {
        announce_self(
            node_id,
            "kitchen.local",
            NodeRole::Primary,
            "1.4.0",
            8123,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, addr)),
        )
        .expect("valid record")
    }

    #[test]
    fn observe_adds_new_peer() {
        let mut reg = PeerRegistry::new();
        let ev = reg.observe(&record_with("hub-a", 10), 0);
        assert_eq!(ev, ObserveEvent::NewPeer);
        assert!(ev.is_new());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("hub-a").expect("present").port(), 8123);
    }

    #[test]
    fn observe_dedupes_by_node_id() {
        let mut reg = PeerRegistry::new();
        reg.observe(&record_with("hub-a", 10), 0);
        let ev = reg.observe(&record_with("hub-a", 10), 50);
        assert_eq!(ev, ObserveEvent::Refreshed);
        assert_eq!(reg.len(), 1, "same node id must not duplicate");
        assert_eq!(reg.get("hub-a").expect("present").last_seen(), 50);
    }

    #[test]
    fn observe_detects_address_change() {
        let mut reg = PeerRegistry::new();
        reg.observe(&record_with("hub-a", 10), 0);
        let ev = reg.observe(&record_with("hub-a", 99), 5);
        assert_eq!(
            ev,
            ObserveEvent::AddressChanged {
                previous: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))]
            }
        );
        assert_eq!(
            reg.get("hub-a").expect("present").addresses(),
            &[IpAddr::V4(Ipv4Addr::new(192, 168, 1, 99))]
        );
    }

    #[test]
    fn distinct_node_ids_coexist() {
        let mut reg = PeerRegistry::new();
        reg.observe(&record_with("hub-a", 10), 0);
        reg.observe(&record_with("hub-b", 11), 0);
        assert_eq!(reg.len(), 2);
        let ids: Vec<&str> = reg.peers().iter().map(|p| p.node_id()).collect();
        assert_eq!(ids, vec!["hub-a", "hub-b"]);
    }

    #[test]
    fn expire_drops_only_stale_peers() {
        let mut reg = PeerRegistry::new();
        // TTL is 120s (DEFAULT_TTL_SECS); seen at tick 0 -> alive through 120.
        reg.observe(&record_with("hub-a", 10), 0);
        reg.observe(&record_with("hub-b", 11), 100);
        let removed = reg.expire(121);
        assert_eq!(removed, vec!["hub-a".to_owned()]);
        assert_eq!(reg.len(), 1);
        assert!(reg.get("hub-b").is_some());
    }

    #[test]
    fn expire_noop_when_all_fresh() {
        let mut reg = PeerRegistry::new();
        reg.observe(&record_with("hub-a", 10), 10);
        let removed = reg.expire(50);
        assert!(removed.is_empty());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn empty_registry_basics() {
        let reg = PeerRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get("nobody").is_none());
    }

    #[test]
    fn refresh_after_expiry_window_extends_life() {
        let mut reg = PeerRegistry::new();
        reg.observe(&record_with("hub-a", 10), 0);
        // Re-observe before expiry, pushing last_seen forward.
        reg.observe(&record_with("hub-a", 10), 100);
        let removed = reg.expire(121);
        assert!(removed.is_empty(), "refreshed peer should survive");
    }
}
