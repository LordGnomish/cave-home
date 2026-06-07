// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `cache` plugin: positive + negative response caching.

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
        c.insert(key("a.example.com", RecordType::A), &positive("a.example.com", 100), 0);
        let got = c.get(&key("a.example.com", RecordType::A), 0).unwrap();
        assert_eq!(got.answers.len(), 1);
    }

    #[test]
    fn ttl_counts_down_with_elapsed_time() {
        let mut c = Cache::new(16);
        c.insert(key("a.example.com", RecordType::A), &positive("a.example.com", 100), 1000);
        let got = c.get(&key("a.example.com", RecordType::A), 1030).unwrap();
        assert_eq!(got.answers[0].ttl, 70);
    }

    #[test]
    fn expired_entry_is_a_miss() {
        let mut c = Cache::new(16);
        c.insert(key("a.example.com", RecordType::A), &positive("a.example.com", 100), 0);
        assert!(c.get(&key("a.example.com", RecordType::A), 101).is_none());
        assert_eq!(c.len(), 0, "expired entry should be evicted on access");
    }

    #[test]
    fn zero_ttl_responses_are_not_cached() {
        let mut c = Cache::new(16);
        c.insert(key("a.example.com", RecordType::A), &positive("a.example.com", 0), 0);
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
        assert_eq!(c.get(&key("nope.example.com", RecordType::A), 49).unwrap().header.rcode, Rcode::NxDomain);
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
        c.insert(key("a.example.com", RecordType::A), &positive("a.example.com", 100), 0);
        assert!(c.get(&key("a.example.com", RecordType::Aaaa), 0).is_none());
        assert!(c.get(&key("a.example.com", RecordType::A), 0).is_some());
    }

    #[test]
    fn plugin_serves_a_hit_without_calling_downstream() {
        let hits = Rc::new(Cell::new(0));
        let cache = CachePlugin::new(16);
        let chain = Chain::new(vec![
            Box::new(cache),
            Box::new(Counter { hits: hits.clone(), answer_ttl: 300 }),
        ]);
        let q = Message::query(Name::parse("x.example.com").unwrap(), RecordType::A, 7);
        let first = chain.handle(&q);
        let second = chain.handle(&Message::query(Name::parse("x.example.com").unwrap(), RecordType::A, 99));
        assert_eq!(hits.get(), 1, "second query must be served from cache");
        assert_eq!(first.answers.len(), 1);
        // The cache hit echoes the *second* query's id, not the cached one.
        assert_eq!(second.header.id, 99);
    }
}
