// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Reverse-DNS (`*.in-addr.arpa` / `*.ip6.arpa`) name <-> address mapping.
//!
//! IPv4 reverse names list the four octets in reverse decimal under
//! `in-addr.arpa` (RFC 1035 §3.5); IPv6 reverse names list all 32 nibbles in
//! reverse hex under `ip6.arpa` (RFC 3596 §2.5). [`from_arpa`] is strict: a name
//! that is not a complete, well-formed reverse name yields `None` (the caller
//! then falls through or returns `NXDOMAIN`).

use crate::name::Name;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Build the reverse-DNS name for an address.
#[must_use]
pub fn to_arpa(ip: IpAddr) -> Name {
    let mut labels: Vec<Vec<u8>> = Vec::new();
    match ip {
        IpAddr::V4(v4) => {
            for octet in v4.octets().iter().rev() {
                labels.push(octet.to_string().into_bytes());
            }
            labels.push(b"in-addr".to_vec());
            labels.push(b"arpa".to_vec());
        }
        IpAddr::V6(v6) => {
            for byte in v6.octets().iter().rev() {
                // Low nibble first, then high nibble (reverse within the byte too).
                labels.push(vec![hex_nibble(byte & 0x0f)]);
                labels.push(vec![hex_nibble(byte >> 4)]);
            }
            labels.push(b"ip6".to_vec());
            labels.push(b"arpa".to_vec());
        }
    }
    // Lengths are fixed and well within limits, so construction cannot fail.
    Name::from_labels(labels).unwrap_or_else(|_| Name::root())
}

/// Parse a reverse-DNS name back into an address, or `None` if it is not a
/// complete, well-formed `in-addr.arpa` / `ip6.arpa` name.
#[must_use]
pub fn from_arpa(name: &Name) -> Option<IpAddr> {
    let labels = name.labels();
    let n = labels.len();
    // IPv4: 4 octet labels + "in-addr" + "arpa".
    if n == 6
        && labels[4].eq_ignore_ascii_case(b"in-addr")
        && labels[5].eq_ignore_ascii_case(b"arpa")
    {
        let mut octets = [0u8; 4];
        for i in 0..4 {
            // labels are most-significant-first and reversed vs. the address.
            let s = core::str::from_utf8(&labels[i]).ok()?;
            octets[3 - i] = s.parse().ok()?;
        }
        return Some(IpAddr::V4(Ipv4Addr::from(octets)));
    }
    // IPv6: 32 nibble labels + "ip6" + "arpa".
    if n == 34
        && labels[32].eq_ignore_ascii_case(b"ip6")
        && labels[33].eq_ignore_ascii_case(b"arpa")
    {
        let mut bytes = [0u8; 16];
        for byte_idx in 0..16 {
            // Two nibbles per byte; label order is fully reversed.
            let low = nibble_value(&labels[byte_idx * 2])?;
            let high = nibble_value(&labels[byte_idx * 2 + 1])?;
            bytes[15 - byte_idx] = (high << 4) | low;
        }
        return Some(IpAddr::V6(Ipv6Addr::from(bytes)));
    }
    None
}

/// The lowercase hex digit for a nibble value 0–15.
const fn hex_nibble(v: u8) -> u8 {
    if v < 10 { b'0' + v } else { b'a' + (v - 10) }
}

/// Parse a single-hex-digit label into its nibble value.
fn nibble_value(label: &[u8]) -> Option<u8> {
    match label {
        [c] => (*c as char).to_digit(16).map(|d| d as u8),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::name::Name;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn ipv4_to_and_from_arpa() {
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let n = to_arpa(ip);
        assert_eq!(n.to_string(), "4.3.2.1.in-addr.arpa.");
        assert_eq!(from_arpa(&n), Some(ip));
    }

    #[test]
    fn ipv6_to_and_from_arpa() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let n = to_arpa(ip);
        // 32 reversed nibbles then ip6.arpa.
        assert!(n.to_string().ends_with(".ip6.arpa."));
        assert_eq!(n.label_count(), 32 + 2);
        assert_eq!(from_arpa(&n), Some(ip));
    }

    #[test]
    fn non_arpa_names_are_rejected() {
        assert_eq!(from_arpa(&Name::parse("example.com").unwrap()), None);
        assert_eq!(from_arpa(&Name::parse("1.2.3.in-addr.arpa").unwrap()), None);
        assert_eq!(from_arpa(&Name::parse("zz.4.3.2.1.in-addr.arpa").unwrap()), None);
    }
}
