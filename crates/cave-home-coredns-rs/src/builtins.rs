// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Operational plugins: `metrics`/`prometheus`, `errors`, `ready`, `log`.
//!
//! These are the small operational plugins. [`Metrics`] is middleware that
//! counts queries by type and responses by rcode (the data the deferred
//! Prometheus exposition would publish). [`Errors`] turns a downstream failure
//! into `SERVFAIL` and tallies it. [`Ready`] is a readiness gate that answers
//! `SERVFAIL` until the server is marked ready. [`format_log_line`] renders the
//! per-query log line; the actual sink (stdout / a file) is the deferred I/O.

use crate::message::Message;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::RecordType;
use crate::wire::Rcode;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// A point-in-time view of the metric counters.
#[derive(Debug, Clone, Default)]
pub struct MetricsSnapshot {
    /// Total queries seen.
    pub total: u64,
    queries_by_type: BTreeMap<u16, u64>,
    responses_by_rcode: BTreeMap<u8, u64>,
}

impl MetricsSnapshot {
    /// Queries seen for a record type.
    #[must_use]
    pub fn queries(&self, qtype: RecordType) -> u64 {
        self.queries_by_type.get(&qtype.to_u16()).copied().unwrap_or(0)
    }

    /// Responses emitted with a response code.
    #[must_use]
    pub fn responses(&self, rcode: Rcode) -> u64 {
        self.responses_by_rcode.get(&(rcode as u8)).copied().unwrap_or(0)
    }
}

/// The `metrics` / `prometheus` plugin: a request/response counter.
#[derive(Default)]
pub struct Metrics {
    inner: RefCell<MetricsSnapshot>,
}

impl Metrics {
    /// A zeroed metrics counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot of the current counters.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.inner.borrow().clone()
    }
}

impl Plugin for Metrics {
    fn name(&self) -> &'static str {
        "metrics"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        if let Some(qtype) = req.qtype() {
            let mut m = self.inner.borrow_mut();
            m.total += 1;
            *m.queries_by_type.entry(qtype.to_u16()).or_insert(0) += 1;
        }
        let outcome = next.run(req);
        // An error surfaces to the client as SERVFAIL; count it as such.
        let rcode = outcome.as_ref().map_or(Rcode::ServFail, |reply| reply.header.rcode);
        *self.inner.borrow_mut().responses_by_rcode.entry(rcode as u8).or_insert(0) += 1;
        outcome
    }
}

/// The `errors` plugin: convert downstream failures into `SERVFAIL` and count.
#[derive(Default)]
pub struct Errors {
    count: Cell<u64>,
}

impl Errors {
    /// A new error counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of downstream errors observed.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count.get()
    }
}

impl Plugin for Errors {
    fn name(&self) -> &'static str {
        "errors"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        next.run(req).or_else(|_| {
            self.count.set(self.count.get() + 1);
            Ok(req.reply().with_rcode(Rcode::ServFail))
        })
    }
}

/// The `ready` plugin: gate the chain until the server is marked ready.
#[derive(Default)]
pub struct Ready {
    ready: Cell<bool>,
}

impl Ready {
    /// A gate that starts not-ready.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the server ready (or not).
    pub fn set_ready(&self, ready: bool) {
        self.ready.set(ready);
    }

    /// Whether the server is currently ready.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready.get()
    }
}

impl Plugin for Ready {
    fn name(&self) -> &'static str {
        "ready"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        if self.ready.get() {
            next.run(req)
        } else {
            Ok(req.reply().with_rcode(Rcode::ServFail))
        }
    }
}

/// Render a per-query log line: `<qname> <qtype> <rcode> <ancount>`.
#[must_use]
pub fn format_log_line(query: &Message, reply: &Message) -> String {
    let (name, qtype) = query
        .questions
        .first()
        .map_or_else(|| (".".to_string(), "ANY".to_string()), |q| (q.name.to_string(), rtype_str(q.qtype)));
    format!("{name} {qtype} {} {}", rcode_str(reply.header.rcode), reply.answers.len())
}

/// The mnemonic for a record type.
fn rtype_str(t: RecordType) -> String {
    match t {
        RecordType::A => "A".into(),
        RecordType::Ns => "NS".into(),
        RecordType::Cname => "CNAME".into(),
        RecordType::Soa => "SOA".into(),
        RecordType::Ptr => "PTR".into(),
        RecordType::Mx => "MX".into(),
        RecordType::Txt => "TXT".into(),
        RecordType::Aaaa => "AAAA".into(),
        RecordType::Srv => "SRV".into(),
        RecordType::Opt => "OPT".into(),
        RecordType::Any => "ANY".into(),
        RecordType::Unknown(v) => format!("TYPE{v}"),
    }
}

