//! Reverse-DNS (PTR) helpers — map an IP to its reverse-lookup name and back.
//!
//! First-party from the *public* reverse-mapping conventions (RFC 1035 §3.5
//! `in-addr.arpa` for IPv4 and RFC 3596 §2.5 `ip6.arpa` nibble form for IPv6):
//! an IPv4 `a.b.c.d` reverses to `d.c.b.a.in-addr.arpa`, and an IPv6 address
//! reverses to its 32 reversed hex nibbles under `ip6.arpa`.
//!
//! The household never sees these names (Charter §6.3); they are the keys a
//! local PTR zone is indexed by so "which device is 192.168.1.50?" resolves to
//! a friendly name.

use crate::name::{DnsName, NameError};
use std::net::IpAddr;

/// The reverse-lookup name (`d.c.b.a.in-addr.arpa` / nibble `ip6.arpa`) for an
/// address.
///
/// # Errors
/// Propagates [`NameError`] only in the (unreachable in practice) case that the
/// constructed reverse name fails validation; the constructed form is always
/// well-formed for a valid `IpAddr`.
pub fn ptr_name(addr: IpAddr) -> Result<DnsName, NameError> {
    let text = match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            // 16 octets -> 32 nibbles, reversed, dot-separated, then ip6.arpa.
            let mut s = String::with_capacity(72);
            for octet in v6.octets().iter().rev() {
                let lo = octet & 0x0f;
                let hi = (octet >> 4) & 0x0f;
                // Low nibble first (it is the least-significant of the octet,
                // and octets are already iterated most-significant-last).
                s.push(nibble_hex(lo));
                s.push('.');
                s.push(nibble_hex(hi));
                s.push('.');
            }
            s.push_str("ip6.arpa");
            s
        }
    };
    DnsName::parse(&text)
}

const fn nibble_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + (n - 10)) as char,
    }
}

/// A PTR (reverse) zone: maps reverse-lookup names to the friendly target name
/// of the device at that address.
#[derive(Debug, Clone, Default)]
pub struct ReverseZone {
    map: std::collections::HashMap<DnsName, DnsName>,
}

impl ReverseZone {
    /// An empty reverse zone.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
        }
    }

    /// Register that `addr` belongs to the device named `target`.
    ///
    /// # Errors
    /// [`NameError`] if the reverse name cannot be constructed.
    pub fn insert(&mut self, addr: IpAddr, target: DnsName) -> Result<(), NameError> {
        let key = ptr_name(addr)?;
        self.map.insert(key, target);
        Ok(())
    }

    /// Resolve an address to its friendly target name, if registered.
    #[must_use]
    pub fn resolve(&self, addr: IpAddr) -> Option<&DnsName> {
        let key = ptr_name(addr).ok()?;
        self.map.get(&key)
    }

    /// Resolve a reverse-lookup name (as a PTR query would carry it) to the
    /// target name.
    #[must_use]
    pub fn resolve_name(&self, ptr: &DnsName) -> Option<&DnsName> {
        self.map.get(ptr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{v4, v6};

    #[test]
    fn ipv4_reverses_to_in_addr_arpa() {
        let n = ptr_name(IpAddr::V4(v4(192, 168, 1, 50))).expect("ptr");
        assert_eq!(n.as_str(), "50.1.168.192.in-addr.arpa");
    }

    #[test]
    fn ipv6_reverses_to_nibble_ip6_arpa() {
        // ::1 -> least-significant nibble (the 1) first, then 31 zeros, per
        // RFC 3596 §2.5.
        let n = ptr_name(IpAddr::V6(v6(0, 0, 0, 0, 0, 0, 0, 1))).expect("ptr");
        let expected = format!("1.{}ip6.arpa", "0.".repeat(31));
        assert_eq!(n.as_str(), expected);
        assert!(n.as_str().ends_with("ip6.arpa"));
        // 32 nibble labels + 2 suffix labels.
        assert_eq!(n.label_count(), 34);
    }

    #[test]
    fn reverse_zone_round_trips_address_to_name() {
        let mut z = ReverseZone::new();
        let printer = DnsName::parse("printer.home.arpa").expect("name");
        z.insert(IpAddr::V4(v4(192, 168, 1, 50)), printer.clone())
            .expect("insert");
        assert_eq!(z.resolve(IpAddr::V4(v4(192, 168, 1, 50))), Some(&printer));
        assert_eq!(z.resolve(IpAddr::V4(v4(192, 168, 1, 51))), None);
    }

    #[test]
    fn reverse_zone_resolves_by_ptr_name() {
        let mut z = ReverseZone::new();
        let nas = DnsName::parse("nas.home.arpa").expect("name");
        z.insert(IpAddr::V4(v4(10, 0, 0, 9)), nas.clone()).expect("insert");
        let ptr = DnsName::parse("9.0.0.10.in-addr.arpa").expect("ptr");
        assert_eq!(z.resolve_name(&ptr), Some(&nas));
    }
}
