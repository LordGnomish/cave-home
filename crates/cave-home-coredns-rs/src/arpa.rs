// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Reverse-DNS (`*.in-addr.arpa` / `*.ip6.arpa`) name <-> address mapping.

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
