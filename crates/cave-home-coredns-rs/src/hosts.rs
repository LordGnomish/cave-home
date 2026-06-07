// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `hosts` plugin: a hostfile answered for A / AAAA / PTR.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request};
    use crate::rr::{Rdata, RecordType};
    use crate::wire::Rcode;
    use crate::arpa::to_arpa;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    /// A terminal fallback plugin that always answers REFUSED-as-marker so we
    /// can detect that the chain fell through to it.
    struct Sentinel;
    impl Plugin for Sentinel {
        fn name(&self) -> &str {
            "sentinel"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            Ok(req.reply().with_rcode(Rcode::Refused))
        }
    }

    const FILE: &str = "
        # a comment line
        192.0.2.10   host.example.com   alias.example.com

        192.0.2.11   host.example.com
        2001:db8::1  host.example.com
    ";

    fn hosts() -> Hosts {
        Hosts::parse(FILE)
    }

    fn ask(chain: &Chain, name: &str, t: RecordType) -> Message {
        chain.handle(&Message::query(Name::parse(name).unwrap(), t, 1))
    }

    #[test]
    fn parses_addresses_and_aliases() {
        let h = hosts();
        assert_eq!(
            h.lookup_a(&Name::parse("host.example.com").unwrap()),
            &[Ipv4Addr::new(192, 0, 2, 10), Ipv4Addr::new(192, 0, 2, 11)]
        );
        assert_eq!(
            h.lookup_a(&Name::parse("alias.example.com").unwrap()),
            &[Ipv4Addr::new(192, 0, 2, 10)]
        );
        assert!(h.knows(&Name::parse("HOST.EXAMPLE.COM").unwrap()));
    }

    #[test]
    fn answers_a_records() {
        let chain = Chain::new(vec![Box::new(hosts())]);
        let m = ask(&chain, "host.example.com", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert!(m.header.aa);
        assert_eq!(m.answers.len(), 2);
        assert!(matches!(m.answers[0].rdata, Rdata::A(_)));
    }

    #[test]
    fn answers_aaaa_records() {
        let chain = Chain::new(vec![Box::new(hosts())]);
        let m = ask(&chain, "host.example.com", RecordType::Aaaa);
        assert_eq!(m.answers.len(), 1);
        assert_eq!(
            m.answers[0].rdata,
            Rdata::Aaaa(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn answers_reverse_ptr() {
        let chain = Chain::new(vec![Box::new(hosts())]);
        let rev = to_arpa(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)));
        let m = chain.handle(&Message::query(rev, RecordType::Ptr, 1));
        assert_eq!(m.header.rcode, Rcode::NoError);
        // Both names mapped to .10 are returned as PTR targets.
        assert_eq!(m.answers.len(), 2);
        assert!(m.answers.iter().all(|rr| matches!(rr.rdata, Rdata::Ptr(_))));
    }

    #[test]
    fn nodata_for_known_name_wrong_type() {
        let chain = Chain::new(vec![Box::new(hosts())]);
        // host has no MX; NODATA = NOERROR + empty + authoritative.
        let m = ask(&chain, "host.example.com", RecordType::Mx);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert!(m.answers.is_empty());
        assert!(m.header.aa);
    }

    #[test]
    fn nxdomain_for_unknown_name_without_fallthrough() {
        let chain = Chain::new(vec![Box::new(hosts())]);
        let m = ask(&chain, "nope.example.com", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NxDomain);
        assert!(m.header.aa);
    }

    #[test]
    fn fallthrough_passes_unknown_names_downstream() {
        let chain = Chain::new(vec![
            Box::new(hosts().with_fallthrough(true)),
            Box::new(Sentinel),
        ]);
        // Unknown name falls through to the sentinel.
        assert_eq!(ask(&chain, "nope.example.com", RecordType::A).header.rcode, Rcode::Refused);
        // Known name is still answered by hosts, not the sentinel.
        assert_eq!(ask(&chain, "host.example.com", RecordType::A).header.rcode, Rcode::NoError);
    }

    #[test]
    fn ttl_is_configurable() {
        let chain = Chain::new(vec![Box::new(hosts().with_ttl(42))]);
        let m = ask(&chain, "host.example.com", RecordType::A);
        assert_eq!(m.answers[0].ttl, 42);
    }
}