/// The mnemonic for a response code.
const fn rcode_str(rcode: Rcode) -> &'static str {
    match rcode {
        Rcode::NoError => "NOERROR",
        Rcode::FormErr => "FORMERR",
        Rcode::ServFail => "SERVFAIL",
        Rcode::NxDomain => "NXDOMAIN",
        Rcode::NotImp => "NOTIMP",
        Rcode::Refused => "REFUSED",
        Rcode::YxDomain => "YXDOMAIN",
        Rcode::YxrrSet => "YXRRSET",
        Rcode::NxrrSet => "NXRRSET",
        Rcode::NotAuth => "NOTAUTH",
        Rcode::NotZone => "NOTZONE",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request, ServerError};
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::wire::Rcode;
    use std::net::Ipv4Addr;
    use std::rc::Rc;

    /// Downstream that answers an A record, or fails / NXDOMAINs on demand.
    struct Backend {
        mode: Mode,
    }
    #[derive(Clone, Copy)]
    enum Mode {
        Ok,
        Nxdomain,
        Fail,
    }
    impl Plugin for Backend {
        fn name(&self) -> &'static str {
            "backend"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            match self.mode {
                Mode::Fail => Err(ServerError::Backend("down")),
                Mode::Nxdomain => Ok(req.reply().with_rcode(Rcode::NxDomain)),
                Mode::Ok => {
                    let mut r = req.reply();
                    r.answers.push(ResourceRecord::new(
                        req.name().cloned().unwrap_or_else(Name::root),
                        Class::In,
                        30,
                        Rdata::A(Ipv4Addr::new(1, 2, 3, 4)),
                    ));
                    Ok(r)
                }
            }
        }
    }

    fn q(t: RecordType) -> Message {
        Message::query(Name::parse("a.example.com").unwrap(), t, 1)
    }

    #[test]
    fn metrics_count_queries_by_type_and_responses_by_rcode() {
        let metrics = Rc::new(Metrics::new());
        let chain = Chain::new(vec![
            Box::new(metrics.clone()),
            Box::new(Backend { mode: Mode::Ok }),
        ]);
        let _ = chain.handle(&q(RecordType::A));
        let _ = chain.handle(&q(RecordType::A));
        let _ = chain.handle(&q(RecordType::Aaaa));
        let snap = metrics.snapshot();
        assert_eq!(snap.total, 3);
        assert_eq!(snap.queries(RecordType::A), 2);
        assert_eq!(snap.queries(RecordType::Aaaa), 1);
        assert_eq!(snap.responses(Rcode::NoError), 3);
    }

    #[test]
    fn metrics_record_nxdomain_responses() {
        let metrics = Rc::new(Metrics::new());
        let chain = Chain::new(vec![
            Box::new(metrics.clone()),
            Box::new(Backend { mode: Mode::Nxdomain }),
        ]);
        let _ = chain.handle(&q(RecordType::A));
        assert_eq!(metrics.snapshot().responses(Rcode::NxDomain), 1);
    }

    #[test]
    fn metrics_record_servfail_for_downstream_errors() {
        let metrics = Rc::new(Metrics::new());
        let chain = Chain::new(vec![
            Box::new(metrics.clone()),
            Box::new(Backend { mode: Mode::Fail }),
        ]);
        let _ = chain.handle(&q(RecordType::A));
        assert_eq!(metrics.snapshot().responses(Rcode::ServFail), 1);
    }

    #[test]
    fn errors_plugin_converts_failures_to_servfail_and_counts_them() {
        let errors = Rc::new(Errors::new());
        let chain = Chain::new(vec![
            Box::new(errors.clone()),
            Box::new(Backend { mode: Mode::Fail }),
        ]);
        let reply = chain.handle(&q(RecordType::A));
        assert_eq!(reply.header.rcode, Rcode::ServFail);
        assert_eq!(errors.count(), 1);
    }

    #[test]
    fn errors_plugin_passes_successful_replies_through() {
        let errors = Rc::new(Errors::new());
        let chain = Chain::new(vec![
            Box::new(errors.clone()),
            Box::new(Backend { mode: Mode::Ok }),
        ]);
        let reply = chain.handle(&q(RecordType::A));
        assert_eq!(reply.answers.len(), 1);
        assert_eq!(errors.count(), 0);
    }

    #[test]
    fn ready_gates_the_chain_until_ready() {
        let ready = Rc::new(Ready::new());
        let chain = Chain::new(vec![
            Box::new(ready.clone()),
            Box::new(Backend { mode: Mode::Ok }),
        ]);
        // Not ready → SERVFAIL, backend not consulted.
        assert_eq!(chain.handle(&q(RecordType::A)).header.rcode, Rcode::ServFail);
        ready.set_ready(true);
        assert_eq!(chain.handle(&q(RecordType::A)).answers.len(), 1);
    }

    #[test]
    fn log_line_formats_the_exchange() {
        let query = q(RecordType::A);
        let mut reply = query.reply();
        reply.answers.push(ResourceRecord::new(
            Name::parse("a.example.com").unwrap(),
            Class::In,
            30,
            Rdata::A(Ipv4Addr::new(1, 2, 3, 4)),
        ));
        let line = format_log_line(&query, &reply);
        assert!(line.contains("a.example.com."));
        assert!(line.contains("A"));
        assert!(line.contains("NOERROR"));
        assert!(line.contains("1"), "answer count should appear");
    }
}
