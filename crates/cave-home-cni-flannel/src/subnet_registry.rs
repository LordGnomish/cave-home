// SPDX-License-Identifier: Apache-2.0
//! The durable subnet-lease store — port of `pkg/subnet/etcd` + `subnet.go`
//! key handling, driven over cave-home's kine (the K3s etcd).
//!
//! flannel persists each node's lease in etcd under
//! `/coreos.com/network/subnets/<subnet-key>`, where the key is the subnet
//! rendered as `10.42.1.0-24` and the value is the `LeaseAttrs` JSON
//! (`PublicIP`, `BackendType`, `BackendData`). The daemon *watches* that prefix
//! and turns each change into a `lease.Event` that drives the datapath
//! ([`crate::vxlan_network`] / [`crate::route_network`]).
//!
//! This module ports the pieces that are pure logic:
//!
//! * [`make_subnet_key`] / [`parse_subnet_key`] ↔ `subnet.go::MakeSubnetKey` /
//!   `ParseSubnetKey`.
//! * [`LeaseAttrs::to_json`] / [`LeaseAttrs::from_json`] ↔ the etcd value codec
//!   (faithful to flannel's `LeaseAttrs` + `vxlanLeaseAttrs` JSON shapes).
//! * [`LeaseRegistry`] ↔ the watched lease cache: it consumes
//!   [`cave_home_kine_rs::watch::WatchEvent`]s on the subnet prefix and emits
//!   [`LeaseEvent`]s, holding the cache needed to reconstruct a removed lease's
//!   attributes (a `Delete` carries no value).
//!
//! The kine I/O itself — the SQL-backed watch stream — is kine's job; this is
//! the flannel-side translation that turns it into datapath work.

use std::collections::BTreeMap;
use std::net::IpAddr;

use cave_home_kine_rs::watch::{EventKind, WatchEvent};

use crate::backend::{MacAddr, NodeBackendData};
use crate::cidr::Cidr;
use crate::routes::PeerLease;
use crate::vxlan_network::LeaseEvent;

/// The etcd prefix flannel leases live under.
pub const SUBNET_PREFIX: &str = "/coreos.com/network/subnets/";

/// Render a subnet as a flannel etcd subnet key: `10.42.1.0-24`
/// (`MakeSubnetKey` — IP with `.` separators, prefix joined with `-`).
#[must_use]
pub fn make_subnet_key(sn: Cidr) -> String {
    format!("{}-{}", sn.network(), sn.prefix_len())
}

/// The full etcd key (prefix + subnet key) for a lease.
#[must_use]
pub fn full_key(sn: Cidr) -> String {
    format!("{SUBNET_PREFIX}{}", make_subnet_key(sn))
}

/// Parse a subnet key (`10.42.1.0-24`, optionally with a `&v6` suffix that we
/// drop) back into a [`Cidr`] — `ParseSubnetKey` for the v4 case.
#[must_use]
pub fn parse_subnet_key(s: &str) -> Option<Cidr> {
    // Drop any dual-stack "&<v6>" suffix; we model the v4 subnet.
    let v4 = s.split('&').next().unwrap_or(s);
    let (ip, prefix) = v4.rsplit_once('-')?;
    let addr: IpAddr = ip.parse().ok()?;
    let prefix: u8 = prefix.parse().ok()?;
    Cidr::new(addr, prefix).ok()
}

/// Strip the subnet prefix off a full etcd key and parse the subnet.
#[must_use]
pub fn subnet_from_full_key(key: &str) -> Option<Cidr> {
    parse_subnet_key(key.strip_prefix(SUBNET_PREFIX)?)
}

/// A node's lease attributes, as stored in the etcd value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAttrs {
    /// The backend data the node advertises (carries its public IP).
    pub data: NodeBackendData,
    /// The VXLAN VNI (only meaningful for the VXLAN backend; flannel stores it
    /// inside `BackendData`).
    pub vni: u32,
}

