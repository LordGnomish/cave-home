// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Caddy-style plugin chain.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use crate::wire::Rcode;
    use std::net::Ipv4Addr;

    /// A plugin that answers every query with a fixed A record (authoritative).
    struct StaticA(Ipv4Addr);
    impl Plugin for StaticA {
        fn name(&self) -> &str {
            "static_a"
        }
        fn serve_dns(&self, req: &Request, _next: Next<'_>) -> Outcome {
            let mut reply = req.reply();
            let owner = req.name().cloned().unwrap_or_else(Name::root);
            reply
                .answers
                .push(ResourceRecord::new(owner, Class::In, 300, Rdata::A(self.0)));
            Ok(reply.with_aa(true))
        }
    }

    /// A plugin that never answers; it always defers to the next plugin.
    struct AlwaysNext;
    impl Plugin for AlwaysNext {
        fn name(&self) -> &str {
            "always_next"
        }
        fn serve_dns(&self, req: &Request, next: Next<'_>) -> Outcome {
            next.run(req)
        }
    }

    /// Middleware: run the rest of the chain, then stamp the AD bit on the reply.
    struct SetAd;
    impl Plugin for SetAd {
        fn name(&self) -> &str {
            "set_ad"
        }
        fn serve_dns(&self, req: &Request, next: Next<'_>) -> Outcome {
            let mut reply = next.run(req)?;
            reply.header.ad = true;
            Ok(reply)
        }
    }

    /// A plugin that fails outright.
    struct Boom;
    impl Plugin for Boom {
        fn name(&self) -> &str {
            "boom"
        }
        fn serve_dns(&self, _req: &Request, _next: Next<'_>) -> Outcome {
            Err(ServerError::Backend("boom"))
        }
    }

    fn query() -> Message {
        Message::query(Name::parse("svc.example.com").unwrap(), RecordType::A, 1)
    }

    #[test]
    fn empty_chain_yields_servfail() {
        let chain = Chain::new(vec![]);
        let reply = chain.handle(&query());
        assert_eq!(reply.header.rcode, Rcode::ServFail);
        assert!(reply.header.qr);
    }

    #[test]
    fn a_responding_plugin_answers_and_stops() {
        let chain = Chain::new(vec![Box::new(StaticA(Ipv4Addr::new(10, 0, 0, 1)))]);
        let reply = chain.handle(&query());
        assert_eq!(reply.header.rcode, Rcode::NoError);
        assert!(reply.header.aa);
        assert_eq!(reply.answers.len(), 1);
    }

    #[test]
    fn fallthrough_reaches_a_later_plugin() {
        let chain = Chain::new(vec![
            Box::new(AlwaysNext),
            Box::new(AlwaysNext),
            Box::new(StaticA(Ipv4Addr::new(192, 0, 2, 9))),
        ]);
        let reply = chain.handle(&query());
        assert_eq!(reply.answers.len(), 1);
        assert!(matches!(reply.answers[0].rdata, Rdata::A(_)));
    }

    #[test]
    fn middleware_post_processes_the_downstream_reply() {
        let chain = Chain::new(vec![
            Box::new(SetAd),
            Box::new(StaticA(Ipv4Addr::new(10, 0, 0, 2))),
        ]);
        let reply = chain.handle(&query());
        assert!(reply.header.ad, "middleware should have set AD after next");
        assert_eq!(reply.answers.len(), 1);
    }

    #[test]
    fn an_earlier_responder_short_circuits_later_plugins() {
        // If StaticA answers first, Boom (which would error) must never run.
        let chain = Chain::new(vec![
            Box::new(StaticA(Ipv4Addr::new(10, 0, 0, 3))),
            Box::new(Boom),
        ]);
        let reply = chain.handle(&query());
        assert_eq!(reply.header.rcode, Rcode::NoError);
    }

    #[test]
    fn a_plugin_error_becomes_servfail() {
        let chain = Chain::new(vec![Box::new(Boom)]);
        assert_eq!(chain.handle(&query()).header.rcode, Rcode::ServFail);
    }

    #[test]
    fn chain_reports_its_plugin_names_in_order() {
        let chain = Chain::new(vec![
            Box::new(SetAd),
            Box::new(AlwaysNext),
            Box::new(StaticA(Ipv4Addr::new(1, 1, 1, 1))),
        ]);
        assert_eq!(chain.plugin_names(), vec!["set_ad", "always_next", "static_a"]);
    }

    #[test]
    fn request_exposes_question_fields() {
        let q = query();
        let req = Request::new(&q);
        assert_eq!(req.name(), Some(&Name::parse("svc.example.com").unwrap()));
        assert_eq!(req.qtype(), Some(RecordType::A));
        assert_eq!(req.qclass(), Some(Class::In));
        assert_eq!(req.id(), 1);
    }
}
