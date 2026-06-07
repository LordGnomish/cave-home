// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `forward` plugin: upstream selection policy + health.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::plugin::{Chain, Plugin};
    use crate::wire::Rcode;
    use std::net::Ipv4Addr;

    fn pool(policy: Policy) -> Pool {
        Pool::new(
            vec!["10.0.0.1:53".into(), "10.0.0.2:53".into(), "10.0.0.3:53".into()],
            policy,
            2,
        )
    }

    #[test]
    fn round_robin_cycles_through_upstreams() {
        let mut p = pool(Policy::RoundRobin);
        let picks: Vec<_> = (0..4)
            .map(|_| {
                let i = p.select().unwrap();
                p.record(i, true);
                i
            })
            .collect();
        assert_eq!(picks, vec![0, 1, 2, 0]);
    }

    #[test]
    fn sequential_prefers_the_first_healthy() {
        let mut p = pool(Policy::Sequential);
        assert_eq!(p.select(), Some(0));
        assert_eq!(p.select(), Some(0)); // sticky until it goes down
    }

    #[test]
    fn max_fails_marks_an_upstream_down_and_excludes_it() {
        let mut p = pool(Policy::Sequential);
        assert!(!p.is_down(0));
        p.record(0, false);
        assert!(!p.is_down(0), "one failure is below max_fails");
        p.record(0, false);
        assert!(p.is_down(0), "two failures reach max_fails");
        assert_eq!(p.healthy(), 2);
        // Sequential now skips the downed first upstream.
        assert_eq!(p.select(), Some(1));
    }

    #[test]
    fn a_success_revives_a_downed_upstream() {
        let mut p = pool(Policy::Sequential);
        p.record(0, false);
        p.record(0, false);
        assert!(p.is_down(0));
        p.record(0, true);
        assert!(!p.is_down(0));
        assert_eq!(p.healthy(), 3);
    }

    #[test]
    fn all_down_selects_nothing() {
        let mut p = pool(Policy::RoundRobin);
        for i in 0..3 {
            p.record(i, false);
            p.record(i, false);
        }
        assert_eq!(p.healthy(), 0);
        assert_eq!(p.select(), None);
    }

    #[test]
    fn random_only_ever_returns_healthy_upstreams() {
        let mut p = pool(Policy::Random);
        p.record(1, false);
        p.record(1, false); // take #1 down
        for _ in 0..50 {
            let i = p.select().unwrap();
            assert_ne!(i, 1, "downed upstream must never be selected");
        }
    }

    fn query() -> Message {
        Message::query(Name::parse("example.com").unwrap(), RecordType::A, 5)
    }

    fn canned(id_ip: u8) -> Message {
        let mut m = query().reply();
        m.answers.push(ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            30,
            Rdata::A(Ipv4Addr::new(203, 0, 113, id_ip)),
        ));
        m
    }

    #[test]
    fn plugin_returns_the_upstream_reply_with_the_live_id() {
        // Transport that always succeeds from upstream 0.
        let fwd = Forward::new(vec!["10.0.0.1:53".into()], Policy::Sequential)
            .with_transport(Box::new(|_addr, _q| Some(canned(1))));
        let chain = Chain::new(vec![Box::new(fwd)]);
        let reply = chain.handle(&Message::query(Name::parse("example.com").unwrap(), RecordType::A, 77));
        assert_eq!(reply.header.id, 77);
        assert_eq!(reply.answers.len(), 1);
    }

    #[test]
    fn plugin_fails_over_to_a_healthy_upstream() {
        // Upstream 0 always fails; upstream 1 succeeds.
        let fwd = Forward::new(vec!["a:53".into(), "b:53".into()], Policy::Sequential)
            .with_transport(Box::new(|addr, _q| if addr == "a:53" { None } else { Some(canned(2)) }));
        let chain = Chain::new(vec![Box::new(fwd)]);
        let reply = chain.handle(&query());
        assert_eq!(reply.header.rcode, Rcode::NoError);
        assert_eq!(reply.answers.len(), 1);
    }

    #[test]
    fn plugin_servfails_when_all_upstreams_fail() {
        let fwd = Forward::new(vec!["a:53".into(), "b:53".into()], Policy::RoundRobin)
            .with_transport(Box::new(|_addr, _q| None));
        let chain = Chain::new(vec![Box::new(fwd)]);
        assert_eq!(chain.handle(&query()).header.rcode, Rcode::ServFail);
    }
}
