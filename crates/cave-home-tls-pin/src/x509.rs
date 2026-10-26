// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Minimal DER reading — just enough to pull the subject common name out of
//! the `RDNSequence` webpki hands back (`Cert::subject`, the SEQUENCE contents).
//!
//! `Name ::= SEQUENCE OF RelativeDistinguishedName`,
//! `RelativeDistinguishedName ::= SET OF AttributeTypeAndValue`,
//! `AttributeTypeAndValue ::= SEQUENCE { type OID, value DirectoryString }`
//! (RFC 5280 §4.1.2.4).

/// DER of OID 2.5.4.3 (id-at-commonName), contents only.
const OID_COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];

const TAG_SET: u8 = 0x31;
const TAG_SEQUENCE: u8 = 0x30;
const TAG_OID: u8 = 0x06;
const TAG_UTF8_STRING: u8 = 0x0c;
const TAG_PRINTABLE_STRING: u8 = 0x13;
const TAG_T61_STRING: u8 = 0x14;
const TAG_IA5_STRING: u8 = 0x16;
const TAG_BMP_STRING: u8 = 0x1e;

/// Split one DER TLV off the front of `input`: `(tag, value, rest)`.
fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let len = rest[..n].iter().fold(0_usize, |acc, b| (acc << 8) | usize::from(*b));
        (len, &rest[n..])
    };
    if rest.len() < len {
        return None;
    }
    Some((tag, &rest[..len], &rest[len..]))
}

fn directory_string(tag: u8, value: &[u8]) -> Option<String> {
    match tag {
        TAG_UTF8_STRING | TAG_PRINTABLE_STRING | TAG_IA5_STRING | TAG_T61_STRING => {
            std::str::from_utf8(value).ok().map(str::to_owned)
        }
        TAG_BMP_STRING => {
            if value.len() % 2 != 0 {
                return None;
            }
            let units: Vec<u16> = value
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units).ok()
        }
        _ => None,
    }
}

/// The subject common name, given the *contents* of the subject `Name`
/// SEQUENCE. `None` if there is no CN, it cannot be decoded, or there is more
/// than one (ambiguous — treated as no match).
pub fn common_name(subject: &[u8]) -> Option<String> {
    let mut found = None;
    let mut rdns = subject;
    while !rdns.is_empty() {
        let (tag, set, rest) = tlv(rdns)?;
        rdns = rest;
        if tag != TAG_SET {
            return None;
        }
        let mut atvs = set;
        while !atvs.is_empty() {
            let (tag, atv, rest) = tlv(atvs)?;
            atvs = rest;
            if tag != TAG_SEQUENCE {
                return None;
            }
            let (oid_tag, oid, value) = tlv(atv)?;
            if oid_tag != TAG_OID || oid != OID_COMMON_NAME {
                continue;
            }
            let (value_tag, value, _) = tlv(value)?;
            if found.is_some() {
                return None;
            }
            found = Some(directory_string(value_tag, value)?);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `C=NL, O=Philips Hue, CN=root-bridge` (the Hue root's subject).
    const HUE_ROOT_SUBJECT: &[u8] = &[
        0x31, 0x0b, 0x30, 0x09, 0x06, 0x03, 0x55, 0x04, 0x06, 0x13, 0x02, b'N', b'L', //
        0x31, 0x14, 0x30, 0x12, 0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 0x0b, b'P', b'h', b'i', b'l',
        b'i', b'p', b's', b' ', b'H', b'u', b'e', //
        0x31, 0x14, 0x30, 0x12, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x0b, b'r', b'o', b'o', b't',
        b'-', b'b', b'r', b'i', b'd', b'g', b'e',
    ];

    #[test]
    fn reads_the_common_name() {
        assert_eq!(common_name(HUE_ROOT_SUBJECT).as_deref(), Some("root-bridge"));
    }

    #[test]
    fn no_common_name_is_none() {
        assert_eq!(common_name(&HUE_ROOT_SUBJECT[..13]), None);
        assert_eq!(common_name(&[]), None);
    }

    #[test]
    fn two_common_names_are_ambiguous() {
        let cn = &HUE_ROOT_SUBJECT[13 + 22..];
        let twice = [cn, cn].concat();
        assert_eq!(common_name(&twice), None);
    }

    #[test]
    fn truncated_input_is_none() {
        assert_eq!(common_name(&HUE_ROOT_SUBJECT[..HUE_ROOT_SUBJECT.len() - 1]), None);
    }

    #[test]
    fn bmp_string_decodes() {
        // SET { SEQUENCE { OID CN, BMPString "ab" } }
        let rdn = [0x31, 0x0d, 0x30, 0x0b, 0x06, 0x03, 0x55, 0x04, 0x03, 0x1e, 0x04, 0, b'a', 0, b'b'];
        assert_eq!(common_name(&rdn).as_deref(), Some("ab"));
    }
}
