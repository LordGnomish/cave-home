//! Resource-record model: [`RecordType`] and a small [`RecordData`] value.
//!
//! First-party from the public DNS record-type registry (RFC 1035 and the
//! IANA DNS Parameters): A / AAAA address records, CNAME aliases, MX mail
//! exchangers, TXT text, PTR reverse pointers, SRV service locators, NS
//! delegation and SOA zone-apex records. Address parsing reuses the standard
//! library's [`std::net::IpAddr`] so we never hand-roll an IPv6 parser.

use crate::name::{DnsName, NameError};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// The DNS record types cave-home reasons about in its local zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordType {
    /// IPv4 address.
    A,
    /// IPv6 address.
    Aaaa,
    /// Canonical-name alias.
    Cname,
    /// Mail exchanger.
    Mx,
    /// Free-form text.
    Txt,
    /// Reverse pointer (IP → name).
    Ptr,
    /// Service locator.
    Srv,
    /// Name server (delegation).
    Ns,
    /// Start of authority (zone apex).
    Soa,
}

impl RecordType {
    /// The conventional all-caps mnemonic (`"A"`, `"AAAA"`, …). This is an
    /// operator/audit string, never shown to the household (Charter §6.3).
    #[must_use]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::Cname => "CNAME",
            Self::Mx => "MX",
            Self::Txt => "TXT",
            Self::Ptr => "PTR",
            Self::Srv => "SRV",
            Self::Ns => "NS",
            Self::Soa => "SOA",
        }
    }

    /// Parse a record-type mnemonic (case-insensitive). Returns `None` for an
    /// unknown type.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "A" => Some(Self::A),
            "AAAA" => Some(Self::Aaaa),
            "CNAME" => Some(Self::Cname),
            "MX" => Some(Self::Mx),
            "TXT" => Some(Self::Txt),
            "PTR" => Some(Self::Ptr),
            "SRV" => Some(Self::Srv),
            "NS" => Some(Self::Ns),
            "SOA" => Some(Self::Soa),
            _ => None,
        }
    }

    /// Is this the IPv4 ([`RecordType::A`]) type?
    #[must_use]
    pub const fn is_a(self) -> bool {
        matches!(self, Self::A)
    }
}

/// The decoded right-hand side of a local-data record.
///
/// Only the variants the local-zone engine needs to *answer* are modelled as
/// rich values; the rest carry their text form, which is sufficient for the
/// resolution-decision core (the wire encoder is Phase 1b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordData {
    /// An IPv4 or IPv6 address (A / AAAA).
    Addr(IpAddr),
    /// A target name (CNAME / NS / PTR).
    Name(DnsName),
    /// Free-form text or an opaque RDATA the engine passes through verbatim
    /// (TXT / MX / SRV / SOA).
    Text(String),
}

/// Why record data failed to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// The address text was not a valid IPv4/IPv6 literal.
    BadAddress,
    /// An A record was given an IPv6 literal, or AAAA an IPv4 literal.
    AddressFamilyMismatch,
    /// The target name was not a valid domain name.
    BadName(NameError),
}

impl RecordData {
    /// Parse an address literal for an A or AAAA record, enforcing the address
    /// family matches the record type.
    ///
    /// # Errors
    /// [`RecordError::BadAddress`] if `text` is not an IP literal, or
    /// [`RecordError::AddressFamilyMismatch`] if it is the wrong family for
    /// `rtype`.
    pub fn parse_addr(rtype: RecordType, text: &str) -> Result<Self, RecordError> {
        let addr = IpAddr::from_str(text.trim()).map_err(|_| RecordError::BadAddress)?;
        match (rtype, addr) {
            (RecordType::A, IpAddr::V4(_)) | (RecordType::Aaaa, IpAddr::V6(_)) => {
                Ok(Self::Addr(addr))
            }
            (RecordType::A, IpAddr::V6(_)) | (RecordType::Aaaa, IpAddr::V4(_)) => {
                Err(RecordError::AddressFamilyMismatch)
            }
            _ => Err(RecordError::BadAddress),
        }
    }

    /// Parse a target name for a CNAME / NS / PTR record.
    ///
    /// # Errors
    /// [`RecordError::BadName`] when the target is not a valid domain name.
    pub fn parse_name(text: &str) -> Result<Self, RecordError> {
        DnsName::parse(text)
            .map(Self::Name)
            .map_err(RecordError::BadName)
    }

    /// Render the data back to its canonical text form for an answer / audit
    /// line. Addresses use the standard-library formatter (compressed IPv6).
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Addr(ip) => ip.to_string(),
            Self::Name(n) => n.to_fqdn(),
            Self::Text(t) => t.clone(),
        }
    }
}

