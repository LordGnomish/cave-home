// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `cache` plugin: positive + negative response caching.
//!
//! The cache is keyed by `(name, qtype, qclass, DO-bit)`. A stored entry
//! expires after a TTL computed from the response: the minimum record TTL for a
//! positive answer (clamped to [`MAX_POSITIVE_TTL`]) or the authority `SOA`
//! minimum for a negative answer (`NXDOMAIN`/`NODATA`, clamped to
//! [`MAX_NEGATIVE_TTL`]); a zero TTL is never cached (`CoreDNS` `cache` docs).
//! On a hit the served record TTLs are counted down by the elapsed time.
//!
//! Time is **caller-supplied** ([`Cache::get`]/[`Cache::insert`] take `now` in
//! seconds) so the decision core is testable without a clock. [`CachePlugin`]
//! wraps it for the chain with a test-advanceable clock; wiring a real
//! wall-clock is the deferred I/O shell (`parity.manifest.toml`).

use crate::message::{Message, Question};
use crate::name::Name;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::{Class, RecordType};
use crate::wire::Rcode;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// The upper bound on a cached positive answer's TTL (`CoreDNS` default).
pub const MAX_POSITIVE_TTL: u32 = 3600;
/// The upper bound on a cached negative answer's TTL (`CoreDNS` default).
pub const MAX_NEGATIVE_TTL: u32 = 1800;

/// The cache lookup key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    name: Name,
    qtype: RecordType,
    qclass: Class,
    /// The EDNS DO bit (DNSSEC-OK); responses differ when set.
    dnssec_ok: bool,
}

impl CacheKey {
    /// Build a key from a question (DO bit currently always `false` — EDNS
    /// parsing is deferred).
    #[must_use]
    pub fn from_question(q: &Question) -> Self {
        Self {
            name: q.name.clone(),
            qtype: q.qtype,
            qclass: q.qclass,
            dnssec_ok: false,
        }
    }
}

/// A cached response with its absolute expiry and insertion time.
struct Entry {
    response: Message,
    inserted_at: u64,
    expires_at: u64,
}

/// A TTL-bounded response cache with capacity eviction.
pub struct Cache {
    entries: HashMap<CacheKey, Entry>,
    capacity: usize,
}

impl Cache {
    /// A cache holding up to `capacity` entries.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// The number of live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Fetch a cached response, counting its TTLs down by the elapsed time.
    /// An expired entry is removed and reported as a miss.
    #[must_use]
    pub fn get(&mut self, key: &CacheKey, now: u64) -> Option<Message> {
        let entry = self.entries.get(key)?;
        if now >= entry.expires_at {
            self.entries.remove(key);
            return None;
        }
        let elapsed = (now - entry.inserted_at) as u32;
        let mut response = entry.response.clone();
        for rr in response
            .answers
            .iter_mut()
            .chain(&mut response.authority)
            .chain(&mut response.additional)
        {
            rr.ttl = rr.ttl.saturating_sub(elapsed);
        }
        Some(response)
    }

    /// Cache a response under `key` as observed at `now`. Zero-TTL responses are
    /// not stored. Inserting past capacity evicts the soonest-to-expire entry.
    pub fn insert(&mut self, key: CacheKey, response: &Message, now: u64) {
        let Some(ttl) = cache_ttl(response) else {
            return;
        };
        if ttl == 0 {
            return;
        }
        if !self.entries.contains_key(&key) && self.entries.len() >= self.capacity {
            self.evict_one();
        }
        self.entries.insert(
            key,
            Entry {
                response: response.clone(),
                inserted_at: now,
                expires_at: now + u64::from(ttl),
            },
        );
    }

    /// Remove the entry closest to expiry (a cheap approximation of LRU that is
    /// deterministic for testing).
    fn evict_one(&mut self) {
        if let Some(victim) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.expires_at)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&victim);
        }
    }
}

/// The TTL to cache `response` for, or `None` if it should not be cached.
fn cache_ttl(response: &Message) -> Option<u32> {
    let is_negative = response.header.rcode == Rcode::NxDomain || response.answers.is_empty();
    if is_negative {
        // Negative TTL = the authority-section SOA minimum.
        let soa_min = response.authority.iter().find_map(|rr| match &rr.rdata {
            crate::rr::Rdata::Soa { minimum, .. } => Some(*minimum),
            _ => None,
        })?;
        Some(soa_min.min(MAX_NEGATIVE_TTL))
    } else {
        // Positive TTL = the minimum record TTL, excluding OPT pseudo-records.
        let min = response
            .answers
            .iter()
            .chain(&response.authority)
            .chain(&response.additional)
            .filter(|rr| rr.rtype() != RecordType::Opt)
            .map(|rr| rr.ttl)
            .min()?;
        Some(min.min(MAX_POSITIVE_TTL))
    }
}

/// The `cache` plugin: a [`Cache`] behind interior mutability plus a
/// test-advanceable clock.
pub struct CachePlugin {
    inner: RefCell<Cache>,
    now: Cell<u64>,
}

impl CachePlugin {
    /// A cache plugin holding up to `capacity` entries, clock at zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: RefCell::new(Cache::new(capacity)),
            now: Cell::new(0),
        }
    }

    /// Set the plugin's clock (seconds). Stands in for the wall-clock the live
    /// server would read.
    pub fn set_now(&self, now: u64) {
        self.now.set(now);
    }

    /// The number of cached entries.
    #[must_use]
    pub fn cached_entries(&self) -> usize {
        self.inner.borrow().len()
    }
}

impl Plugin for CachePlugin {
    fn name(&self) -> &'static str {
        "cache"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        let Some(q) = req.question() else {
            return next.run(req);
        };
        let key = CacheKey::from_question(q);
        let now = self.now.get();

