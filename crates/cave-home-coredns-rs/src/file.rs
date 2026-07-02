// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `file` plugin: authoritative answers from a zone.
//!
//! [`Zone::lookup`] implements the RFC 1034 §4.3.2 authoritative algorithm:
//! delegation referral (NS below the apex, with in-zone glue) → exact match →
//! `CNAME` chasing (followed within the zone) → wildcard synthesis →
//! `NXDOMAIN`. A name that exists with no record of the queried type is
//! `NODATA`; both `NODATA` and `NXDOMAIN` carry the apex `SOA` in the authority
//! section for negative caching. A name outside the zone yields `None` — the
//! plugin then defers to the rest of the chain.
//!
//! Zones can be built programmatically or parsed from a master file
//! ([`Zone::from_master`]) supporting `$ORIGIN` / `$TTL`, `@`, blank-owner
//! continuation, relative names, paren-spanned records and the common record
//! types. The on-disk file watch/reload is the deferred I/O shell
//! (`parity.manifest.toml`).

use crate::error::{Result, WireError};
use crate::name::Name;
use crate::plugin::{Next, Outcome, Plugin, Request};
use crate::rr::{Class, Rdata, RecordType, ResourceRecord};
use crate::wire::Rcode;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

/// The outcome of an authoritative zone lookup.
#[derive(Debug, Clone)]
pub struct ZoneReply {
    /// The response code.
    pub rcode: Rcode,
    /// Whether the answer is authoritative (false for a referral).
    pub aa: bool,
    /// The answer section.
    pub answers: Vec<ResourceRecord>,
    /// The authority section (NS referral, or SOA for negatives).
    pub authority: Vec<ResourceRecord>,
    /// The additional section (glue).
    pub additional: Vec<ResourceRecord>,
}

/// An authoritative zone: records grouped by owner name.
pub struct Zone {
    origin: Name,
    owners: HashMap<Name, Vec<ResourceRecord>>,
    soa: Option<ResourceRecord>,
}

impl Zone {
    /// An empty zone rooted at `origin`.
    #[must_use]
    pub fn new(origin: Name) -> Self {
        Self {
            origin,
            owners: HashMap::new(),
            soa: None,
        }
    }

    /// Add a record (builder style).
    #[must_use]
    pub fn with_record(mut self, rr: ResourceRecord) -> Self {
        self.add_record(rr);
        self
    }

    /// Add a record, tracking the apex SOA.
    pub fn add_record(&mut self, rr: ResourceRecord) {
        if matches!(rr.rdata, Rdata::Soa { .. }) && rr.name == self.origin {
            self.soa = Some(rr.clone());
        }
        self.owners.entry(rr.name.clone()).or_default().push(rr);
    }

    /// The records at `name` whose type matches `qtype` (ANY matches all).
    fn matching(&self, name: &Name, qtype: RecordType) -> Vec<ResourceRecord> {
        self.owners.get(name).map_or_else(Vec::new, |recs| {
            recs.iter()
                .filter(|r| qtype == RecordType::Any || r.rtype() == qtype)
                .cloned()
                .collect()
        })
    }

    /// The apex SOA wrapped as the single-element authority section.
    fn soa_authority(&self) -> Vec<ResourceRecord> {
        self.soa.clone().into_iter().collect()
    }

    /// Glue: the in-zone address records for a nameserver target.
    fn glue(&self, ns_target: &Name) -> Vec<ResourceRecord> {
        self.owners.get(ns_target).map_or_else(Vec::new, |recs| {
            recs.iter()
                .filter(|r| matches!(r.rdata, Rdata::A(_) | Rdata::Aaaa(_)))
                .cloned()
                .collect()
        })
    }

    /// Find a delegation point: the deepest owner with NS records that is a
    /// proper descendant of the apex and an ancestor-or-self of `qname`.
    fn delegation(&self, qname: &Name) -> Option<&Name> {
        self.owners
            .iter()
            .filter(|(owner, recs)| {
                **owner != self.origin
                    && qname.is_subdomain_of(owner)
                    && recs.iter().any(|r| matches!(r.rdata, Rdata::Ns(_)))
            })
            .map(|(owner, _)| owner)
            .max_by_key(|owner| owner.label_count())
    }

