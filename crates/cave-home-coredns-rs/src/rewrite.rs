// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `rewrite` plugin: query name / type / class rewriting.

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
