// SPDX-License-Identifier: Apache-2.0
//! A minimal, std-only DER walker that extracts the *subject* identity from an
//! X.509 leaf certificate — the bytes the apiserver needs to turn a verified
//! client certificate into a [`UserInfo`].
//!
//! Behavioural reference: the Kubernetes x509 authenticator
//! (`k8s.io/apiserver/pkg/authentication/request/x509`), which maps the
//! certificate **Subject Common Name** to the user name and each **Organization**
//! attribute to a group. RFC 5280 defines the `Certificate`/`TBSCertificate`
//! grammar and RFC 4519 the `CN` (2.5.4.3) / `O` (2.5.4.10) attribute OIDs.
//!
//! This is deliberately *not* a general X.509 library: it does no signature
//! verification (rustls/webpki already did that during the TLS handshake — see
//! [`crate::tls`]), no validity-window check, and no extension parsing. It walks
//! just far enough into the DER to read the Subject `Name`. Operating on raw
//! `&[u8]` keeps it dependency-free and unconditionally testable; the `tls`
//! feature only supplies the DER from the live handshake.

use crate::rbac::UserInfo;

/// DER object identifier for `id-at-commonName` (2.5.4.3), encoded as the OID
/// content octets (the leading `2.5` collapses to the single byte `0x55`).
const OID_COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
/// DER object identifier for `id-at-organizationName` (2.5.4.10).
const OID_ORGANIZATION: &[u8] = &[0x55, 0x04, 0x0a];

/// One DER tag-length-value triple, borrowed from the input.
struct Tlv<'a> {
    /// The identifier octet (tag class + constructed bit + number).
    tag: u8,
    /// The value (content) octets.
    value: &'a [u8],
    /// Total bytes consumed from the input (header + value).
    len: usize,
}

/// Read one DER TLV from the front of `input`. Supports the definite short form
/// and the long form up to a 4-byte length (more than enough for a certificate
/// subject). Returns `None` on any truncation or malformed length — the parser
/// is total and never panics on adversarial input.
fn read_tlv(input: &[u8]) -> Option<Tlv<'_>> {
    if input.len() < 2 {
        return None;
    }
    let tag = input[0];
    let first = input[1];
    let (value_len, header) = if first < 0x80 {
        (first as usize, 2)
    } else {
        let n = (first & 0x7f) as usize;
        // n == 0 is the indefinite form (illegal in DER); cap at 4 length bytes.
        if n == 0 || n > 4 || input.len() < 2 + n {
            return None;
        }
        let mut len = 0usize;
        for &b in &input[2..2 + n] {
            len = (len << 8) | b as usize;
        }
        (len, 2 + n)
    };
    let end = header.checked_add(value_len)?;
    if input.len() < end {
        return None;
    }
    Some(Tlv { tag, value: &input[header..end], len: end })
}

/// DER `SEQUENCE` tag (constructed).
const TAG_SEQUENCE: u8 = 0x30;
/// DER `SET` tag (constructed).
const TAG_SET: u8 = 0x31;
/// DER `OBJECT IDENTIFIER` tag.
const TAG_OID: u8 = 0x06;
/// Context-specific `[0]` tag (the explicit `version` field of `TBSCertificate`).
const TAG_VERSION: u8 = 0xa0;

/// Extract the subject identity from a DER-encoded X.509 certificate.
///
/// Returns the certificate's Subject Common Name as [`UserInfo::name`] and every
/// Subject Organization attribute as a group, in certificate order. Returns
/// `None` if the bytes are not a parseable certificate or carry no CN.
///
/// This performs **no** cryptographic verification; callers must only feed it a
/// certificate that the TLS layer has already validated against a trusted CA.
#[must_use]
pub fn subject_identity(cert_der: &[u8]) -> Option<UserInfo> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signature }
    let certificate = read_tlv(cert_der)?;
    if certificate.tag != TAG_SEQUENCE {
        return None;
    }
    // The first element of the certificate is the TBSCertificate SEQUENCE.
    let tbs = read_tlv(certificate.value)?;
    if tbs.tag != TAG_SEQUENCE {
        return None;
    }

    // TBSCertificate ::= SEQUENCE {
    //   version [0] EXPLICIT ... DEFAULT v1,   -- optional, context tag 0xA0
    //   serialNumber, signature, issuer, validity, subject, ... }
    // Walk past the optional version, then the four fields before `subject`.
    let mut rest = tbs.value;
    let first = read_tlv(rest)?;
    if first.tag == TAG_VERSION {
        rest = &rest[first.len..];
    }
    // Skip serialNumber, signature(AlgorithmIdentifier), issuer(Name),
    // validity — the four TLVs preceding the subject Name.
    for _ in 0..4 {
        let tlv = read_tlv(rest)?;
        rest = &rest[tlv.len..];
    }
    let subject = read_tlv(rest)?;
    if subject.tag != TAG_SEQUENCE {
        return None;
    }
    parse_name(subject.value)
}