impl LeaseAttrs {
    /// Serialise to the flannel etcd value JSON.
    ///
    /// VXLAN: `{"PublicIP":"..","BackendType":"vxlan","BackendData":{"VNI":N,
    /// "VtepMAC":".."}}`; host-gw omits `BackendData`; wireguard carries
    /// `{"PublicKey":".."}`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let public_ip = self.data.public_ip();
        match &self.data {
            NodeBackendData::Vxlan { vtep_mac, .. } => format!(
                "{{\"PublicIP\":\"{public_ip}\",\"BackendType\":\"vxlan\",\
                 \"BackendData\":{{\"VNI\":{},\"VtepMAC\":\"{vtep_mac}\"}}}}",
                self.vni
            ),
            NodeBackendData::HostGw { .. } => {
                format!("{{\"PublicIP\":\"{public_ip}\",\"BackendType\":\"host-gw\"}}")
            }
            NodeBackendData::Wireguard { public_key, .. } => format!(
                "{{\"PublicIP\":\"{public_ip}\",\"BackendType\":\"wireguard\",\
                 \"BackendData\":{{\"PublicKey\":\"{public_key}\"}}}}"
            ),
        }
    }

    /// Parse a flannel etcd value JSON back into [`LeaseAttrs`].
    ///
    /// A tolerant field extractor over the fixed flannel shape (no general JSON
    /// dependency); round-trips [`to_json`](LeaseAttrs::to_json).
    ///
    /// # Errors
    /// Returns [`LeaseDecodeError`] if required fields are missing or malformed.
    pub fn from_json(s: &str) -> Result<Self, LeaseDecodeError> {
        let public_ip: IpAddr = json_str(s, "PublicIP")
            .ok_or(LeaseDecodeError::MissingField("PublicIP"))?
            .parse()
            .map_err(|_| LeaseDecodeError::Malformed)?;
        let backend = json_str(s, "BackendType")
            .ok_or(LeaseDecodeError::MissingField("BackendType"))?;
        match backend.as_str() {
            "vxlan" => {
                let mac: MacAddr = json_str(s, "VtepMAC")
                    .ok_or(LeaseDecodeError::MissingField("VtepMAC"))?
                    .parse()
                    .map_err(|_| LeaseDecodeError::Malformed)?;
                let vni = json_num(s, "VNI").unwrap_or(1);
                Ok(Self {
                    data: NodeBackendData::Vxlan {
                        public_ip,
                        vtep_mac: mac,
                    },
                    vni,
                })
            }
            "host-gw" => Ok(Self {
                data: NodeBackendData::HostGw { public_ip },
                vni: 0,
            }),
            "wireguard" => {
                let key = json_str(s, "PublicKey")
                    .ok_or(LeaseDecodeError::MissingField("PublicKey"))?;
                Ok(Self {
                    data: NodeBackendData::Wireguard {
                        public_ip,
                        public_key: key,
                    },
                    vni: 0,
                })
            }
            _ => Err(LeaseDecodeError::UnknownBackend),
        }
    }
}

/// Extract a quoted-string JSON field value (`"key":"value"`).
fn json_str(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = s.find(&needle)? + needle.len();
    let rest = &s[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Extract a numeric JSON field value (`"key":N`).
fn json_num(s: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\":");
    let start = s.find(&needle)? + needle.len();
    let rest = &s[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// An error decoding a lease value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseDecodeError {
    /// A required JSON field was absent.
    MissingField(&'static str),
    /// A field could not be parsed.
    Malformed,
    /// The `BackendType` is not one this build supports.
    UnknownBackend,
}

impl std::fmt::Display for LeaseDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(k) => write!(f, "lease value missing field {k}"),
            Self::Malformed => write!(f, "lease value field is malformed"),
            Self::UnknownBackend => write!(f, "lease value has an unknown BackendType"),
        }
    }
}

impl std::error::Error for LeaseDecodeError {}

/// The watched lease cache: turns kine watch events into flannel lease events.
///
/// Holds the last-seen attributes per subnet so a `Delete` (which carries no
/// value) can be turned into a fully-populated `Removed` event — exactly what
/// upstream's cached lease list provides.
#[derive(Debug, Default, Clone)]
pub struct LeaseRegistry {
    leases: BTreeMap<Cidr, LeaseAttrs>,
}

