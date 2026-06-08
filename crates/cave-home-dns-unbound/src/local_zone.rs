//! Local-zone resolution decision — the heart of this crate.
//!
//! First-party from Unbound's *public* `local-zone` / `local-data`
//! documentation (the `unbound.conf(5)` man page describing the local-zone
//! types). The BSD source was not copied; the documented per-type behaviour is
//! re-expressed in Rust.
//!
//! A [`LocalZone`] owns a zone apex name, a [`LocalZoneType`] and a bag of
//! [`Record`] local-data. Given a query (name + type), [`LocalZone::decide`]
//! returns a [`LocalDecision`]: answer locally with records, return an empty
//! local answer, NXDOMAIN, refuse, or fall through to recursion. The semantics
//! mirror the documented types:
//!
//! - **`Static`** — the zone is authoritative for these names *only*. A
//!   matching name+type is answered; a matching name with no matching type
//!   yields a (NODATA-style) local answer; a name in the zone with no data at
//!   all is NXDOMAIN. Nothing falls through to recursion.
//! - **`Transparent`** — local data is answered if present; anything not in the
//!   local data falls through to recursion (the name is *not* shadowed).
//! - **`Redirect`** — the zone's data answers for the apex *and every name
//!   under it*; the queried name is redirected to the apex data.
//! - **`Refuse`** — the resolver refuses to answer names in the zone (REFUSED).
//! - **`Deny`** — queries in the zone are dropped (no response at all).
//! - **`AlwaysNxdomain`** — every name in the zone is reported as
//!   non-existent (NXDOMAIN), regardless of any data.

use crate::name::DnsName;
use crate::record::{Record, RecordType};

/// The documented Unbound local-zone types cave-home supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalZoneType {
    /// Authoritative for listed names only; never recurse.
    Static,
    /// Answer local data if present, otherwise recurse.
    Transparent,
    /// Redirect the whole zone to the apex's data.
    Redirect,
    /// Refuse (REFUSED) names in the zone.
    Refuse,
    /// Silently drop queries in the zone.
    Deny,
    /// Always report names in the zone as non-existent.
    AlwaysNxdomain,
}

/// The outcome of a local-zone lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalDecision {
    /// Answer locally with these records (≥ 1).
    Answer(Vec<Record>),
    /// The name exists locally but has no record of the queried type
    /// (NODATA-style positive empty answer).
    NoData,
    /// The name does not exist (NXDOMAIN).
    NxDomain,
    /// Refuse the query (REFUSED — the client may not ask this).
    Refuse,
    /// Drop the query with no response.
    Drop,
    /// Not handled locally — fall through to recursion / forwarding.
    Passthrough,
}

impl LocalDecision {
    /// Did the zone produce a concrete local answer?
    #[must_use]
    pub const fn is_answer(&self) -> bool {
        matches!(self, Self::Answer(_))
    }
}

/// A configured local zone.
#[derive(Debug, Clone)]
pub struct LocalZone {
    apex: DnsName,
    ztype: LocalZoneType,
    data: Vec<Record>,
}

impl LocalZone {
    /// Create an empty zone of the given type.
    #[must_use]
    pub const fn new(apex: DnsName, ztype: LocalZoneType) -> Self {
        Self {
            apex,
            ztype,
            data: Vec::new(),
        }
    }

    /// Add a local-data record to the zone.
    pub fn add(&mut self, record: Record) {
        self.data.push(record);
    }

    /// The zone apex (its name).
    #[must_use]
    pub const fn apex(&self) -> &DnsName {
        &self.apex
    }

    /// The zone's type.
    #[must_use]
    pub const fn zone_type(&self) -> LocalZoneType {
        self.ztype
    }

    /// Does this zone cover `name`?
    #[must_use]
    pub fn covers(&self, name: &DnsName) -> bool {
        name.is_within(&self.apex)
    }

    fn records_for(&self, name: &DnsName, rtype: RecordType) -> Vec<Record> {
        self.data
            .iter()
            .filter(|r| r.name == *name && r.rtype == rtype)
            .cloned()
            .collect()
    }

    fn name_exists(&self, name: &DnsName) -> bool {
        self.data.iter().any(|r| r.name == *name)
    }

