// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `hosts` plugin: a hostfile answered for A / AAAA / PTR.
//!
//! The plugin is authoritative for the names in its hostfile: a known name with
//! no record of the queried type returns `NODATA` (`NOERROR`, empty), while an
//! unknown name returns `NXDOMAIN` — unless `fallthrough` is configured, in
//! which case unknown names are passed to the rest of the chain (`CoreDNS`
//! `hosts` plugin docs). Lookups are case-insensitive because [`Name`] compares
//! that way.

use crate::arpa::from_arpa;
use crate::name::Name;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
use crate::wire::Rcode;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The default record TTL `CoreDNS` uses for hostfile answers.
const DEFAULT_TTL: u32 = 3600;

/// A parsed hostfile served as a plugin.
pub struct Hosts {
    v4: HashMap<Name, Vec<Ipv4Addr>>,
    v6: HashMap<Name, Vec<Ipv6Addr>>,
    ptr: HashMap<IpAddr, Vec<Name>>,
    fallthrough: bool,
    ttl: u32,
}

impl Hosts {
    /// Parse a hostfile (`IP name [name…]` lines; `#` comments and blank lines
    /// ignored).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut v4: HashMap<Name, Vec<Ipv4Addr>> = HashMap::new();
        let mut v6: HashMap<Name, Vec<Ipv6Addr>> = HashMap::new();
        let mut ptr: HashMap<IpAddr, Vec<Name>> = HashMap::new();

        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split_whitespace();
            let Some(ip_str) = fields.next() else {
                continue;
            };
            let Ok(ip) = ip_str.parse::<IpAddr>() else {
                continue;
            };
            for host in fields {
                let Ok(name) = Name::parse(host) else {
                    continue;
                };
                match ip {
                    IpAddr::V4(a) => v4.entry(name.clone()).or_default().push(a),
                    IpAddr::V6(a) => v6.entry(name.clone()).or_default().push(a),
                }
                ptr.entry(ip).or_default().push(name);
            }
        }
        Self {
            v4,
            v6,
            ptr,
            fallthrough: false,
            ttl: DEFAULT_TTL,
        }
    }

    /// Enable `fallthrough`: pass unknown names to the next plugin.
    #[must_use]
    pub const fn with_fallthrough(mut self, on: bool) -> Self {
        self.fallthrough = on;
        self
    }

    /// Override the answer TTL.
    #[must_use]
    pub const fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }

    /// The IPv4 addresses for a name (empty slice if none).
    #[must_use]
    pub fn lookup_a(&self, name: &Name) -> &[Ipv4Addr] {
        self.v4.get(name).map_or(&[], Vec::as_slice)
    }

    /// The IPv6 addresses for a name (empty slice if none).
    #[must_use]
    pub fn lookup_aaaa(&self, name: &Name) -> &[Ipv6Addr] {
        self.v6.get(name).map_or(&[], Vec::as_slice)
    }

    /// The names mapped to an address (empty slice if none).
    #[must_use]
    pub fn lookup_ptr(&self, ip: IpAddr) -> &[Name] {
        self.ptr.get(&ip).map_or(&[], Vec::as_slice)
    }

    /// Whether the hostfile mentions this name at all.
    #[must_use]
    pub fn knows(&self, name: &Name) -> bool {
        self.v4.contains_key(name) || self.v6.contains_key(name)
    }

    /// Build the authoritative reply for `req` from already-found `answers`, or
    /// the NODATA/NXDOMAIN/fallthrough decision when nothing matched.
    fn respond(
        &self,
        req: &Request<'_>,
        next: Next<'_>,
        answers: Vec<ResourceRecord>,
        name_known: bool,
    ) -> Outcome {
        if !answers.is_empty() {
            let mut reply = req.reply().with_aa(true);
            reply.answers = answers;
            return Ok(reply);
        }
        if name_known {
            // Known name, no record of this type → NODATA.
            return Ok(req.reply().with_aa(true));
        }
        if self.fallthrough {
            return next.run(req);
        }
        Ok(req.reply().with_aa(true).with_rcode(Rcode::NxDomain))
    }
}

impl Plugin for Hosts {
    fn name(&self) -> &'static str {
        "hosts"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        let Some(q) = req.question() else {
            return next.run(req);
        };
        let owner = q.name.clone();
        match q.qtype {
            RecordType::A => {
                let answers = self
                    .lookup_a(&owner)
                    .iter()
                    .map(|ip| {
                        ResourceRecord::new(owner.clone(), Class::In, self.ttl, Rdata::A(*ip))
                    })
                    .collect();
                self.respond(req, next, answers, self.knows(&owner))
            }
            RecordType::Aaaa => {
                let answers = self
                    .lookup_aaaa(&owner)
                    .iter()
                    .map(|ip| {
                        ResourceRecord::new(owner.clone(), Class::In, self.ttl, Rdata::Aaaa(*ip))
                    })
                    .collect();
                self.respond(req, next, answers, self.knows(&owner))
            }
            RecordType::Ptr => {
                let targets = from_arpa(&owner).map_or(&[][..], |ip| self.lookup_ptr(ip));
                let answers = targets
                    .iter()
                    .map(|n| {
                        ResourceRecord::new(
                            owner.clone(),
                            Class::In,
                            self.ttl,
                            Rdata::Ptr(n.clone()),
                        )
                    })
                    .collect();
                // A reverse name is never "known" as a forward name, so the
                // empty-answer case is NXDOMAIN (or fallthrough), not NODATA.
                self.respond(req, next, answers, false)
            }
            _ => self.respond(req, next, Vec::new(), self.knows(&owner)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::arpa::to_arpa;
    use crate::message::Message;
    use crate::name::Name;
    use crate::plugin::{Chain, Next, Outcome, Plugin, Request};
    use crate::rr::{Rdata, RecordType};
    use crate::wire::Rcode;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    /// A terminal fallback plugin that always answers REFUSED-as-marker so we
    /// can detect that the chain fell through to it.
    struct Sentinel;
    impl Plugin for Sentinel {
        fn name(&self) -> &'static str {
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
        assert_eq!(
            ask(&chain, "nope.example.com", RecordType::A).header.rcode,
            Rcode::Refused
        );
        // Known name is still answered by hosts, not the sentinel.
        assert_eq!(
            ask(&chain, "host.example.com", RecordType::A).header.rcode,
            Rcode::NoError
        );
    }

    #[test]
    fn ttl_is_configurable() {
        let chain = Chain::new(vec![Box::new(hosts().with_ttl(42))]);
        let m = ask(&chain, "host.example.com", RecordType::A);
        assert_eq!(m.answers[0].ttl, 42);
    }
}