    /// The authoritative answer for `qname`/`qtype`, or `None` if out of zone.
    #[must_use]
    pub fn lookup(&self, qname: &Name, qtype: RecordType) -> Option<ZoneReply> {
        if !qname.is_subdomain_of(&self.origin) {
            return None;
        }
        Some(self.lookup_in_zone(qname, qtype, 0))
    }

    fn lookup_in_zone(&self, qname: &Name, qtype: RecordType, depth: u8) -> ZoneReply {
        // Delegation takes precedence over local data below the apex.
        if let Some(deleg) = self.delegation(qname) {
            let authority = self.matching(deleg, RecordType::Ns);
            let additional = authority
                .iter()
                .filter_map(|rr| match &rr.rdata {
                    Rdata::Ns(t) => Some(self.glue(t)),
                    _ => None,
                })
                .flatten()
                .collect();
            return ZoneReply {
                rcode: Rcode::NoError,
                aa: false,
                answers: Vec::new(),
                authority,
                additional,
            };
        }

        // Exact owner match.
        if self.owners.contains_key(qname) {
            let matched = self.matching(qname, qtype);
            if !matched.is_empty() {
                return Self::answer(matched);
            }
            // CNAME at the name (and the client didn't ask for CNAME) → chase.
            if qtype != RecordType::Cname {
                if let Some(cname) = self.cname_at(qname) {
                    return self.chase_cname(qname, &cname, qtype, depth);
                }
            }
            return self.negative(Rcode::NoError); // NODATA
        }

        // Wildcard synthesis at the closest enclosing level.
        if let Some(reply) = self.wildcard(qname, qtype) {
            return reply;
        }

        self.negative(Rcode::NxDomain)
    }

    const fn answer(answers: Vec<ResourceRecord>) -> ZoneReply {
        ZoneReply {
            rcode: Rcode::NoError,
            aa: true,
            answers,
            authority: Vec::new(),
            additional: Vec::new(),
        }
    }

    fn negative(&self, rcode: Rcode) -> ZoneReply {
        ZoneReply {
            rcode,
            aa: true,
            answers: Vec::new(),
            authority: self.soa_authority(),
            additional: Vec::new(),
        }
    }

    fn cname_at(&self, name: &Name) -> Option<Name> {
        self.owners.get(name)?.iter().find_map(|r| match &r.rdata {
            Rdata::Cname(t) => Some(t.clone()),
            _ => None,
        })
    }

    fn chase_cname(&self, owner: &Name, target: &Name, qtype: RecordType, depth: u8) -> ZoneReply {
        let cname_rr = ResourceRecord::new(
            owner.clone(),
            Class::In,
            self.owners
                .get(owner)
                .and_then(|r| r.iter().find(|x| matches!(x.rdata, Rdata::Cname(_))))
                .map_or(3600, |x| x.ttl),
            Rdata::Cname(target.clone()),
        );
        // Only follow the chain within this zone, and bound the depth.
        if depth < 8 && target.is_subdomain_of(&self.origin) {
            let mut chained = self.lookup_in_zone(target, qtype, depth + 1);
            let mut answers = vec![cname_rr];
            answers.append(&mut chained.answers);
            return ZoneReply {
                rcode: chained.rcode,
                aa: true,
                answers,
                authority: if chained.rcode == Rcode::NoError {
                    Vec::new()
                } else {
                    chained.authority
                },
                additional: Vec::new(),
            };
        }
        // Out-of-zone target: hand back the CNAME alone.
        Self::answer(vec![cname_rr])
    }

