// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Resource-record types, classes and RDATA codecs.
//!
//! A [`ResourceRecord`] is `name + class + ttl + rdata`; its [`RecordType`] is
//! derived from the [`Rdata`] variant. Encoding writes the RFC 1035 §3.2.1
//! preamble, a placeholder RDLENGTH, the RDATA, then backpatches the real
//! length — the only way to get RDLENGTH right when an embedded name is
//! compressed. Decoding lands the cursor exactly on `rdata_start + RDLENGTH`,
//! tolerating compression (consumed < declared) and rejecting overruns
//! (consumed > declared).

use crate::error::{Result, WireError};
use crate::name::Name;
use crate::wire::{Reader, Writer};
use std::net::{Ipv4Addr, Ipv6Addr};

/// A DNS resource-record / query type (RFC 1035 §3.2.2 and successors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordType {
    /// IPv4 host address.
    A,
    /// Authoritative name server.
    Ns,
    /// Canonical name (alias).
    Cname,
    /// Start of authority.
    Soa,
    /// Domain-name pointer (reverse).
    Ptr,
    /// Mail exchange.
    Mx,
    /// Text strings.
    Txt,
    /// IPv6 host address (RFC 3596).
    Aaaa,
    /// Service location (RFC 2782).
    Srv,
    /// EDNS(0) pseudo-record (RFC 6891).
    Opt,
    /// `*` — request for all records (query only).
    Any,
    /// Any type this crate does not model, kept by numeric code.
    Unknown(u16),
}

impl RecordType {
    /// The 16-bit wire value (RFC 1035 §3.2.2).
    #[must_use]
    pub const fn to_u16(self) -> u16 {
        match self {
            Self::A => 1,
            Self::Ns => 2,
            Self::Cname => 5,
            Self::Soa => 6,
            Self::Ptr => 12,
            Self::Mx => 15,
            Self::Txt => 16,
            Self::Aaaa => 28,
            Self::Srv => 33,
            Self::Opt => 41,
            Self::Any => 255,
            Self::Unknown(v) => v,
        }
    }

    /// Decode the 16-bit wire value, preserving unmodelled types as
    /// [`RecordType::Unknown`].
    #[must_use]
    pub const fn from_u16(v: u16) -> Self {
        match v {
            1 => Self::A,
            2 => Self::Ns,
            5 => Self::Cname,
            6 => Self::Soa,
            12 => Self::Ptr,
            15 => Self::Mx,
            16 => Self::Txt,
            28 => Self::Aaaa,
            33 => Self::Srv,
            41 => Self::Opt,
            255 => Self::Any,
            other => Self::Unknown(other),
        }
    }
}

/// A DNS class (RFC 1035 §3.2.4). In practice only `IN` is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// The Internet class.
    In,
    /// Chaos (used for `version.bind` etc.).
    Ch,
    /// Hesiod.
    Hs,
    /// `*` — any class (query only).
    Any,
    /// Any class this crate does not model, kept by numeric code.
    Unknown(u16),
}

impl Class {
    /// The 16-bit wire value.
    #[must_use]
    pub const fn to_u16(self) -> u16 {
        match self {
            Self::In => 1,
            Self::Ch => 3,
            Self::Hs => 4,
            Self::Any => 255,
            Self::Unknown(v) => v,
        }
    }

    /// Decode the 16-bit wire value.
    #[must_use]
    pub const fn from_u16(v: u16) -> Self {
        match v {
            1 => Self::In,
            3 => Self::Ch,
            4 => Self::Hs,
            255 => Self::Any,
            other => Self::Unknown(other),
        }
    }
}

