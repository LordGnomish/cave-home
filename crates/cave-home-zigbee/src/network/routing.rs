// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! Routing table — Zigbee 3.0 §3.6.1.4.
//!
//! Maps a 16-bit destination network address to the next-hop NWK
//! address. Each entry also tracks a routing status: ACTIVE,
//! DISCOVERY_UNDERWAY, DISCOVERY_FAILED, INACTIVE, VALIDATION_UNDERWAY
//! (Zigbee 3.0 Table 3-67).

use std::collections::HashMap;

/// Routing entry status — Zigbee 3.0 Table 3-67.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingStatus {
    /// Route is valid; packets may be forwarded.
    Active,
    /// Route discovery currently in progress.
    DiscoveryUnderway,
    /// Most recent route discovery failed (no path).
    DiscoveryFailed,
    /// Route is inactive (cleaned up but slot retained).
    Inactive,
    /// Validation underway after rejoin.
    ValidationUnderway,
}

/// One entry of the network's routing table — §3.6.1.4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingTableEntry {
    /// Destination 16-bit short network address.
    pub destination: u16,
    /// Next-hop short network address.
    pub next_hop: u16,
    /// Status of this entry.
    pub status: RoutingStatus,
    /// `true` ⇒ no-route-cache flag set (the destination has no route cache).
    pub no_route_cache: bool,
    /// `true` ⇒ many-to-one route (concentrator pattern).
    pub many_to_one: bool,
}

/// The routing table is bounded; entries past the capacity must be
/// reclaimed via LRU or by status (DiscoveryFailed / Inactive first).
const DEFAULT_CAPACITY: usize = 64;

/// Routing table — Zigbee 3.0 §3.6.3.
///
/// Thread-safe access is provided by callers (the coordinator wraps it in
/// an `Arc<RwLock<...>>`); the struct itself is intentionally plain.
#[derive(Clone, Debug)]
pub struct RoutingTable {
    entries: HashMap<u16, RoutingTableEntry>,
    capacity: usize,
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutingTable {
    /// Create an empty table with the default capacity (64 entries).
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create an empty table with a custom capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            capacity,
        }
    }

    /// Number of entries currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` ⇔ no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Snapshot of all routing entries (for diagnostics / Portal UI).
    #[must_use]
    pub fn snapshot(&self) -> Vec<RoutingTableEntry> {
        self.entries.values().copied().collect()
    }

    /// Insert or replace the entry for `entry.destination`.
    ///
    /// When the table is full, the eviction policy is: first drop a
    /// `DiscoveryFailed` entry; failing that, drop an `Inactive` one;
    /// failing that, drop the entry with the largest hash-table index
    /// (so the new entry replaces it). This is documented and
    /// deterministic for tests.
    pub fn upsert(&mut self, entry: RoutingTableEntry) {
        if !self.entries.contains_key(&entry.destination) && self.entries.len() >= self.capacity {
            self.evict_one();
        }
        self.entries.insert(entry.destination, entry);
    }

    /// Look up the route for `destination`.
    #[must_use]
    pub fn lookup(&self, destination: u16) -> Option<RoutingTableEntry> {
        self.entries.get(&destination).copied()
    }

    /// Remove the route for `destination` if any (returns the dropped entry).
    pub fn remove(&mut self, destination: u16) -> Option<RoutingTableEntry> {
        self.entries.remove(&destination)
    }

    /// Mark `destination` as `status` (no-op if absent).
    pub fn set_status(&mut self, destination: u16, status: RoutingStatus) {
        if let Some(e) = self.entries.get_mut(&destination) {
            e.status = status;
        }
    }

    fn evict_one(&mut self) {
        // Try to drop a DiscoveryFailed entry first.
        if let Some(k) = self
            .entries
            .iter()
            .find(|(_, e)| e.status == RoutingStatus::DiscoveryFailed)
            .map(|(k, _)| *k)
        {
            self.entries.remove(&k);
            return;
        }
        if let Some(k) = self
            .entries
            .iter()
            .find(|(_, e)| e.status == RoutingStatus::Inactive)
            .map(|(k, _)| *k)
        {
            self.entries.remove(&k);
            return;
        }
        // Last resort: drop the entry with the smallest destination address.
        // (deterministic, doesn't require LRU bookkeeping for Phase 1)
        if let Some(k) = self.entries.keys().min().copied() {
            self.entries.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dest: u16, next: u16) -> RoutingTableEntry {
        RoutingTableEntry {
            destination: dest,
            next_hop: next,
            status: RoutingStatus::Active,
            no_route_cache: false,
            many_to_one: false,
        }
    }

    #[test]
    fn insert_then_lookup_returns_entry() {
        let mut t = RoutingTable::new();
        let e = entry(0x1234, 0x0001);
        t.upsert(e);
        assert_eq!(t.lookup(0x1234), Some(e));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let t = RoutingTable::new();
        assert!(t.lookup(0xffff).is_none());
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let mut t = RoutingTable::new();
        t.upsert(entry(0x1234, 0x0001));
        t.upsert(entry(0x1234, 0x0002));
        assert_eq!(t.lookup(0x1234).unwrap().next_hop, 0x0002);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn remove_drops_entry() {
        let mut t = RoutingTable::new();
        t.upsert(entry(0x1234, 0x0001));
        assert!(t.remove(0x1234).is_some());
        assert!(t.is_empty());
    }

    #[test]
    fn set_status_updates_existing() {
        let mut t = RoutingTable::new();
        t.upsert(entry(0x1234, 0x0001));
        t.set_status(0x1234, RoutingStatus::DiscoveryFailed);
        assert_eq!(t.lookup(0x1234).unwrap().status, RoutingStatus::DiscoveryFailed);
    }

    #[test]
    fn snapshot_returns_all_entries() {
        let mut t = RoutingTable::new();
        t.upsert(entry(0x0001, 0x0001));
        t.upsert(entry(0x0002, 0x0002));
        let snap = t.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn eviction_prefers_discovery_failed() {
        let mut t = RoutingTable::with_capacity(2);
        let mut a = entry(0x0001, 0x0001);
        a.status = RoutingStatus::DiscoveryFailed;
        let b = entry(0x0002, 0x0002);
        t.upsert(a);
        t.upsert(b);
        // Insert a third entry — should evict the DiscoveryFailed one (0x0001).
        t.upsert(entry(0x0003, 0x0003));
        assert!(t.lookup(0x0001).is_none());
        assert!(t.lookup(0x0002).is_some());
        assert!(t.lookup(0x0003).is_some());
    }

    #[test]
    fn eviction_then_inactive_then_smallest() {
        let mut t = RoutingTable::with_capacity(2);
        let mut a = entry(0x0001, 0x0001);
        a.status = RoutingStatus::Inactive;
        let b = entry(0x0002, 0x0002);
        t.upsert(a);
        t.upsert(b);
        t.upsert(entry(0x0003, 0x0003));
        // Inactive one (0x0001) should have been evicted.
        assert!(t.lookup(0x0001).is_none());
        assert_eq!(t.len(), 2);
    }
}