impl LeaseRegistry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            leases: BTreeMap::new(),
        }
    }

    /// The currently-known leases, as peer leases (e.g. to seed a node at
    /// start-up before watching).
    #[must_use]
    pub fn snapshot(&self) -> Vec<PeerLease> {
        self.leases
            .iter()
            .map(|(sn, attrs)| PeerLease {
                node: make_subnet_key(*sn),
                subnet: *sn,
                data: attrs.data.clone(),
            })
            .collect()
    }

    /// Apply a kine [`WatchEvent`] on the subnet prefix, returning the flannel
    /// [`LeaseEvent`] it implies (or `None` for an unrelated / undecodable key).
    ///
    /// `Put` decodes the value, caches it and yields `Added`; `Delete` looks the
    /// cached lease up, drops it and yields `Removed`.
    pub fn apply(&mut self, evt: &WatchEvent) -> Option<LeaseEvent> {
        let key = std::str::from_utf8(&evt.key).ok()?;
        let subnet = subnet_from_full_key(key)?;
        match evt.kind {
            EventKind::Put => {
                let value = std::str::from_utf8(&evt.value).ok()?;
                let attrs = LeaseAttrs::from_json(value).ok()?;
                self.leases.insert(subnet, attrs.clone());
                Some(LeaseEvent::Added(PeerLease {
                    node: make_subnet_key(subnet),
                    subnet,
                    data: attrs.data,
                }))
            }
            EventKind::Delete => {
                let attrs = self.leases.remove(&subnet)?;
                Some(LeaseEvent::Removed(PeerLease {
                    node: make_subnet_key(subnet),
                    subnet,
                    data: attrs.data,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).expect("v4"))
    }
    fn cidr(s: &str) -> Cidr {
        Cidr::from_str(s).expect("cidr")
    }
    fn put(key: &str, value: &str) -> WatchEvent {
        WatchEvent {
            kind: EventKind::Put,
            key: key.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
            revision: 1,
            create_revision: 1,
        }
    }
    fn del(key: &str) -> WatchEvent {
        WatchEvent {
            kind: EventKind::Delete,
            key: key.as_bytes().to_vec(),
            value: Vec::new(),
            revision: 2,
            create_revision: 1,
        }
    }

    #[test]
    fn subnet_key_round_trips() {
        let sn = cidr("10.42.1.0/24");
        assert_eq!(make_subnet_key(sn), "10.42.1.0-24");
        assert_eq!(parse_subnet_key("10.42.1.0-24"), Some(sn));
        assert_eq!(full_key(sn), "/coreos.com/network/subnets/10.42.1.0-24");
        assert_eq!(subnet_from_full_key(&full_key(sn)), Some(sn));
    }

    #[test]
    fn parse_subnet_key_drops_dualstack_suffix() {
        assert_eq!(
            parse_subnet_key("10.42.1.0-24&fd00-aa-0-0-0-0-0-0-64"),
            Some(cidr("10.42.1.0/24"))
        );
    }

    #[test]
    fn vxlan_lease_value_round_trips() {
        let attrs = LeaseAttrs {
            data: NodeBackendData::Vxlan {
                public_ip: v4("192.168.1.2"),
                vtep_mac: MacAddr::new([0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f]),
            },
            vni: 1,
        };
        let json = attrs.to_json();
        assert!(json.contains("\"BackendType\":\"vxlan\""));
        assert!(json.contains("\"VtepMAC\":\"0a:1b:2c:3d:4e:5f\""));
        assert_eq!(LeaseAttrs::from_json(&json).expect("decode"), attrs);
    }

    #[test]
    fn hostgw_lease_value_round_trips_without_backenddata() {
        let attrs = LeaseAttrs {
            data: NodeBackendData::HostGw {
                public_ip: v4("192.168.1.3"),
            },
            vni: 0,
        };
        let json = attrs.to_json();
        assert!(!json.contains("BackendData"));
        assert_eq!(LeaseAttrs::from_json(&json).expect("decode"), attrs);
    }

    #[test]
    fn decodes_real_flannel_vxlan_value() {
        // The exact shape flanneld writes.
        let json = "{\"PublicIP\":\"10.0.0.5\",\"BackendType\":\"vxlan\",\
                     \"BackendData\":{\"VNI\":1,\"VtepMAC\":\"de:ad:be:ef:00:01\"}}";
        let attrs = LeaseAttrs::from_json(json).expect("decode");
        assert_eq!(attrs.vni, 1);
        match attrs.data {
            NodeBackendData::Vxlan { public_ip, vtep_mac } => {
                assert_eq!(public_ip, v4("10.0.0.5"));
                assert_eq!(vtep_mac.to_string(), "de:ad:be:ef:00:01");
            }
            other => panic!("expected vxlan, got {other:?}"),
        }
    }

    #[test]
    fn registry_put_yields_added_and_caches() {
        let mut reg = LeaseRegistry::new();
        let attrs = LeaseAttrs {
            data: NodeBackendData::Vxlan {
                public_ip: v4("192.168.1.2"),
                vtep_mac: MacAddr::new([2; 6]),
            },
            vni: 1,
        };
        let evt = put(&full_key(cidr("10.42.1.0/24")), &attrs.to_json());
        let lease_evt = reg.apply(&evt).expect("event");
        match lease_evt {
            LeaseEvent::Added(p) => {
                assert_eq!(p.subnet, cidr("10.42.1.0/24"));
                assert_eq!(p.data, attrs.data);
            }
            other => panic!("expected Added, got {other:?}"),
        }
        assert_eq!(reg.snapshot().len(), 1);
    }

    #[test]
    fn registry_delete_reconstructs_removed_from_cache() {
        let mut reg = LeaseRegistry::new();
        let sn = cidr("10.42.1.0/24");
        let attrs = LeaseAttrs {
            data: NodeBackendData::Vxlan {
                public_ip: v4("192.168.1.2"),
                vtep_mac: MacAddr::new([2; 6]),
            },
            vni: 1,
        };
        reg.apply(&put(&full_key(sn), &attrs.to_json())).expect("add");
        // The Delete carries no value, but the cache supplies the attrs.
        let removed = reg.apply(&del(&full_key(sn))).expect("removed");
        match removed {
            LeaseEvent::Removed(p) => {
                assert_eq!(p.subnet, sn);
                assert_eq!(p.data, attrs.data); // full attrs, from cache
            }
            other => panic!("expected Removed, got {other:?}"),
        }
        assert!(reg.snapshot().is_empty());
    }

    #[test]
    fn registry_ignores_keys_outside_the_prefix() {
        let mut reg = LeaseRegistry::new();
        assert!(reg.apply(&put("/other/key", "{}")).is_none());
        // a delete of an unknown subnet yields nothing.
        assert!(reg.apply(&del(&full_key(cidr("10.42.9.0/24")))).is_none());
    }
}