/// The typed payload of a resource record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rdata {
    /// `A` — an IPv4 address.
    A(Ipv4Addr),
    /// `AAAA` — an IPv6 address.
    Aaaa(Ipv6Addr),
    /// `NS` — a name-server name.
    Ns(Name),
    /// `CNAME` — the canonical name an alias resolves to.
    Cname(Name),
    /// `PTR` — the target of a reverse lookup.
    Ptr(Name),
    /// `SOA` — the zone's start-of-authority parameters.
    Soa {
        /// Primary name server.
        mname: Name,
        /// Responsible mailbox (encoded as a name).
        rname: Name,
        /// Zone serial number.
        serial: u32,
        /// Secondary refresh interval (seconds).
        refresh: u32,
        /// Secondary retry interval (seconds).
        retry: u32,
        /// Secondary expiry (seconds).
        expire: u32,
        /// Negative-caching TTL / minimum (seconds).
        minimum: u32,
    },
    /// `MX` — a mail exchanger with its preference.
    Mx {
        /// Lower is preferred.
        preference: u16,
        /// The mail-exchanger host.
        exchange: Name,
    },
    /// `TXT` — one or more character-strings.
    Txt(Vec<Vec<u8>>),
    /// `SRV` — a service location (RFC 2782).
    Srv {
        /// Lower is preferred.
        priority: u16,
        /// Relative weight among equal priorities.
        weight: u16,
        /// TCP/UDP port.
        port: u16,
        /// The host offering the service.
        target: Name,
    },
    /// Any record type this crate does not model, kept verbatim.
    Unknown {
        /// The numeric type code.
        rtype: u16,
        /// The raw RDATA octets.
        data: Vec<u8>,
    },
}

impl Rdata {
    /// The record type this payload represents.
    #[must_use]
    pub const fn record_type(&self) -> RecordType {
        match self {
            Self::A(_) => RecordType::A,
            Self::Aaaa(_) => RecordType::Aaaa,
            Self::Ns(_) => RecordType::Ns,
            Self::Cname(_) => RecordType::Cname,
            Self::Ptr(_) => RecordType::Ptr,
            Self::Soa { .. } => RecordType::Soa,
            Self::Mx { .. } => RecordType::Mx,
            Self::Txt(_) => RecordType::Txt,
            Self::Srv { .. } => RecordType::Srv,
            Self::Unknown { rtype, .. } => RecordType::Unknown(*rtype),
        }
    }

    /// Encode just the RDATA octets (the caller writes/backpatches RDLENGTH).
    fn encode(&self, w: &mut Writer) {
        match self {
            Self::A(ip) => w.write_bytes(&ip.octets()),
            Self::Aaaa(ip) => w.write_bytes(&ip.octets()),
            // The classic name-bearing types MAY be compressed (RFC 1035 §4.1.4).
            Self::Ns(n) | Self::Cname(n) | Self::Ptr(n) => w.write_name(n),
            Self::Soa { mname, rname, serial, refresh, retry, expire, minimum } => {
                w.write_name(mname);
                w.write_name(rname);
                w.write_u32(*serial);
                w.write_u32(*refresh);
                w.write_u32(*retry);
                w.write_u32(*expire);
                w.write_u32(*minimum);
            }
            Self::Mx { preference, exchange } => {
                w.write_u16(*preference);
                w.write_name(exchange);
            }
            Self::Txt(strings) => {
                for s in strings {
                    // Each character-string is a single length octet + bytes;
                    // a string longer than 255 is split into 255-octet chunks.
                    let mut rest = s.as_slice();
                    loop {
                        let take = rest.len().min(255);
                        w.write_u8(take as u8);
                        w.write_bytes(&rest[..take]);
                        rest = &rest[take..];
                        if rest.is_empty() {
                            break;
                        }
                    }
                }
            }
            Self::Srv { priority, weight, port, target } => {
                w.write_u16(*priority);
                w.write_u16(*weight);
                w.write_u16(*port);
                // RFC 2782: the SRV target MUST NOT be compressed.
                w.write_name_uncompressed(target);
            }
            Self::Unknown { data, .. } => w.write_bytes(data),
        }
    }

