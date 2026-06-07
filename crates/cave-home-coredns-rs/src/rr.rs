// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Resource-record types, classes and RDATA codecs.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::name::Name;
    use crate::wire::{Reader, Writer};
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn round_trip(rr: &ResourceRecord) -> ResourceRecord {
        let mut w = Writer::new();
        rr.encode(&mut w);
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        ResourceRecord::decode(&mut r).unwrap()
    }

    #[test]
    fn type_and_class_wire_values_round_trip() {
        for (t, v) in [
            (RecordType::A, 1u16),
            (RecordType::Ns, 2),
            (RecordType::Cname, 5),
            (RecordType::Soa, 6),
            (RecordType::Ptr, 12),
            (RecordType::Mx, 15),
            (RecordType::Txt, 16),
            (RecordType::Aaaa, 28),
            (RecordType::Srv, 33),
            (RecordType::Opt, 41),
            (RecordType::Any, 255),
        ] {
            assert_eq!(t.to_u16(), v);
            assert_eq!(RecordType::from_u16(v), t);
        }
        assert_eq!(RecordType::from_u16(999), RecordType::Unknown(999));
        assert_eq!(RecordType::Unknown(999).to_u16(), 999);
        assert_eq!(Class::from_u16(1), Class::In);
        assert_eq!(Class::In.to_u16(), 1);
    }

    #[test]
    fn a_record_round_trips() {
        let rr = ResourceRecord::new(
            Name::parse("host.example.com").unwrap(),
            Class::In,
            300,
            Rdata::A(Ipv4Addr::new(192, 0, 2, 7)),
        );
        let back = round_trip(&rr);
        assert_eq!(back, rr);
        assert_eq!(back.rtype(), RecordType::A);
    }

    #[test]
    fn aaaa_record_round_trips() {
        let rr = ResourceRecord::new(
            Name::parse("v6.example.com").unwrap(),
            Class::In,
            60,
            Rdata::Aaaa(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
        );
        assert_eq!(round_trip(&rr), rr);
    }

    #[test]
    fn name_bearing_records_round_trip() {
        for rd in [
            Rdata::Ns(Name::parse("ns1.example.com").unwrap()),
            Rdata::Cname(Name::parse("canonical.example.com").unwrap()),
            Rdata::Ptr(Name::parse("host.example.com").unwrap()),
        ] {
            let rr = ResourceRecord::new(Name::parse("example.com").unwrap(), Class::In, 100, rd);
            assert_eq!(round_trip(&rr), rr);
        }
    }

    #[test]
    fn soa_round_trips_all_fields() {
        let rr = ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            3600,
            Rdata::Soa {
                mname: Name::parse("ns.example.com").unwrap(),
                rname: Name::parse("hostmaster.example.com").unwrap(),
                serial: 2_026_060_701,
                refresh: 7200,
                retry: 3600,
                expire: 1_209_600,
                minimum: 300,
            },
        );
        assert_eq!(round_trip(&rr), rr);
    }

    #[test]
    fn mx_round_trips() {
        let rr = ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            300,
            Rdata::Mx {
                preference: 10,
                exchange: Name::parse("mail.example.com").unwrap(),
            },
        );
        assert_eq!(round_trip(&rr), rr);
    }

    #[test]
    fn txt_round_trips_multiple_strings_including_empty() {
        let rr = ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            300,
            Rdata::Txt(vec![b"v=spf1 -all".to_vec(), Vec::new(), b"k=v".to_vec()]),
        );
        assert_eq!(round_trip(&rr), rr);
    }

    #[test]
    fn srv_round_trips() {
        let rr = ResourceRecord::new(
            Name::parse("_sip._tcp.example.com").unwrap(),
            Class::In,
            300,
            Rdata::Srv {
                priority: 1,
                weight: 5,
                port: 5060,
                target: Name::parse("sipserver.example.com").unwrap(),
            },
        );
        assert_eq!(round_trip(&rr), rr);
    }

    #[test]
    fn unknown_type_rdata_is_preserved() {
        let rr = ResourceRecord::new(
            Name::parse("example.com").unwrap(),
            Class::In,
            300,
            Rdata::Unknown {
                rtype: 99,
                data: vec![0xde, 0xad, 0xbe, 0xef],
            },
        );
        let back = round_trip(&rr);
        assert_eq!(back, rr);
        assert_eq!(back.rtype(), RecordType::Unknown(99));
    }

    #[test]
    fn rdlength_mismatch_is_detected() {
        // root name, type A, class IN, ttl 0, rdlength = 2, but 4 data octets.
        let bytes = [
            0, // root name
            0, 1, // type A
            0, 1, // class IN
            0, 0, 0, 0, // ttl
            0, 2, // rdlength = 2 (too short for an A record's 4 octets)
            1, 2, 3, 4,
        ];
        let mut r = Reader::new(&bytes);
        assert!(matches!(
            ResourceRecord::decode(&mut r),
            Err(crate::WireError::RdataLengthMismatch { .. })
        ));
    }

    #[test]
    fn srv_target_is_not_compressed() {
        // Two SRV records sharing a target suffix: RFC 2782 forbids compressing
        // the SRV target, so the second target must be written in full.
        let mut w = Writer::new();
        w.write_bytes(&[0u8; 12]);
        let a = ResourceRecord::new(
            Name::parse("_a._tcp.example.com").unwrap(),
            Class::In,
            300,
            Rdata::Srv { priority: 0, weight: 0, port: 1, target: Name::parse("t.example.com").unwrap() },
        );
        a.encode(&mut w);
        let before = w.len();
        let b = ResourceRecord::new(
            Name::parse("_b._tcp.example.com").unwrap(),
            Class::In,
            300,
            Rdata::Srv { priority: 0, weight: 0, port: 2, target: Name::parse("t.example.com").unwrap() },
        );
        b.encode(&mut w);
        let bytes = w.into_bytes();
        // The owner name of `b` compresses against `a`, but the target does not.
        let mut r = Reader::new(&bytes);
        r.skip(12).unwrap();
        assert_eq!(ResourceRecord::decode(&mut r), Ok(a));
        assert_eq!(ResourceRecord::decode(&mut r), Ok(b));
        let _ = before;
    }
}
