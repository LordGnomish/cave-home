//! Response cache — TTL-based, pure, caller-supplied clock.
//!
//! First-party from the *public* DNS caching rules (RFC 1035 §3.2.1 / RFC 2181
//! §8: a record may be cached for its TTL, after which it must be discarded).
//! Unbound also offers `cache-min-ttl` / `cache-max-ttl` clamps; both are
//! modelled here.
//!
//! The cache takes no real clock — the caller supplies "now" in seconds on
//! every call — so the logic is deterministic and fully testable without
//! sleeping. Entry I/O for the actual resolver loop is Phase 1b.

use crate::name::DnsName;
use crate::record::{Record, RecordType};
use std::collections::HashMap;

/// One cached answer: the records and the absolute second at which they expire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    records: Vec<Record>,
    /// Absolute expiry, in the same "seconds" unit the caller passes as `now`.
    expires_at: u64,
}

impl CacheEntry {
    /// The cached records.
    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Is the entry still live at `now`?
    #[must_use]
    pub const fn is_live(&self, now: u64) -> bool {
        now < self.expires_at
    }

    /// Seconds remaining before expiry at `now` (0 once expired).
    #[must_use]
    pub const fn ttl_remaining(&self, now: u64) -> u64 {
        self.expires_at.saturating_sub(now)
    }
}

/// TTL clamp bounds applied on insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtlClamp {
    /// Lower bound: TTLs below this are raised to it.
    pub min: u32,
    /// Upper bound: TTLs above this are lowered to it.
    pub max: u32,
}

impl Default for TtlClamp {
    fn default() -> Self {
        // Sensible Unbound-style defaults: don't re-query sub-second, don't
        // pin stale answers for longer than a day.
        Self {
            min: 0,
            max: 86_400,
        }
    }
}

impl TtlClamp {
    /// Clamp a raw TTL into `[min, max]`.
    #[must_use]
    pub const fn apply(&self, ttl: u32) -> u32 {
        if ttl < self.min {
            self.min
        } else if ttl > self.max {
            self.max
        } else {
            ttl
        }
    }
}

/// A pure TTL response cache keyed on (name, type).
#[derive(Debug, Clone, Default)]
pub struct ResponseCache {
    map: HashMap<(DnsName, RecordType), CacheEntry>,
    clamp: TtlClamp,
}

impl ResponseCache {
    /// A new cache with default TTL clamps.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            clamp: TtlClamp::default(),
        }
    }

    /// A new cache with explicit TTL clamps.
    #[must_use]
    pub fn with_clamp(clamp: TtlClamp) -> Self {
        Self {
            map: HashMap::new(),
            clamp,
        }
    }

    /// Number of entries currently held (live or not, until evicted).
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Is the cache empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Insert an answer for (`name`, `rtype`) with a raw `ttl` (clamped),
    /// computing the absolute expiry from `now`. Replaces any existing entry.
    /// Returns the clamped TTL actually used.
    pub fn insert(
        &mut self,
        name: DnsName,
        rtype: RecordType,
        records: Vec<Record>,
        ttl: u32,
        now: u64,
    ) -> u32 {
        let clamped = self.clamp.apply(ttl);
        let entry = CacheEntry {
            records,
            expires_at: now.saturating_add(u64::from(clamped)),
        };
        self.map.insert((name, rtype), entry);
        clamped
    }

    /// Look up a live answer at `now`. An expired entry yields `None` (and is
    /// left for [`evict_expired`](Self::evict_expired) to reap).
    #[must_use]
    pub fn lookup(&self, name: &DnsName, rtype: RecordType, now: u64) -> Option<&CacheEntry> {
        self.map
            .get(&(name.clone(), rtype))
            .filter(|e| e.is_live(now))
    }

    /// Drop every entry that has expired at `now`. Returns how many were
    /// removed.
    pub fn evict_expired(&mut self, now: u64) -> usize {
        let before = self.map.len();
        self.map.retain(|_, e| e.is_live(now));
        before - self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RecordType;

    fn name(n: &str) -> DnsName {
        DnsName::parse(n).expect("name")
    }

    fn recs(ip: &str) -> Vec<Record> {
        vec![Record::address("nas.home.arpa", RecordType::A, ip).expect("rec")]
    }

    #[test]
    fn insert_then_lookup_within_ttl() {
        let mut c = ResponseCache::new();
        c.insert(name("nas.home.arpa"), RecordType::A, recs("10.0.0.1"), 300, 1_000);
        let hit = c.lookup(&name("nas.home.arpa"), RecordType::A, 1_100);
        assert!(hit.is_some());
        assert_eq!(hit.expect("hit").records()[0].data.to_text(), "10.0.0.1");
    }

    #[test]
    fn lookup_misses_after_expiry() {
        let mut c = ResponseCache::new();
        c.insert(name("nas.home.arpa"), RecordType::A, recs("10.0.0.1"), 300, 1_000);
        // At exactly now+ttl it is expired (now < expires_at is false).
        assert!(c.lookup(&name("nas.home.arpa"), RecordType::A, 1_300).is_none());
        assert!(c.lookup(&name("nas.home.arpa"), RecordType::A, 5_000).is_none());
    }

    #[test]
    fn ttl_is_clamped_to_min_and_max() {
        let clamp = TtlClamp { min: 60, max: 3_600 };
        let mut c = ResponseCache::with_clamp(clamp);
        // Below min -> raised.
        let used = c.insert(name("a.home.arpa"), RecordType::A, recs("10.0.0.2"), 5, 0);
        assert_eq!(used, 60);
        assert!(c.lookup(&name("a.home.arpa"), RecordType::A, 59).is_some());
        assert!(c.lookup(&name("a.home.arpa"), RecordType::A, 60).is_none());

        // Above max -> lowered.
        let used = c.insert(name("b.home.arpa"), RecordType::A, recs("10.0.0.3"), 99_999, 0);
        assert_eq!(used, 3_600);
    }

    #[test]
    fn ttl_remaining_counts_down_and_floors_at_zero() {
        let mut c = ResponseCache::new();
        c.insert(name("nas.home.arpa"), RecordType::A, recs("10.0.0.1"), 300, 1_000);
        let e = c
            .map
            .get(&(name("nas.home.arpa"), RecordType::A))
            .expect("entry");
        assert_eq!(e.ttl_remaining(1_000), 300);
        assert_eq!(e.ttl_remaining(1_250), 50);
        assert_eq!(e.ttl_remaining(9_999), 0);
    }

    #[test]
    fn evict_expired_reaps_only_dead_entries() {
        let mut c = ResponseCache::new();
        c.insert(name("short.home.arpa"), RecordType::A, recs("10.0.0.4"), 10, 0);
        c.insert(name("long.home.arpa"), RecordType::A, recs("10.0.0.5"), 1_000, 0);
        assert_eq!(c.len(), 2);
        let removed = c.evict_expired(100);
        assert_eq!(removed, 1);
        assert_eq!(c.len(), 1);
        assert!(c.lookup(&name("long.home.arpa"), RecordType::A, 100).is_some());
    }

    #[test]
    fn insert_replaces_existing_entry() {
        let mut c = ResponseCache::new();
        c.insert(name("nas.home.arpa"), RecordType::A, recs("10.0.0.1"), 300, 0);
        c.insert(name("nas.home.arpa"), RecordType::A, recs("10.0.0.99"), 300, 0);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c.lookup(&name("nas.home.arpa"), RecordType::A, 1)
                .expect("hit")
                .records()[0]
                .data
                .to_text(),
            "10.0.0.99"
        );
    }
}
