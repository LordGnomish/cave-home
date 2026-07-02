// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `forward` plugin: upstream selection policy + health.
//!
//! [`Pool`] is the pure decision core: a set of upstreams, a load-balancing
//! [`Policy`], and per-upstream health driven by `max_fails` consecutive
//! failures (an upstream is marked *down* and excluded from selection; a
//! success revives it). [`Forward`] wraps a pool with an injected *transport*
//! and the query loop — it selects healthy upstreams in policy order, retries
//! on failure, returns the first success (re-stamped with the live id), and
//! `SERVFAIL`s if every upstream fails.
//!
//! The transport is a `Fn(&str, &Message) -> Option<Message>`; the real
//! UDP/TCP/TLS/QUIC/DoH client that implements it is the deferred I/O shell
//! (`parity.manifest.toml`). Everything that decides *which* upstream and
//! *whether to retry* lives here and is tested without a socket.

use crate::message::Message;
use crate::plugin::{Next, Outcome, Plugin, Request, ServerError};
use std::cell::RefCell;

/// The default `max_fails` before an upstream is marked down (`CoreDNS` default).
pub const DEFAULT_MAX_FAILS: u32 = 2;

/// How the pool spreads queries across upstreams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Rotate through the upstreams in order.
    RoundRobin,
    /// Pick a (pseudo-)random healthy upstream.
    Random,
    /// Always prefer the earliest healthy upstream.
    Sequential,
}

/// One upstream's address and health.
struct Health {
    addr: String,
    fails: u32,
    down: bool,
}

/// The upstream set, policy and health state — the `forward` decision core.
pub struct Pool {
    upstreams: Vec<Health>,
    policy: Policy,
    max_fails: u32,
    cursor: usize,
    rng: u64,
}

