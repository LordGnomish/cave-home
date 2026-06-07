// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Operational plugins: `metrics`/`prometheus`, `errors`, `ready`, `log`.

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
        chain.handle(&q(RecordType::A));
        chain.handle(&q(RecordType::A));
        chain.handle(&q(RecordType::Aaaa));
        let snap = metrics.snapshot();
        assert_eq!(snap.total, 3);
        assert_eq!(snap.queries(RecordType::A), 2);
        assert_eq!(snap.queries(RecordType::Aaaa), 1);
        assert_eq!(snap.responses(Rcode::NoError), 3);
    }

    #[test]
    fn metrics_record_servfail_for_downstream_errors() {
        let metrics = Rc::new(Metrics::new());
        let chain = Chain::new(vec![
            Box::new(metrics.clone()),
            Box::new(Backend { mode: Mode::Fail }),
        ]);
        chain.handle(&q(RecordType::A));
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