/// Parse a Subject `Name` (an `RDNSequence`): a `SEQUENCE OF SET OF
/// AttributeTypeAndValue`. Collects the Common Name and Organization attributes.
fn parse_name(name: &[u8]) -> Option<UserInfo> {
    let mut common_name: Option<String> = None;
    let mut groups: Vec<String> = Vec::new();

    let mut rdns = name;
    while !rdns.is_empty() {
        let set = read_tlv(rdns)?;
        rdns = &rdns[set.len..];
        if set.tag != TAG_SET {
            continue;
        }
        // Each SET holds one or more AttributeTypeAndValue SEQUENCEs.
        let mut atvs = set.value;
        while !atvs.is_empty() {
            let atv = read_tlv(atvs)?;
            atvs = &atvs[atv.len..];
            if atv.tag != TAG_SEQUENCE {
                continue;
            }
            let oid = read_tlv(atv.value)?;
            if oid.tag != TAG_OID {
                continue;
            }
            // The attribute value follows the OID inside the ATV SEQUENCE.
            let value = read_tlv(&atv.value[oid.len..])?;
            let text = String::from_utf8_lossy(value.value).into_owned();
            if oid.value == OID_COMMON_NAME {
                common_name = Some(text);
            } else if oid.value == OID_ORGANIZATION {
                groups.push(text);
            }
        }
    }

    common_name.map(|name| UserInfo { name, groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CA-signed client certificate fixture: Subject
    /// `O=system:masters, O=dev, CN=alice`.
    const CLIENT_DER: &[u8] = include_bytes!("../tests/fixtures/client.der");

    #[test]
    fn extracts_common_name_as_user() {
        let id = subject_identity(CLIENT_DER).expect("parse subject");
        assert_eq!(id.name, "alice");
    }

    #[test]
    fn extracts_organizations_as_groups_in_order() {
        let id = subject_identity(CLIENT_DER).expect("parse subject");
        assert_eq!(id.groups, vec!["system:masters".to_string(), "dev".to_string()]);
    }

    #[test]
    fn rejects_non_certificate_bytes() {
        assert!(subject_identity(b"not a certificate").is_none());
        assert!(subject_identity(&[]).is_none());
        // A bare SEQUENCE header whose body is truncated must not panic.
        assert!(subject_identity(&[0x30, 0x82, 0xff, 0xff]).is_none());
    }

    #[test]
    fn read_tlv_handles_short_and_long_form() {
        // Short form: tag 0x02 (INTEGER), len 1, value 0x07.
        let t = read_tlv(&[0x02, 0x01, 0x07]).expect("short");
        assert_eq!(t.tag, 0x02);
        assert_eq!(t.value, &[0x07]);
        assert_eq!(t.len, 3);
        // Long form: 0x81 => one length byte = 0x02, then two value bytes.
        let t = read_tlv(&[0x04, 0x81, 0x02, 0xaa, 0xbb]).expect("long");
        assert_eq!(t.value, &[0xaa, 0xbb]);
        assert_eq!(t.len, 5);
        // Indefinite form (0x80) is illegal in DER.
        assert!(read_tlv(&[0x30, 0x80, 0x00]).is_none());
    }

    #[test]
    fn read_tlv_rejects_truncated_value() {
        // Claims 5 content bytes but only 2 are present.
        assert!(read_tlv(&[0x04, 0x05, 0x01, 0x02]).is_none());
    }
}