impl Pool {
    /// Build a pool over upstream addresses.
    #[must_use]
    pub fn new(addrs: Vec<String>, policy: Policy, max_fails: u32) -> Self {
        let upstreams = addrs
            .into_iter()
            .map(|addr| Health {
                addr,
                fails: 0,
                down: false,
            })
            .collect();
        Self {
            upstreams,
            policy,
            max_fails: max_fails.max(1),
            cursor: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// The number of upstreams.
    #[must_use]
    pub fn len(&self) -> usize {
        self.upstreams.len()
    }

    /// Whether there are no upstreams.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upstreams.is_empty()
    }

    /// The number of healthy (not-down) upstreams.
    #[must_use]
    pub fn healthy(&self) -> usize {
        self.upstreams.iter().filter(|u| !u.down).count()
    }

    /// Whether upstream `idx` is currently down.
    #[must_use]
    pub fn is_down(&self, idx: usize) -> bool {
        self.upstreams.get(idx).is_some_and(|u| u.down)
    }

    /// The address of upstream `idx`.
    #[must_use]
    pub fn addr(&self, idx: usize) -> &str {
        self.upstreams.get(idx).map_or("", |u| u.addr.as_str())
    }

    /// Record the result of a query to upstream `idx`: a success resets and
    /// revives it; a failure increments its counter and marks it down at
    /// `max_fails`.
    pub fn record(&mut self, idx: usize, ok: bool) {
        if let Some(u) = self.upstreams.get_mut(idx) {
            if ok {
                u.fails = 0;
                u.down = false;
            } else {
                u.fails += 1;
                if u.fails >= self.max_fails {
                    u.down = true;
                }
            }
        }
    }

    /// Select the next healthy upstream per the policy, advancing any policy
    /// state. `None` if every upstream is down.
    pub fn select(&mut self) -> Option<usize> {
        if self.healthy() == 0 {
            return None;
        }
        match self.policy {
            Policy::Sequential => self.upstreams.iter().position(|u| !u.down),
            Policy::RoundRobin => {
                let n = self.upstreams.len();
                for i in 0..n {
                    let idx = (self.cursor + i) % n;
                    if !self.upstreams[idx].down {
                        self.cursor = (idx + 1) % n;
                        return Some(idx);
                    }
                }
                None
            }
            Policy::Random => {
                let healthy: Vec<usize> = (0..self.upstreams.len())
                    .filter(|&i| !self.upstreams[i].down)
                    .collect();
                // SplitMix64-style step; deterministic but well-spread.
                self.rng = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(1);
                let pick = (self.rng >> 33) as usize % healthy.len();
                Some(healthy[pick])
            }
        }
    }

    /// The healthy upstreams in policy order, each appearing once — the order
    /// the `forward` loop tries them in for a single query.
    fn try_order(&mut self) -> Vec<usize> {
        let n = self.upstreams.len();
        match self.policy {
            Policy::Sequential => (0..n).filter(|&i| !self.upstreams[i].down).collect(),
            Policy::RoundRobin => {
                let start = self.cursor;
                self.cursor = (self.cursor + 1) % n.max(1);
                (0..n)
                    .map(|i| (start + i) % n)
                    .filter(|&i| !self.upstreams[i].down)
                    .collect()
            }
            Policy::Random => {
                let mut healthy: Vec<usize> = (0..n).filter(|&i| !self.upstreams[i].down).collect();
                // Fisher–Yates with the SplitMix step.
                for i in (1..healthy.len()).rev() {
                    self.rng = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(1);
                    let j = (self.rng >> 33) as usize % (i + 1);
                    healthy.swap(i, j);
                }
                healthy
            }
        }
    }
}

/// A transport that exchanges a query with one upstream, returning the reply or
/// `None` on failure. The real network client implements this; tests inject a
/// fake. (`&str` is the upstream address.)
pub type Transport = Box<dyn Fn(&str, &Message) -> Option<Message>>;

/// The `forward` plugin: a [`Pool`] plus a transport.
pub struct Forward {
    pool: RefCell<Pool>,
    transport: Transport,
}

impl Forward {
    /// A forwarder over the given upstreams and policy, with `max_fails` =
    /// [`DEFAULT_MAX_FAILS`] and a transport that always fails (replace it with
    /// [`Forward::with_transport`]).
    #[must_use]
    pub fn new(addrs: Vec<String>, policy: Policy) -> Self {
        Self {
            pool: RefCell::new(Pool::new(addrs, policy, DEFAULT_MAX_FAILS)),
            transport: Box::new(|_, _| None),
        }
    }

    /// Set the transport (builder style).
    #[must_use]
    pub fn with_transport(mut self, transport: Transport) -> Self {
        self.transport = transport;
        self
    }
}

impl Plugin for Forward {
    fn name(&self) -> &'static str {
        "forward"
    }

    fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
        let order = self.pool.borrow_mut().try_order();
        for idx in order {
            let addr = self.pool.borrow().addr(idx).to_string();
            match (self.transport)(&addr, req.query()) {
                Some(mut reply) => {
                    self.pool.borrow_mut().record(idx, true);
                    reply.header.id = req.id();
                    reply.questions.clone_from(&req.query().questions);
                    return Ok(reply);
                }
                None => self.pool.borrow_mut().record(idx, false),
            }
        }
        Err(ServerError::Backend("all upstreams failed"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::Chain;
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::wire::Rcode;
    use std::net::Ipv4Addr;

    fn pool(policy: Policy) -> Pool {
        Pool::new(
            vec![
                "10.0.0.1:53".into(),
                "10.0.0.2:53".into(),
                "10.0.0.3:53".into(),
            ],
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
        let reply = chain.handle(&Message::query(
            Name::parse("example.com").unwrap(),
            RecordType::A,
            77,
        ));
        assert_eq!(reply.header.id, 77);
        assert_eq!(reply.answers.len(), 1);
    }

    #[test]
    fn plugin_fails_over_to_a_healthy_upstream() {
        // Upstream 0 always fails; upstream 1 succeeds.
        let fwd = Forward::new(vec!["a:53".into(), "b:53".into()], Policy::Sequential)
            .with_transport(Box::new(|addr, _q| {
                if addr == "a:53" {
                    None
                } else {
                    Some(canned(2))
                }
            }));
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
