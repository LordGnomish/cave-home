//! Self-announcement: build *this* node's advertisement record set.
//!
//! When a cave-home node comes up it must advertise itself on the LAN so other
//! hubs can discover it (ADR-005 OS-image path: "the box announces itself").
//! [`announce_self`] assembles the [`crate::record::ServiceRecord`] (PTR + SRV
//! + TXT + A) describing this node from its identity and network coordinates.
//!
//! The *live multicast loop* that actually emits and re-emits this record on
//! the wire is deferred to Phase 1b (see the parity manifest); here we build
//! the record once, deterministically, as pure data.

use crate::compat::Version;
use crate::peer::NodeRole;
use crate::record::{RecordError, ServiceRecord, TxtRecord};
use std::net::IpAddr;

/// The default record TTL (seconds) cave-home advertises with. mDNS commonly
/// uses 120 s for SRV/TXT/A; the registry uses this as the freshness window.
pub const DEFAULT_TTL_SECS: u32 = 120;

/// Build this node's own advertisement record set.
///
/// - `node_id` — the stable unique id for this node (used as the instance
///   label and the `id` TXT key).
/// - `hostname` — the node's host name (e.g. `kitchen.local`).
/// - `role` — its [`NodeRole`].
/// - `version` — its version string (`major.minor.patch`).
/// - `port` — the SRV port other nodes reach it on (also the `api` TXT value).
/// - `address` — one reachable [`IpAddr`]; use [`announce_self_multi`] for
///   multi-homed nodes.
///
/// # Errors
/// - [`RecordError::BadVersion`] if `version` does not parse.
/// - Any [`RecordError`] from record construction (empty fields, zero port).
pub fn announce_self(
    node_id: &str,
    hostname: &str,
    role: NodeRole,
    version: &str,
    port: u16,
    address: IpAddr,
) -> Result<ServiceRecord, RecordError> {
    announce_self_multi(node_id, hostname, role, version, port, vec![address])
}

/// Like [`announce_self`] but for a multi-homed node advertising several
/// addresses (one A/AAAA record each).
///
/// # Errors
/// As [`announce_self`].
pub fn announce_self_multi(
    node_id: &str,
    hostname: &str,
    role: NodeRole,
    version: &str,
    port: u16,
    addresses: Vec<IpAddr>,
) -> Result<ServiceRecord, RecordError> {
    let version =
        Version::parse(version).map_err(|_| RecordError::BadVersion(version.to_owned()))?;
    let txt = TxtRecord {
        node_id: node_id.to_owned(),
        role,
        version,
        api_port: port,
    };
    ServiceRecord::build(node_id, hostname, txt, addresses, port, DEFAULT_TTL_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn announces_full_record_set() {
        let rec = announce_self(
            "hub-kitchen",
            "kitchen.local",
            NodeRole::Primary,
            "1.4.0",
            8123,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
        )
        .expect("announces");

        assert_eq!(rec.node_id(), "hub-kitchen");
        assert_eq!(rec.port(), 8123);
        assert_eq!(rec.ptr.instance.to_dotted(), "hub-kitchen._cavehome._tcp.local");
        assert_eq!(rec.txt.role, NodeRole::Primary);
        assert_eq!(rec.txt.version, Version { major: 1, minor: 4, patch: 0 });
        assert_eq!(rec.ttl, DEFAULT_TTL_SECS);
        assert_eq!(rec.addresses().len(), 1);
    }

    #[test]
    fn announcement_txt_round_trips() {
        let rec = announce_self(
            "hub-attic",
            "attic.local",
            NodeRole::MlNode,
            "2.0.5",
            8123,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        )
        .expect("announces");
        let back = TxtRecord::decode(&rec.txt.encode()).expect("round-trips");
        assert_eq!(back, rec.txt);
    }

    #[test]
    fn multi_homed_node_has_one_a_record_per_address() {
        let rec = announce_self_multi(
            "hub-rack",
            "rack.local",
            NodeRole::Secondary,
            "1.4.1",
            8123,
            vec![
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
        )
        .expect("announces");
        assert_eq!(rec.addresses.len(), 2);
        // Each A record points at the same target host.
        assert!(rec.addresses.iter().all(|a| a.host == rec.srv.target));
    }

    #[test]
    fn rejects_bad_version() {
        let e = announce_self(
            "n",
            "h.local",
            NodeRole::Primary,
            "not-a-version",
            8123,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        assert_eq!(e, Err(RecordError::BadVersion("not-a-version".to_owned())));
    }

    #[test]
    fn rejects_zero_port() {
        let e = announce_self(
            "n",
            "h.local",
            NodeRole::Primary,
            "1.0.0",
            0,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        assert_eq!(e, Err(RecordError::ZeroPort));
    }
}