        if let Some(mut hit) = self.inner.borrow_mut().get(&key, now) {
            // Re-stamp the live query's id and question onto the cached body.
            hit.header.id = req.id();
            hit.questions.clone_from(&req.query().questions);
            return Ok(hit);
        }

        let response = next.run(req)?;
        self.inner.borrow_mut().insert(key, &response, now);
        Ok(response)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::{Message, Question};
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request};
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::wire::Rcode;
    use std::cell::Cell;
    use std::net::Ipv4Addr;
    use std::rc::Rc;

    /// Downstream plugin that counts how often it is actually invoked, and
    /// answers a configurable record set.
    struct Counter {
        hits: Rc<Cell<u32>>,
        answer_ttl: u32,
    }
    impl Plugin for Counter {
        fn name(&self) -> &'static str {
            "counter"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            self.hits.set(self.hits.get() + 1);
            let owner = req.name().cloned().unwrap_or_else(Name::root);
            let mut reply = req.reply();
            reply.answers.push(ResourceRecord::new(
                owner,
                Class::In,
                self.answer_ttl,
                Rdata::A(Ipv4Addr::new(1, 2, 3, 4)),
            ));
            Ok(reply)
        }
    }

    fn key(name: &str, t: RecordType) -> CacheKey {
        CacheKey::from_question(&Question::new(Name::parse(name).unwrap(), t, Class::In))
    }

    fn positive(name: &str, ttl: u32) -> Message {
        let mut m = Message::query(Name::parse(name).unwrap(), RecordType::A, 1).reply();
        m.answers.push(ResourceRecord::new(
            Name::parse(name).unwrap(),
            Class::In,
            ttl,
            Rdata::A(Ipv4Addr::new(9, 9, 9, 9)),
        ));
        m
    }

    #[test]
    fn insert_then_get_within_ttl_hits() {
        let mut c = Cache::new(16);
        c.insert(
            key("a.example.com", RecordType::A),
            &positive("a.example.com", 100),
            0,
        );
        let got = c.get(&key("a.example.com", RecordType::A), 0).unwrap();
        assert_eq!(got.answers.len(), 1);
    }

    #[test]
    fn ttl_counts_down_with_elapsed_time() {
        let mut c = Cache::new(16);
        c.insert(
            key("a.example.com", RecordType::A),
            &positive("a.example.com", 100),
            1000,
        );
        let got = c.get(&key("a.example.com", RecordType::A), 1030).unwrap();
        assert_eq!(got.answers[0].ttl, 70);
    }

    #[test]
    fn expired_entry_is_a_miss() {
        let mut c = Cache::new(16);
        c.insert(
            key("a.example.com", RecordType::A),
            &positive("a.example.com", 100),
            0,
        );
        assert!(c.get(&key("a.example.com", RecordType::A), 101).is_none());
        assert_eq!(c.len(), 0, "expired entry should be evicted on access");
    }

    #[test]
    fn zero_ttl_responses_are_not_cached() {
        let mut c = Cache::new(16);
        c.insert(
            key("a.example.com", RecordType::A),
            &positive("a.example.com", 0),
            0,
        );
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn negative_responses_cache_for_the_soa_minimum() {
        let mut nxd = Message::query(Name::parse("nope.example.com").unwrap(), RecordType::A, 1)
            .reply()
            .with_rcode(Rcode::NxDomain);
        nxd.authority.push(ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            3600,
            Rdata::Soa {
                mname: Name::parse("ns.example.com").unwrap(),
                rname: Name::parse("hostmaster.example.com").unwrap(),
                serial: 1,
                refresh: 7200,
                retry: 3600,
                expire: 1_209_600,
                minimum: 50,
            },
        ));
        let mut c = Cache::new(16);
        c.insert(key("nope.example.com", RecordType::A), &nxd, 0);
        assert_eq!(
            c.get(&key("nope.example.com", RecordType::A), 49)
                .unwrap()
                .header
                .rcode,
            Rcode::NxDomain
        );
        assert!(c.get(&key("nope.example.com", RecordType::A), 51).is_none());
    }

    #[test]
    fn capacity_is_enforced_by_eviction() {
        let mut c = Cache::new(2);
        c.insert(key("a", RecordType::A), &positive("a", 100), 0);
        c.insert(key("b", RecordType::A), &positive("b", 100), 0);
        c.insert(key("c", RecordType::A), &positive("c", 100), 0);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn keys_distinguish_query_type() {
        let mut c = Cache::new(16);
        c.insert(
            key("a.example.com", RecordType::A),
            &positive("a.example.com", 100),
            0,
        );
        assert!(c.get(&key("a.example.com", RecordType::Aaaa), 0).is_none());
        assert!(c.get(&key("a.example.com", RecordType::A), 0).is_some());
    }

    #[test]
    fn plugin_serves_a_hit_without_calling_downstream() {
        let hits = Rc::new(Cell::new(0));
        let cache = CachePlugin::new(16);
        let chain = Chain::new(vec![
            Box::new(cache),
            Box::new(Counter {
                hits: hits.clone(),
                answer_ttl: 300,
            }),
        ]);
        let q = Message::query(Name::parse("x.example.com").unwrap(), RecordType::A, 7);
        let first = chain.handle(&q);
        let second = chain.handle(&Message::query(
            Name::parse("x.example.com").unwrap(),
            RecordType::A,
            99,
        ));
        assert_eq!(hits.get(), 1, "second query must be served from cache");
        assert_eq!(first.answers.len(), 1);
        // The cache hit echoes the *second* query's id, not the cached one.
        assert_eq!(second.header.id, 99);
    }
}