    /// Decide how to handle a query for `name` of type `rtype`.
    ///
    /// The caller is expected to have selected this zone as the most specific
    /// one covering `name`; [`covers`](Self::covers) can confirm. If the zone
    /// does not cover the name, the decision is always
    /// [`LocalDecision::Passthrough`].
    #[must_use]
    pub fn decide(&self, name: &DnsName, rtype: RecordType) -> LocalDecision {
        if !self.covers(name) {
            return LocalDecision::Passthrough;
        }
        match self.ztype {
            LocalZoneType::Refuse => LocalDecision::Refuse,
            LocalZoneType::Deny => LocalDecision::Drop,
            LocalZoneType::AlwaysNxdomain => LocalDecision::NxDomain,
            LocalZoneType::Transparent => {
                let recs = self.records_for(name, rtype);
                if recs.is_empty() {
                    // Not shadowed: let recursion try.
                    LocalDecision::Passthrough
                } else {
                    LocalDecision::Answer(recs)
                }
            }
            LocalZoneType::Static => {
                let recs = self.records_for(name, rtype);
                if !recs.is_empty() {
                    LocalDecision::Answer(recs)
                } else if self.name_exists(name) {
                    // Name is present with other types: NODATA, not NXDOMAIN.
                    LocalDecision::NoData
                } else {
                    // Authoritative and the name is absent.
                    LocalDecision::NxDomain
                }
            }
            LocalZoneType::Redirect => {
                // Every name in the zone is answered from the apex's data.
                let recs = self.records_for(&self.apex, rtype);
                if recs.is_empty() {
                    if self.name_exists(&self.apex) {
                        LocalDecision::NoData
                    } else {
                        LocalDecision::NxDomain
                    }
                } else {
                    LocalDecision::Answer(recs)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::RecordType;

    fn zone(apex: &str, t: LocalZoneType) -> LocalZone {
        LocalZone::new(DnsName::parse(apex).expect("apex"), t)
    }

    fn name(n: &str) -> DnsName {
        DnsName::parse(n).expect("name")
    }

    fn a_record(n: &str, ip: &str) -> Record {
        Record::address(n, RecordType::A, ip).expect("record")
    }

    #[test]
    fn static_answers_matching_name_and_type() {
        let mut z = zone("home.arpa", LocalZoneType::Static);
        z.add(a_record("printer.home.arpa", "192.168.1.50"));
        match z.decide(&name("printer.home.arpa"), RecordType::A) {
            LocalDecision::Answer(recs) => {
                assert_eq!(recs.len(), 1);
                assert_eq!(recs[0].data.to_text(), "192.168.1.50");
            }
            other => panic!("expected Answer, got {other:?}"),
        }
    }

    #[test]
    fn static_name_present_wrong_type_is_nodata() {
        let mut z = zone("home.arpa", LocalZoneType::Static);
        z.add(a_record("printer.home.arpa", "192.168.1.50"));
        assert_eq!(
            z.decide(&name("printer.home.arpa"), RecordType::Aaaa),
            LocalDecision::NoData
        );
    }

    #[test]
    fn static_absent_name_is_nxdomain_never_recurses() {
        let mut z = zone("home.arpa", LocalZoneType::Static);
        z.add(a_record("printer.home.arpa", "192.168.1.50"));
        assert_eq!(
            z.decide(&name("toaster.home.arpa"), RecordType::A),
            LocalDecision::NxDomain
        );
    }

    #[test]
    fn transparent_answers_local_then_passes_through() {
        let mut z = zone("home.arpa", LocalZoneType::Transparent);
        z.add(a_record("nas.home.arpa", "192.168.1.10"));
        assert!(
            z.decide(&name("nas.home.arpa"), RecordType::A)
                .is_answer()
        );
        // No local data for this one -> recursion handles it.
        assert_eq!(
            z.decide(&name("unknown.home.arpa"), RecordType::A),
            LocalDecision::Passthrough
        );
    }

    #[test]
    fn redirect_sends_every_subname_to_apex_data() {
        let mut z = zone("ads.example", LocalZoneType::Redirect);
        z.add(a_record("ads.example", "0.0.0.0"));
        for q in ["ads.example", "tracker.ads.example", "a.b.ads.example"] {
            match z.decide(&name(q), RecordType::A) {
                LocalDecision::Answer(recs) => assert_eq!(recs[0].data.to_text(), "0.0.0.0"),
                other => panic!("{q}: expected redirect Answer, got {other:?}"),
            }
        }
    }

    #[test]
    fn refuse_deny_and_always_nxdomain() {
        assert_eq!(
            zone("blocked.test", LocalZoneType::Refuse)
                .decide(&name("x.blocked.test"), RecordType::A),
            LocalDecision::Refuse
        );
        assert_eq!(
            zone("blocked.test", LocalZoneType::Deny)
                .decide(&name("x.blocked.test"), RecordType::A),
            LocalDecision::Drop
        );
        assert_eq!(
            zone("gone.test", LocalZoneType::AlwaysNxdomain)
                .decide(&name("anything.gone.test"), RecordType::A),
            LocalDecision::NxDomain
        );
    }

    #[test]
    fn name_outside_zone_passes_through() {
        let z = zone("home.arpa", LocalZoneType::Static);
        assert_eq!(
            z.decide(&name("example.com"), RecordType::A),
            LocalDecision::Passthrough
        );
    }
}
