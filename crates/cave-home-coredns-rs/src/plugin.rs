// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Caddy-style plugin chain.
//!
//! `CoreDNS` assembles a server from an ordered list of plugins. Each plugin
//! either answers the query itself or *defers* to the rest of the chain via
//! [`Next::run`] — the same `NextOrFailure` contract `CoreDNS` uses. Because a
//! plugin receives `next` as a value it can call, plugins compose as middleware
//! (`cache`, `forward`): act before `next`, call it, then post-process the
//! reply. A plugin that answers without calling `next` short-circuits the rest.
//! If the chain is exhausted (the last plugin defers), the server answers
//! `SERVFAIL`, exactly as `CoreDNS` does.

use crate::message::Message;
use crate::name::Name;
use crate::rr::{Class, RecordType};
use crate::wire::Rcode;

/// The view a plugin has of the incoming query.
///
/// `CoreDNS` calls this the request "state"; it wraps the query message and
/// exposes the question fields plugins branch on.
pub struct Request<'a> {
    query: &'a Message,
}

impl<'a> Request<'a> {
    /// Wrap a query message.
    #[must_use]
    pub const fn new(query: &'a Message) -> Self {
        Self { query }
    }

    /// The underlying query message.
    #[must_use]
    pub const fn query(&self) -> &Message {
        self.query
    }

    /// The first question, if any.
    #[must_use]
    pub fn question(&self) -> Option<&crate::message::Question> {
        self.query.questions.first()
    }

    /// The queried name.
    #[must_use]
    pub fn name(&self) -> Option<&Name> {
        self.question().map(|q| &q.name)
    }

    /// The queried type.
    #[must_use]
    pub fn qtype(&self) -> Option<RecordType> {
        self.question().map(|q| q.qtype)
    }

    /// The queried class.
    #[must_use]
    pub fn qclass(&self) -> Option<Class> {
        self.question().map(|q| q.qclass)
    }

    /// The query id.
    #[must_use]
    pub const fn id(&self) -> u16 {
        self.query.header.id
    }

    /// A response skeleton echoing this query (see [`Message::reply`]).
    #[must_use]
    pub fn reply(&self) -> Message {
        self.query.reply()
    }
}

/// A failure raised by a plugin or the chain. The server maps any of these to a
/// `SERVFAIL` response; they are never shown to the homeowner (Charter §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerError {
    /// A plugin deferred but there was no next plugin to handle the query.
    NoNextPlugin,
    /// A plugin's backend (upstream, store, …) failed; the tag is for logs.
    Backend(&'static str),
}

/// What a plugin returns: the reply it produced, or a [`ServerError`].
pub type Outcome = Result<Message, ServerError>;

/// A handle to the remainder of the plugin chain.
///
/// It is a cheap `Copy` view over the not-yet-run plugins; calling [`Next::run`]
/// invokes the next plugin with a `Next` over the rest.
#[derive(Clone, Copy)]
pub struct Next<'a> {
    rest: &'a [Box<dyn Plugin>],
}

impl Next<'_> {
    /// Invoke the next plugin in the chain.
    ///
    /// # Errors
    /// [`ServerError::NoNextPlugin`] if the chain is exhausted, or whatever the
    /// next plugin returns.
    pub fn run(self, req: &Request<'_>) -> Outcome {
        match self.rest.split_first() {
            Some((head, tail)) => head.serve_dns(req, Next { rest: tail }),
            None => Err(ServerError::NoNextPlugin),
        }
    }
}

/// A `CoreDNS`-style plugin: it either answers or defers to `next`.
pub trait Plugin {
    /// The plugin's directive name (as it appears in the `Corefile`).
    fn name(&self) -> &str;

    /// Handle the request, optionally deferring to the rest of the chain.
    ///
    /// # Errors
    /// Any [`ServerError`] the plugin or a downstream plugin raises.
    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome;
}

/// An assembled, ordered chain of plugins for one server block.
#[derive(Default)]
pub struct Chain {
    plugins: Vec<Box<dyn Plugin>>,
}

impl Chain {
    /// Build a chain from plugins in execution order.
    #[must_use]
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Self {
        Self { plugins }
    }

    /// The plugin names in execution order.
    #[must_use]
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Run the chain for a query, returning the reply to put on the wire.
    ///
    /// A [`ServerError`] (including an exhausted chain) becomes a `SERVFAIL`
    /// response that still echoes the query's id and question.
    #[must_use]
    pub fn handle(&self, query: &Message) -> Message {
        let req = Request::new(query);
        Next { rest: &self.plugins }
            .run(&req)
            .unwrap_or_else(|_| req.reply().with_rcode(Rcode::ServFail))
    }
}

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
