// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `rewrite` plugin: query name / type / class rewriting.
//!
//! Rules run in order against the incoming query. A matching rule mutates the
//! query and then, by default (`stop`), ends rule processing; `continue` lets
//! later rules see the mutated query, forming a chain (`CoreDNS` `rewrite`
//! docs). The rewritten query is handed to the rest of the chain; on the way
//! back, the original question is restored and any answer owned by the
//! rewritten name is renamed to what the client actually asked for — so the
//! client always sees a coherent answer.
//!
//! Name matching is case-insensitive and operates on the fully-qualified
//! presentation form (trailing dot included). Regex name rules and `edns0`
//! rules are deferred (see `parity.manifest.toml`).

use crate::message::Message;
use crate::name::Name;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::{Class, RecordType};

/// What happens after a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Apply the rule and keep evaluating later rules.
    Continue,
    /// Apply the rule and stop (the `CoreDNS` default).
    Stop,
}

/// How a name rule matches the query name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRule {
    /// The whole name equals `from`.
    Exact,
    /// The name begins with `from`.
    Prefix,
    /// The name ends with `from`.
    Suffix,
    /// The name contains `from`.
    Substring,
}

/// The field a rule rewrites.
#[derive(Debug, Clone)]
enum Kind {
    Name { mode: NameRule, from: String, to: String },
    Type { from: RecordType, to: RecordType },
    Class { from: Class, to: Class },
}

/// A single rewrite rule.
#[derive(Debug, Clone)]
pub struct Rule {
    policy: Policy,
    kind: Kind,
}

impl Rule {
    /// A name-rewrite rule (default policy `stop`).
    #[must_use]
    pub fn name(mode: NameRule, from: &str, to: &str) -> Self {
        Self {
            policy: Policy::Stop,
            kind: Kind::Name { mode, from: from.to_ascii_lowercase(), to: to.to_ascii_lowercase() },
        }
    }

    /// A type-rewrite rule (default policy `stop`).
    #[must_use]
    pub const fn rtype(from: RecordType, to: RecordType) -> Self {
        Self { policy: Policy::Stop, kind: Kind::Type { from, to } }
    }

    /// A class-rewrite rule (default policy `stop`).
    #[must_use]
    pub const fn class(from: Class, to: Class) -> Self {
        Self { policy: Policy::Stop, kind: Kind::Class { from, to } }
    }

    /// Switch this rule to the `continue` policy.
    #[must_use]
    pub const fn continuing(mut self) -> Self {
        self.policy = Policy::Continue;
        self
    }