/// One local-data record: a name, its type, and its decoded value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Owner name (already normalised).
    pub name: DnsName,
    /// Record type.
    pub rtype: RecordType,
    /// Decoded RDATA.
    pub data: RecordData,
}

impl Record {
    /// Build a record from already-validated parts.
    #[must_use]
    pub const fn new(name: DnsName, rtype: RecordType, data: RecordData) -> Self {
        Self { name, rtype, data }
    }

    /// Convenience: build an address (A/AAAA) record from text inputs.
    ///
    /// # Errors
    /// Propagates name/address parse failures.
    pub fn address(name: &str, rtype: RecordType, addr: &str) -> Result<Self, RecordError> {
        let n = DnsName::parse(name).map_err(RecordError::BadName)?;
        let data = RecordData::parse_addr(rtype, addr)?;
        Ok(Self::new(n, rtype, data))
    }
}

/// Parse a dotted-quad / IPv6 literal into a standard [`IpAddr`]. Thin wrapper
/// kept so callers do not import `FromStr` everywhere.
///
/// # Errors
/// [`RecordError::BadAddress`] if the text is not an IP literal.
pub fn parse_ip(text: &str) -> Result<IpAddr, RecordError> {
    IpAddr::from_str(text.trim()).map_err(|_| RecordError::BadAddress)
}

/// Format any [`IpAddr`] in its canonical text form.
#[must_use]
pub fn format_ip(addr: IpAddr) -> String {
    addr.to_string()
}

/// The IPv4 octets of an address, or `None` if it is IPv6.
#[must_use]
pub const fn v4_octets(addr: IpAddr) -> Option<[u8; 4]> {
    match addr {
        IpAddr::V4(v4) => Some(v4.octets()),
        IpAddr::V6(_) => None,
    }
}

/// Build an [`Ipv4Addr`] from octets (re-export-style helper for tests/callers).
#[must_use]
pub const fn v4(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
    Ipv4Addr::new(a, b, c, d)
}

/// Build an [`Ipv6Addr`] from segments.
#[must_use]
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
pub const fn v6(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Ipv6Addr {
    Ipv6Addr::new(a, b, c, d, e, f, g, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_type_round_trips_through_mnemonic() {
        for rt in [
            RecordType::A,
            RecordType::Aaaa,
            RecordType::Cname,
            RecordType::Mx,
            RecordType::Txt,
            RecordType::Ptr,
            RecordType::Srv,
            RecordType::Ns,
            RecordType::Soa,
        ] {
            assert_eq!(RecordType::parse(rt.mnemonic()), Some(rt));
        }
        assert_eq!(RecordType::parse("aaaa"), Some(RecordType::Aaaa));
        assert_eq!(RecordType::parse("WAT"), None);
    }

    #[test]
    fn parses_ipv4_dotted_quad_for_a_record() {
        let d = RecordData::parse_addr(RecordType::A, "192.168.1.50").expect("ok");
        assert_eq!(d, RecordData::Addr(IpAddr::V4(v4(192, 168, 1, 50))));
        assert_eq!(d.to_text(), "192.168.1.50");
    }

    #[test]
    fn parses_ipv6_for_aaaa_record_and_compresses() {
        let d = RecordData::parse_addr(RecordType::Aaaa, "2001:db8:0:0:0:0:0:1").expect("ok");
        // std compresses the zero run.
        assert_eq!(d.to_text(), "2001:db8::1");
    }

    #[test]
    fn rejects_address_family_mismatch() {
        assert_eq!(
            RecordData::parse_addr(RecordType::A, "2001:db8::1"),
            Err(RecordError::AddressFamilyMismatch)
        );
        assert_eq!(
            RecordData::parse_addr(RecordType::Aaaa, "10.0.0.1"),
            Err(RecordError::AddressFamilyMismatch)
        );
    }

    #[test]
    fn rejects_garbage_address() {
        assert_eq!(
            RecordData::parse_addr(RecordType::A, "999.1.1.1"),
            Err(RecordError::BadAddress)
        );
        assert_eq!(parse_ip("not-an-ip"), Err(RecordError::BadAddress));
    }

    #[test]
    fn parse_name_data_validates_target() {
        assert!(RecordData::parse_name("printer.home.arpa").is_ok());
        assert!(matches!(
            RecordData::parse_name("bad..name"),
            Err(RecordError::BadName(_))
        ));
    }

    #[test]
    fn v4_octets_extracts_ipv4_only() {
        assert_eq!(v4_octets(IpAddr::V4(v4(10, 0, 0, 5))), Some([10, 0, 0, 5]));
        assert_eq!(v4_octets(IpAddr::V6(v6(0, 0, 0, 0, 0, 0, 0, 1))), None);
    }
}