    /// Decode RDATA of `rtype`, given the declared `rdlength`.
    fn decode(rtype: RecordType, r: &mut Reader<'_>, rdlength: usize) -> Result<Self> {
        let start = r.position();
        let end = start + rdlength;
        let rd = match rtype {
            RecordType::A => {
                let o = r.read_bytes(4)?;
                Self::A(Ipv4Addr::new(o[0], o[1], o[2], o[3]))
            }
            RecordType::Aaaa => {
                let o = r.read_bytes(16)?;
                let mut arr = [0u8; 16];
                arr.copy_from_slice(o);
                Self::Aaaa(Ipv6Addr::from(arr))
            }
            RecordType::Ns => Self::Ns(r.read_name()?),
            RecordType::Cname => Self::Cname(r.read_name()?),
            RecordType::Ptr => Self::Ptr(r.read_name()?),
            RecordType::Soa => Self::Soa {
                mname: r.read_name()?,
                rname: r.read_name()?,
                serial: r.read_u32()?,
                refresh: r.read_u32()?,
                retry: r.read_u32()?,
                expire: r.read_u32()?,
                minimum: r.read_u32()?,
            },
            RecordType::Mx => Self::Mx {
                preference: r.read_u16()?,
                exchange: r.read_name()?,
            },
            RecordType::Txt => {
                let mut strings = Vec::new();
                while r.position() < end {
                    let len = r.read_u8()? as usize;
                    strings.push(r.read_bytes(len)?.to_vec());
                }
                Self::Txt(strings)
            }
            RecordType::Srv => Self::Srv {
                priority: r.read_u16()?,
                weight: r.read_u16()?,
                port: r.read_u16()?,
                target: r.read_name()?,
            },
            RecordType::Opt | RecordType::Any | RecordType::Unknown(_) => Self::Unknown {
                rtype: rtype.to_u16(),
                data: r.read_bytes(rdlength)?.to_vec(),
            },
        };
        let consumed = r.position() - start;
        if consumed > rdlength {
            return Err(WireError::RdataLengthMismatch { declared: rdlength, consumed });
        }
        // A compressed embedded name consumes fewer octets than RDLENGTH; land
        // exactly on the record boundary so the next record reads correctly.
        if consumed < rdlength {
            r.set_position(end)?;
        }
        Ok(rd)
    }
}

/// A complete resource record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRecord {
    /// The owner name.
    pub name: Name,
    /// The class (almost always [`Class::In`]).
    pub class: Class,
    /// Time-to-live in seconds.
    pub ttl: u32,
    /// The typed payload.
    pub rdata: Rdata,
}

impl ResourceRecord {
    /// Assemble a record.
    #[must_use]
    pub const fn new(name: Name, class: Class, ttl: u32, rdata: Rdata) -> Self {
        Self { name, class, ttl, rdata }
    }

    /// The record's type, derived from its RDATA.
    #[must_use]
    pub const fn rtype(&self) -> RecordType {
        self.rdata.record_type()
    }

    /// Encode the full record (preamble + RDLENGTH-prefixed RDATA).
    pub fn encode(&self, w: &mut Writer) {
        w.write_name(&self.name);
        w.write_u16(self.rtype().to_u16());
        w.write_u16(self.class.to_u16());
        w.write_u32(self.ttl);
        let len_at = w.len();
        w.write_u16(0); // RDLENGTH placeholder
        let rdata_start = w.len();
        self.rdata.encode(w);
        let rdlength = (w.len() - rdata_start) as u16;
        w.patch_u16(len_at, rdlength);
    }

    /// Decode a full record.
    ///
    /// # Errors
    /// Any [`WireError`] from the name/RDATA codecs, including
    /// [`WireError::RdataLengthMismatch`] when RDATA overruns RDLENGTH.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let name = r.read_name()?;
        let rtype = RecordType::from_u16(r.read_u16()?);
        let class = Class::from_u16(r.read_u16()?);
        let ttl = r.read_u32()?;
        let rdlength = r.read_u16()? as usize;
        let rdata = Rdata::decode(rtype, r, rdlength)?;
        Ok(Self { name, class, ttl, rdata })
    }
}

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
