// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `file` plugin: authoritative answers from a zone.

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
            .with_record(ResourceRecord::new(n("example.com"), Class::In, 3600, Rdata::Ns(n("ns1.example.com"))))
            .with_record(a("ns1.example.com", [192, 0, 2, 1]))
            .with_record(a("www.example.com", [192, 0, 2, 10]))
            .with_record(ResourceRecord::new(n("www.example.com"), Class::In, 3600, Rdata::Aaaa(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 10))))
            .with_record(ResourceRecord::new(n("alias.example.com"), Class::In, 3600, Rdata::Cname(n("www.example.com"))))
            .with_record(ResourceRecord::new(n("ext.example.com"), Class::In, 3600, Rdata::Cname(n("elsewhere.net"))))
            .with_record(a("*.wild.example.com", [192, 0, 2, 99]))
            // Delegation of sub.example.com with in-zone glue.
            .with_record(ResourceRecord::new(n("sub.example.com"), Class::In, 3600, Rdata::Ns(n("ns.sub.example.com"))))
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
            zone().lookup(&n("example.com"), RecordType::Soa).unwrap().answers[0].rdata,
            Rdata::Soa { .. }
        ));
        assert!(matches!(
            zone().lookup(&n("example.com"), RecordType::Ns).unwrap().answers[0].rdata,
            Rdata::Ns(_)
        ));
    }

    #[test]
    fn nodata_for_existing_name_wrong_type() {
        // www exists (A/AAAA) but has no MX → NOERROR, empty answer, SOA in authority.
        let r = zone().lookup(&n("www.example.com"), RecordType::Mx).unwrap();
        assert_eq!(r.rcode, Rcode::NoError);
        assert!(r.answers.is_empty());
        assert!(matches!(r.authority[0].rdata, Rdata::Soa { .. }));
    }

    #[test]
    fn nxdomain_for_absent_name() {
        let r = zone().lookup(&n("ghost.example.com"), RecordType::A).unwrap();
        assert_eq!(r.rcode, Rcode::NxDomain);
        assert!(matches!(r.authority[0].rdata, Rdata::Soa { .. }));
    }

    #[test]
    fn cname_is_chased_within_the_zone() {
        let r = zone().lookup(&n("alias.example.com"), RecordType::A).unwrap();
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
        let r = zone().lookup(&n("anything.wild.example.com"), RecordType::A).unwrap();
        assert_eq!(r.answers.len(), 1);
        // The synthesized record is owned by the queried name.
        assert_eq!(r.answers[0].name, n("anything.wild.example.com"));
        assert_eq!(r.answers[0].rdata, Rdata::A(Ipv4Addr::new(192, 0, 2, 99)));
    }

    #[test]
    fn delegation_returns_a_referral_with_glue() {
        let r = zone().lookup(&n("host.sub.example.com"), RecordType::A).unwrap();
        assert_eq!(r.rcode, Rcode::NoError);
        assert!(!r.aa, "a referral is not authoritative");
        assert!(matches!(r.authority[0].rdata, Rdata::Ns(_)));
        // Glue address for the in-zone nameserver is in additional.
        assert!(r.additional.iter().any(|rr| matches!(rr.rdata, Rdata::A(_))));
    }

    #[test]
    fn out_of_zone_is_not_authoritative() {
        assert!(zone().lookup(&n("www.example.org"), RecordType::A).is_none());
    }

    #[test]
    fn plugin_defers_out_of_zone_and_answers_in_zone() {
        let chain = Chain::new(vec![Box::new(FilePlugin::new(zone())), Box::new(Sentinel)]);
        assert_eq!(
            chain.handle(&Message::query(n("www.example.com"), RecordType::A, 1)).answers.len(),
            1
        );
        assert_eq!(
            chain.handle(&Message::query(n("foo.example.org"), RecordType::A, 1)).header.rcode,
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
        assert!(matches!(mx.answers[0].rdata, Rdata::Mx { preference: 10, .. }));
        assert!(matches!(
            z.lookup(&n("example.com"), RecordType::Soa).unwrap().answers[0].rdata,
            Rdata::Soa { serial: 2026060701, .. }
        ));
    }
}