    fn wildcard(&self, qname: &Name, qtype: RecordType) -> Option<ZoneReply> {
        // Walk from the immediate parent up to the apex; the closest `*.anc`
        // with a matching record synthesises the answer.
        let mut anc = qname.parent();
        while let Some(parent) = anc {
            if !parent.is_subdomain_of(&self.origin) {
                break;
            }
            let mut labels = vec![b"*".to_vec()];
            labels.extend(parent.labels().iter().cloned());
            if let Ok(wc) = Name::from_labels(labels) {
                let matched = self.matching(&wc, qtype);
                if !matched.is_empty() {
                    // Re-own the synthesised records under the queried name.
                    let answers = matched
                        .into_iter()
                        .map(|mut rr| {
                            rr.name = qname.clone();
                            rr
                        })
                        .collect();
                    return Some(Self::answer(answers));
                }
                if self.owners.contains_key(&wc) {
                    // Wildcard node exists but not this type → NODATA.
                    return Some(self.negative(Rcode::NoError));
                }
            }
            anc = parent.parent();
        }
        None
    }
}

/// Parse a master-file record `$ORIGIN`/`$TTL`/owner/type/rdata into a record.
impl Zone {
    /// Parse a zone from RFC 1035 master-file text.
    ///
    /// # Errors
    /// [`WireError::Corefile`] (reused as a generic config error) on a line that
    /// cannot be parsed.
    pub fn from_master(origin: Name, text: &str) -> Result<Self> {
        let mut zone = Self::new(origin.clone());
        let mut current_origin = origin;
        let mut default_ttl: u32 = 3600;
        let mut last_owner: Option<Name> = None;

        for raw in logical_lines(text) {
            let line = strip_comment(&raw);
            if line.trim().is_empty() {
                continue;
            }
            if let Some(rest) = line.trim().strip_prefix("$ORIGIN") {
                current_origin = Name::parse(rest.trim())?;
                continue;
            }
            if let Some(rest) = line.trim().strip_prefix("$TTL") {
                default_ttl = rest
                    .trim()
                    .parse()
                    .map_err(|_| WireError::Corefile { reason: "bad $TTL" })?;
                continue;
            }

            let owner_in_line = !line.starts_with([' ', '\t']);
            let mut toks = line.split_whitespace().peekable();
            let owner = if owner_in_line {
                let tok = toks.next().ok_or(WireError::Corefile {
                    reason: "empty record",
                })?;
                to_name(tok, &current_origin)?
            } else {
                last_owner.clone().ok_or(WireError::Corefile {
                    reason: "blank owner with no prior record",
                })?
            };
            last_owner = Some(owner.clone());

            // Optional TTL, optional class, then the type.
            let mut ttl = default_ttl;
            if let Some(t) = toks.peek() {
                if let Ok(v) = t.parse::<u32>() {
                    ttl = v;
                    toks.next();
                }
            }
            if let Some(t) = toks.peek() {
                if matches!(t.to_ascii_uppercase().as_str(), "IN" | "CS" | "CH" | "HS") {
                    toks.next();
                }
            }
            let rtype = toks.next().ok_or(WireError::Corefile {
                reason: "missing type",
            })?;
            let rest: Vec<&str> = toks.collect();
            let rdata = parse_rdata(rtype, &rest, &current_origin)?;
            zone.add_record(ResourceRecord::new(owner, Class::In, ttl, rdata));
        }
        Ok(zone)
    }
}

/// The `file` plugin: one authoritative zone.
pub struct FilePlugin {
    zone: Zone,
}

impl FilePlugin {
    /// Serve `zone`.
    #[must_use]
    pub const fn new(zone: Zone) -> Self {
        Self { zone }
    }
}

impl Plugin for FilePlugin {
    fn name(&self) -> &'static str {
        "file"
    }

    fn serve_dns(&self, req: &Request<'_>, next: Next<'_>) -> Outcome {
        let Some(q) = req.question() else {
            return next.run(req);
        };
        match self.zone.lookup(&q.name, q.qtype) {
            None => next.run(req), // out of zone — not authoritative
            Some(z) => {
                let mut reply = req.reply().with_aa(z.aa).with_rcode(z.rcode);
                reply.answers = z.answers;
                reply.authority = z.authority;
                reply.additional = z.additional;
                Ok(reply)
            }
        }
    }
}