    /// Apply this rule to the first question of `m`, returning `true` if it
    /// matched (and mutated the query).
    fn apply(&self, m: &mut Message) -> bool {
        let Some(q) = m.questions.first_mut() else { return false };
        match &self.kind {
            Kind::Name { mode, from, to } => {
                let Some(new) = rewrite_name(&q.name, *mode, from, to) else { return false };
                q.name = new;
                true
            }
            Kind::Type { from, to } => {
                if q.qtype == *from {
                    q.qtype = *to;
                    true
                } else {
                    false
                }
            }
            Kind::Class { from, to } => {
                if q.qclass == *from {
                    q.qclass = *to;
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Compute the rewritten name for a name rule, or `None` if it does not match
/// (or the result is not a valid name).
fn rewrite_name(name: &Name, mode: NameRule, from: &str, to: &str) -> Option<Name> {
    // Work on the lower-cased FQDN (trailing dot) so matching is case- and
    // qualification-insensitive.
    let fqdn = name.to_string().to_ascii_lowercase();
    let rewritten = match mode {
        NameRule::Exact => {
            let lhs = fqdn.trim_end_matches('.');
            let rhs = from.trim_end_matches('.');
            (lhs == rhs).then(|| to.to_string())?
        }
        NameRule::Prefix => {
            let rest = fqdn.strip_prefix(from)?;
            format!("{to}{rest}")
        }
        NameRule::Suffix => {
            // Compare against the dot-terminated name but match the suffix
            // ignoring the single trailing dot.
            let trimmed = fqdn.trim_end_matches('.');
            let rest = trimmed.strip_suffix(from)?;
            format!("{rest}{to}")
        }
        NameRule::Substring => {
            if !fqdn.contains(from) {
                return None;
            }
            fqdn.replace(from, to)
        }
    };
    Name::parse(&rewritten).ok()
}

/// The `rewrite` plugin: an ordered list of rules.
pub struct Rewriter {
    rules: Vec<Rule>,
}

impl Rewriter {
    /// Build a rewriter from rules in evaluation order.
    #[must_use]
    pub const fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Apply the rules to a query in place, returning the original name if the
    /// name was changed (so the response can be restored).
    #[must_use]
    pub fn rewrite_query(&self, m: &mut Message) -> Option<Name> {
        let original_name = m.questions.first().map(|q| q.name.clone());
        for rule in &self.rules {
            if rule.apply(m) && rule.policy == Policy::Stop {
                break;
            }
        }
        let new_name = m.questions.first().map(|q| q.name.clone());
        match (original_name, new_name) {
            (Some(o), Some(n)) if o != n => Some(o),
            _ => None,
        }
    }
}

impl Plugin for Rewriter {
    fn name(&self) -> &'static str {
        "rewrite"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        let original = req.query().clone();
        let mut rewritten = original.clone();
        let restored_name = self.rewrite_query(&mut rewritten);

        let mut resp = next.run(&Request::new(&rewritten))?;

        // Restore the question the client sent.
        resp.questions.clone_from(&original.questions);
        // Rename answers owned by the rewritten name back to the original.
        if let Some(orig) = restored_name {
            let rewritten_name = rewritten.questions.first().map(|q| q.name.clone());
            if let Some(rn) = rewritten_name {
                for rr in &mut resp.answers {
                    if rr.name == rn {
                        rr.name = orig.clone();
                    }
                }
            }
        }
        Ok(resp)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request};
    use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
    use std::net::Ipv4Addr;

    /// Downstream plugin: answers an A record owned by the name it *received*,
    /// and a root-owned TXT carrying that name so tests can see what reached it.
    struct Echo;
    impl Plugin for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            let seen = req.name().cloned().unwrap_or_else(Name::root);
            let mut reply = req.reply();
            reply.answers.push(ResourceRecord::new(
                seen.clone(),
                req.qclass().unwrap_or(Class::In),
                60,
                Rdata::A(Ipv4Addr::new(1, 2, 3, 4)),
            ));
            reply.answers.push(ResourceRecord::new(
                Name::root(),
                Class::In,
                60,
                Rdata::Txt(vec![seen.to_string().into_bytes()]),
            ));
            // Echo the type it received as a second TXT for type/class tests.
            reply.answers.push(ResourceRecord::new(
                Name::root(),
                Class::In,
                60,
                Rdata::Txt(vec![format!("{:?}", req.qtype()).into_bytes()]),
            ));
            Ok(reply)
        }
    }

    fn run(rules: Vec<Rule>, name: &str, t: RecordType) -> Message {
        let chain = Chain::new(vec![Box::new(Rewriter::new(rules)), Box::new(Echo)]);
        chain.handle(&Message::query(Name::parse(name).unwrap(), t, 1))
    }

    fn downstream_name(m: &Message) -> String {
        match &m.answers[1].rdata {
            Rdata::Txt(s) => String::from_utf8(s[0].clone()).unwrap(),
            _ => panic!("expected TXT"),
        }
    }

    #[test]
    fn exact_name_rewrite_and_response_restore() {
        let m = run(
            vec![Rule::name(NameRule::Exact, "old.example.com", "new.example.com")],
            "old.example.com",
            RecordType::A,
        );
        // Downstream saw the rewritten name.
        assert_eq!(downstream_name(&m), "new.example.com.");
        // The client's answer owner is restored to what it asked for.
        assert_eq!(m.answers[0].name, Name::parse("old.example.com").unwrap());
        // And the question is restored too.
        assert_eq!(m.questions[0].name, Name::parse("old.example.com").unwrap());
    }

    #[test]
    fn suffix_name_rewrite() {
        let m = run(
            vec![Rule::name(NameRule::Suffix, ".example.internal", ".example.com")],
            "host.example.internal",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "host.example.com.");
    }

    #[test]
    fn prefix_name_rewrite() {
        let m = run(
            vec![Rule::name(NameRule::Prefix, "dev.", "prod.")],
            "dev.example.com",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "prod.example.com.");
    }

    #[test]
    fn substring_name_rewrite() {
        let m = run(
            vec![Rule::name(NameRule::Substring, "-staging", "")],
            "api-staging.example.com",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "api.example.com.");
    }

    #[test]
    fn type_rewrite() {
        let m = run(
            vec![Rule::rtype(RecordType::Any, RecordType::A)],
            "x.example.com",
            RecordType::Any,
        );
        // The downstream type (3rd answer TXT) reflects the rewrite to A.
        let seen_type = match &m.answers[2].rdata {
            Rdata::Txt(s) => String::from_utf8(s[0].clone()).unwrap(),
            _ => panic!(),
        };
        assert_eq!(seen_type, "Some(A)");
    }

    #[test]
    fn no_rule_matches_passes_through_unchanged() {
        let m = run(
            vec![Rule::name(NameRule::Exact, "other.example.com", "x")],
            "host.example.com",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "host.example.com.");
    }

    #[test]
    fn stop_policy_applies_only_the_first_matching_rule() {
        let m = run(
            vec![
                Rule::name(NameRule::Suffix, ".a", ".b"), // stop (default)
                Rule::name(NameRule::Suffix, ".b", ".c"),
            ],
            "host.a",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "host.b.");
    }

    #[test]
    fn continue_policy_chains_rules() {
        let m = run(
            vec![
                Rule::name(NameRule::Suffix, ".a", ".b").continuing(),
                Rule::name(NameRule::Suffix, ".b", ".c"),
            ],
            "host.a",
            RecordType::A,
        );
        assert_eq!(downstream_name(&m), "host.c.");
    }
}