/// Join physical lines into logical records, honouring `( … )` continuations.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut buf = String::new();
    for line in text.lines() {
        let stripped = strip_comment(line);
        for ch in stripped.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        if buf.is_empty() {
            buf.push_str(line);
        } else {
            buf.push(' ');
            buf.push_str(stripped.trim());
        }
        if depth <= 0 {
            out.push(buf.replace(['(', ')'], " "));
            buf.clear();
            depth = 0;
        }
    }
    if !buf.is_empty() {
        out.push(buf.replace(['(', ')'], " "));
    }
    out
}

/// Strip a `;` comment that is not inside double quotes.
fn strip_comment(line: &str) -> String {
    let mut in_quote = false;
    let mut out = String::new();
    for ch in line.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                out.push(ch);
            }
            ';' if !in_quote => break,
            _ => out.push(ch),
        }
    }
    out
}

/// Resolve a master-file name token (`@`, absolute, or relative to origin).
fn to_name(tok: &str, origin: &Name) -> Result<Name> {
    if tok == "@" {
        return Ok(origin.clone());
    }
    if tok.ends_with('.') {
        return Name::parse(tok);
    }
    Name::parse(&format!("{}.{}", tok.trim_end_matches('.'), origin))
}

/// Parse the RDATA tokens for a record type.
fn parse_rdata(rtype: &str, rest: &[&str], origin: &Name) -> Result<Rdata> {
    let bad = || WireError::Corefile {
        reason: "bad rdata",
    };
    let parse_u32 = |s: &str| s.parse::<u32>().map_err(|_| bad());
    let parse_u16 = |s: &str| s.parse::<u16>().map_err(|_| bad());
    match rtype.to_ascii_uppercase().as_str() {
        "A" => Ok(Rdata::A(
            rest.first()
                .ok_or_else(bad)?
                .parse::<Ipv4Addr>()
                .map_err(|_| bad())?,
        )),
        "AAAA" => Ok(Rdata::Aaaa(
            rest.first()
                .ok_or_else(bad)?
                .parse::<Ipv6Addr>()
                .map_err(|_| bad())?,
        )),
        "NS" => Ok(Rdata::Ns(to_name(rest.first().ok_or_else(bad)?, origin)?)),
        "CNAME" => Ok(Rdata::Cname(to_name(
            rest.first().ok_or_else(bad)?,
            origin,
        )?)),
        "PTR" => Ok(Rdata::Ptr(to_name(rest.first().ok_or_else(bad)?, origin)?)),
        "MX" => Ok(Rdata::Mx {
            preference: parse_u16(rest.first().ok_or_else(bad)?)?,
            exchange: to_name(rest.get(1).ok_or_else(bad)?, origin)?,
        }),
        "TXT" => {
            let joined = rest.join(" ");
            Ok(Rdata::Txt(vec![
                joined.trim_matches('"').as_bytes().to_vec(),
            ]))
        }
        "SRV" => Ok(Rdata::Srv {
            priority: parse_u16(rest.first().ok_or_else(bad)?)?,
            weight: parse_u16(rest.get(1).ok_or_else(bad)?)?,
            port: parse_u16(rest.get(2).ok_or_else(bad)?)?,
            target: to_name(rest.get(3).ok_or_else(bad)?, origin)?,
        }),
        "SOA" => Ok(Rdata::Soa {
            mname: to_name(rest.first().ok_or_else(bad)?, origin)?,
            rname: to_name(rest.get(1).ok_or_else(bad)?, origin)?,
            serial: parse_u32(rest.get(2).ok_or_else(bad)?)?,
            refresh: parse_u32(rest.get(3).ok_or_else(bad)?)?,
            retry: parse_u32(rest.get(4).ok_or_else(bad)?)?,
            expire: parse_u32(rest.get(5).ok_or_else(bad)?)?,
            minimum: parse_u32(rest.get(6).ok_or_else(bad)?)?,
        }),
        _ => Err(WireError::Corefile {
            reason: "unsupported record type",
        }),
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
    use crate::wire::Rcode;
    use std::net::{Ipv4Addr, Ipv6Addr};

    struct Sentinel;
    impl Plugin for Sentinel {
        fn name(&self) -> &'static str {
            "sentinel"
        }
        fn serve_dns(&self, req: &Request<'_>, _next: Next<'_>) -> Outcome {
            Ok(req.reply().with_rcode(Rcode::Refused))
        }
    }

    fn n(s: &str) -> Name {
        Name::parse(s).unwrap()
    }

    fn a(name: &str, ip: [u8; 4]) -> ResourceRecord {
        ResourceRecord::new(n(name), Class::In, 300, Rdata::A(Ipv4Addr::from(ip)))
    }

    fn soa() -> ResourceRecord {
        ResourceRecord::new(
            n("example.com"),
            Class::In,
            3600,
            Rdata::Soa {
                mname: n("ns1.example.com"),
                rname: n("hostmaster.example.com"),
                serial: 1,
                refresh: 7200,
                retry: 3600,
                expire: 1_209_600,
                minimum: 300,
            },
        )
    }

    fn zone() -> Zone {
        Zone::new(n("example.com"))
            .with_record(soa())
            .with_record(ResourceRecord::new(
                n("example.com"),
                Class::In,
                3600,
                Rdata::Ns(n("ns1.example.com")),
            ))
            .with_record(a("ns1.example.com", [192, 0, 2, 1]))
            .with_record(a("www.example.com", [192, 0, 2, 10]))
            .with_record(ResourceRecord::new(
                n("www.example.com"),
                Class::In,
                3600,
                Rdata::Aaaa(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 10)),
            ))
            .with_record(ResourceRecord::new(
                n("alias.example.com"),
                Class::In,
                3600,
                Rdata::Cname(n("www.example.com")),
            ))
            .with_record(ResourceRecord::new(
                n("ext.example.com"),
                Class::In,
                3600,
                Rdata::Cname(n("elsewhere.net")),
            ))
            .with_record(a("*.wild.example.com", [192, 0, 2, 99]))
            // Delegation of sub.example.com with in-zone glue.
            .with_record(ResourceRecord::new(
                n("sub.example.com"),
                Class::In,
                3600,
                Rdata::Ns(n("ns.sub.example.com")),
            ))
            .with_record(a("ns.sub.example.com", [192, 0, 2, 53]))
    }

    #[test]
    fn exact_match_answers() {
        let r = zone().lookup(&n("www.example.com"), RecordType::A).unwrap();
        assert_eq!(r.rcode, Rcode::NoError);
        assert!(r.aa);
        assert_eq!(r.answers.len(), 1);
        assert_eq!(r.answers[0].rdata, Rdata::A(Ipv4Addr::new(192, 0, 2, 10)));
    }

    #[test]
    fn apex_soa_and_ns() {
        assert!(matches!(
            zone()
                .lookup(&n("example.com"), RecordType::Soa)
                .unwrap()
                .answers[0]
                .rdata,
            Rdata::Soa { .. }
        ));
        assert!(matches!(
            zone()
                .lookup(&n("example.com"), RecordType::Ns)
                .unwrap()
                .answers[0]
                .rdata,
            Rdata::Ns(_)
        ));
    }

    #[test]
    fn nodata_for_existing_name_wrong_type() {
        // www exists (A/AAAA) but has no MX → NOERROR, empty answer, SOA in authority.
        let r = zone()
            .lookup(&n("www.example.com"), RecordType::Mx)
            .unwrap();
        assert_eq!(r.rcode, Rcode::NoError);
        assert!(r.answers.is_empty());
        assert!(matches!(r.authority[0].rdata, Rdata::Soa { .. }));
    }

    #[test]
    fn nxdomain_for_absent_name() {
        let r = zone()
            .lookup(&n("ghost.example.com"), RecordType::A)
            .unwrap();
        assert_eq!(r.rcode, Rcode::NxDomain);
        assert!(matches!(r.authority[0].rdata, Rdata::Soa { .. }));
    }

    #[test]
    fn cname_is_chased_within_the_zone() {
        let r = zone()
            .lookup(&n("alias.example.com"), RecordType::A)
            .unwrap();
        // CNAME alias→www, then www's A.
        assert!(matches!(r.answers[0].rdata, Rdata::Cname(_)));
        assert!(r.answers.iter().any(|rr| matches!(rr.rdata, Rdata::A(_))));
    }

    #[test]
    fn cname_to_out_of_zone_returns_just_the_cname() {
        let r = zone().lookup(&n("ext.example.com"), RecordType::A).unwrap();
        assert_eq!(r.answers.len(), 1);
        assert_eq!(r.answers[0].rdata, Rdata::Cname(n("elsewhere.net")));
    }

    #[test]
    fn wildcard_synthesis() {
        let r = zone()
            .lookup(&n("anything.wild.example.com"), RecordType::A)
            .unwrap();
        assert_eq!(r.answers.len(), 1);
        // The synthesized record is owned by the queried name.
        assert_eq!(r.answers[0].name, n("anything.wild.example.com"));
        assert_eq!(r.answers[0].rdata, Rdata::A(Ipv4Addr::new(192, 0, 2, 99)));
    }

    #[test]
    fn delegation_returns_a_referral_with_glue() {
        let r = zone()
            .lookup(&n("host.sub.example.com"), RecordType::A)
            .unwrap();
        assert_eq!(r.rcode, Rcode::NoError);
        assert!(!r.aa, "a referral is not authoritative");
        assert!(matches!(r.authority[0].rdata, Rdata::Ns(_)));
        // Glue address for the in-zone nameserver is in additional.
        assert!(
            r.additional
                .iter()
                .any(|rr| matches!(rr.rdata, Rdata::A(_)))
        );
    }

    #[test]
    fn out_of_zone_is_not_authoritative() {
        assert!(
            zone()
                .lookup(&n("www.example.org"), RecordType::A)
                .is_none()
        );
    }

    #[test]
    fn plugin_defers_out_of_zone_and_answers_in_zone() {
        let chain = Chain::new(vec![Box::new(FilePlugin::new(zone())), Box::new(Sentinel)]);
        assert_eq!(
            chain
                .handle(&Message::query(n("www.example.com"), RecordType::A, 1))
                .answers
                .len(),
            1
        );
        assert_eq!(
            chain
                .handle(&Message::query(n("foo.example.org"), RecordType::A, 1))
                .header
                .rcode,
            Rcode::Refused
        );
    }

    #[test]
    fn master_file_parses_and_answers() {
        let text = "
$ORIGIN example.com.
$TTL 3600
@   IN SOA ns1.example.com. hostmaster.example.com. (
        2026060701 ; serial
        7200 3600 1209600 300 )
@        IN NS    ns1.example.com.
ns1      IN A     192.0.2.1
www      IN A     192.0.2.10
www      IN AAAA  2001:db8::10
mail     IN MX    10 mailhost.example.com.
        ";
        let z = Zone::from_master(n("example.com"), text).unwrap();
        let r = z.lookup(&n("www.example.com"), RecordType::A).unwrap();
        assert_eq!(r.answers[0].rdata, Rdata::A(Ipv4Addr::new(192, 0, 2, 10)));
        let mx = z.lookup(&n("mail.example.com"), RecordType::Mx).unwrap();
        assert!(matches!(
            mx.answers[0].rdata,
            Rdata::Mx { preference: 10, .. }
        ));
        assert!(matches!(
            z.lookup(&n("example.com"), RecordType::Soa)
                .unwrap()
                .answers[0]
                .rdata,
            Rdata::Soa {
                serial: 2026060701,
                ..
            }
        ));
    }
}
