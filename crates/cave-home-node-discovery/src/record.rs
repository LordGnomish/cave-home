//! The mDNS / DNS-SD record model for the cave-home service.
//!
//! cave-home advertises itself as a DNS-SD service of type
//! [`SERVICE_TYPE`] (`_cavehome._tcp.local`). A full advertisement is a set of
//! records:
//!
//! - **PTR** — `_cavehome._tcp.local` → `<instance>._cavehome._tcp.local`:
//!   "a service of this type exists, here is its instance name".
//! - **SRV** — `<instance>...` → priority / weight / port / target host:
//!   "reach this instance at this host:port".
//! - **TXT** — `key=value` metadata: node id, role, version, api port.
//! - **A** — `target host` → IPv4 address (AAAA for IPv6 is modelled the same
//!   way via [`std::net::IpAddr`]).
//!
//! This module models those records as typed structs and the TXT key/value
//! codec. The actual DNS *message* framing (header, question/answer sections,
//! compression pointers) and the UDP multicast transport are deferred to
//! Phase 1b — see the parity manifest. The names within are encoded with the
//! real wire codec ([`crate::dns_name`]).

use crate::compat::Version;
use crate::dns_name::DnsName;
use crate::peer::NodeRole;
use std::net::IpAddr;

/// The DNS-SD service type cave-home advertises under.
pub const SERVICE_TYPE: &str = "_cavehome._tcp.local";

/// TXT keys (DNS-SD `key=value`, RFC 6763 §6). Machine-facing, never shown to
/// the end-user.
const TXT_KEY_NODE_ID: &str = "id";
const TXT_KEY_ROLE: &str = "role";
const TXT_KEY_VERSION: &str = "v";
const TXT_KEY_API_PORT: &str = "api";

/// Why a record could not be built, parsed or validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// A required field was empty.
    EmptyField(&'static str),
    /// The advertised port was zero.
    ZeroPort,
    /// A required TXT key was missing on parse.
    MissingTxtKey(&'static str),
    /// A TXT entry had no `=` separator.
    MalformedTxtEntry(String),
    /// A TXT key appeared more than once.
    DuplicateTxtKey(String),
    /// The `role` TXT value was not a known role token.
    UnknownRole(String),
    /// The `api` TXT value was not a valid port number.
    BadApiPort(String),
    /// The version string failed to parse.
    BadVersion(String),
    /// An instance / service name was not a valid DNS name.
    BadName(String),
}

impl core::fmt::Display for RecordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyField(name) => write!(f, "record field {name:?} is empty"),
            Self::ZeroPort => f.write_str("record port is zero"),
            Self::MissingTxtKey(k) => write!(f, "required TXT key {k:?} is missing"),
            Self::MalformedTxtEntry(e) => write!(f, "TXT entry {e:?} has no '=' separator"),
            Self::DuplicateTxtKey(k) => write!(f, "TXT key {k:?} appears more than once"),
            Self::UnknownRole(r) => write!(f, "TXT role {r:?} is not a known role"),
            Self::BadApiPort(p) => write!(f, "TXT api port {p:?} is not a valid port"),
            Self::BadVersion(v) => write!(f, "TXT version {v:?} is not a valid version"),
            Self::BadName(n) => write!(f, "name {n:?} is not a valid DNS name"),
        }
    }
}

impl std::error::Error for RecordError {}

/// A DNS-SD PTR record: service type → instance name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtrRecord {
    /// The service type, e.g. `_cavehome._tcp.local`.
    pub service: DnsName,
    /// The instance name, e.g. `hub-kitchen._cavehome._tcp.local`.
    pub instance: DnsName,
}

/// A DNS SRV record: where to reach an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    /// The target host name, e.g. `kitchen.local`.
    pub target: DnsName,
}

/// A DNS A / AAAA record: a host name's IP address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ARecord {
    pub host: DnsName,
    pub addr: IpAddr,
}

/// The TXT record's decoded `key=value` metadata for a cave-home instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxtRecord {
    pub node_id: String,
    pub role: NodeRole,
    pub version: Version,
    pub api_port: u16,
}

impl TxtRecord {
    /// Encode to the ordered list of `key=value` strings DNS-SD carries.
    ///
    /// Order is stable (id, role, v, api) so round-trips and tests are
    /// deterministic; DNS-SD itself treats TXT order as insignificant.
    #[must_use]
    pub fn encode(&self) -> Vec<String> {
        vec![
            format!("{TXT_KEY_NODE_ID}={}", self.node_id),
            format!("{TXT_KEY_ROLE}={}", self.role.wire_token()),
            format!("{TXT_KEY_VERSION}={}", self.version),
            format!("{TXT_KEY_API_PORT}={}", self.api_port),
        ]
    }

    /// Decode from a list of `key=value` strings.
    ///
    /// # Errors
    /// - [`RecordError::MalformedTxtEntry`] for an entry with no `=`.
    /// - [`RecordError::DuplicateTxtKey`] for a repeated key.
    /// - [`RecordError::MissingTxtKey`] when a required key is absent.
    /// - [`RecordError::UnknownRole`] / [`RecordError::BadApiPort`] /
    ///   [`RecordError::BadVersion`] for unparseable values.
    pub fn decode(entries: &[String]) -> Result<Self, RecordError> {
        let mut node_id: Option<String> = None;
        let mut role: Option<NodeRole> = None;
        let mut version: Option<Version> = None;
        let mut api_port: Option<u16> = None;

        for entry in entries {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| RecordError::MalformedTxtEntry(entry.clone()))?;
            match key {
                TXT_KEY_NODE_ID => {
                    if node_id.is_some() {
                        return Err(RecordError::DuplicateTxtKey(key.to_owned()));
                    }
                    node_id = Some(value.to_owned());
                }
                TXT_KEY_ROLE => {
                    if role.is_some() {
                        return Err(RecordError::DuplicateTxtKey(key.to_owned()));
                    }
                    role = Some(
                        NodeRole::from_wire(value)
                            .ok_or_else(|| RecordError::UnknownRole(value.to_owned()))?,
                    );
                }
                TXT_KEY_VERSION => {
                    if version.is_some() {
                        return Err(RecordError::DuplicateTxtKey(key.to_owned()));
                    }
                    version = Some(
                        Version::parse(value)
                            .map_err(|_| RecordError::BadVersion(value.to_owned()))?,
                    );
                }
                TXT_KEY_API_PORT => {
                    if api_port.is_some() {
                        return Err(RecordError::DuplicateTxtKey(key.to_owned()));
                    }
                    api_port = Some(
                        value
                            .parse::<u16>()
                            .map_err(|_| RecordError::BadApiPort(value.to_owned()))?,
                    );
                }
                // Unknown keys are tolerated (forward-compatible TXT), ignored.
                _ => {}
            }
        }

        let node_id = node_id.ok_or(RecordError::MissingTxtKey(TXT_KEY_NODE_ID))?;
        if node_id.is_empty() {
            return Err(RecordError::EmptyField("node_id"));
        }
        Ok(Self {
            node_id,
            role: role.ok_or(RecordError::MissingTxtKey(TXT_KEY_ROLE))?,
            version: version.ok_or(RecordError::MissingTxtKey(TXT_KEY_VERSION))?,
            api_port: api_port.ok_or(RecordError::MissingTxtKey(TXT_KEY_API_PORT))?,
        })
    }
}

/// A complete cave-home advertisement: the PTR + SRV + TXT + A record set for
/// one instance. This is what [`crate::announce::announce_self`] produces and
/// what the registry consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    pub ptr: PtrRecord,
    pub srv: SrvRecord,
    pub txt: TxtRecord,
    /// One A/AAAA record per advertised address.
    pub addresses: Vec<ARecord>,
    /// Record TTL in seconds (used as the freshness window by the registry).
    pub ttl: u32,
}

impl ServiceRecord {
    /// Build and validate a service record from its parts.
    ///
    /// # Errors
    /// - [`RecordError::EmptyField`] for an empty node id / hostname / instance.
    /// - [`RecordError::ZeroPort`] for a zero SRV port.
    /// - [`RecordError::BadName`] if the instance / target names are invalid.
    pub fn build(
        instance_label: &str,
        host: &str,
        txt: TxtRecord,
        addresses: Vec<IpAddr>,
        port: u16,
        ttl: u32,
    ) -> Result<Self, RecordError> {
        if instance_label.is_empty() {
            return Err(RecordError::EmptyField("instance"));
        }
        if host.is_empty() {
            return Err(RecordError::EmptyField("host"));
        }
        if port == 0 {
            return Err(RecordError::ZeroPort);
        }

        let service =
            DnsName::parse(SERVICE_TYPE).map_err(|_| RecordError::BadName(SERVICE_TYPE.to_owned()))?;
        // Instance name = "<label>._cavehome._tcp.local".
        let instance_dotted = format!("{instance_label}.{SERVICE_TYPE}");
        let instance = DnsName::parse(&instance_dotted)
            .map_err(|_| RecordError::BadName(instance_dotted.clone()))?;
        let target = DnsName::parse(host).map_err(|_| RecordError::BadName(host.to_owned()))?;

        let ptr = PtrRecord { service, instance };
        let srv = SrvRecord { priority: 0, weight: 0, port, target: target.clone() };
        let addresses = addresses
            .into_iter()
            .map(|addr| ARecord { host: target.clone(), addr })
            .collect();

        Ok(Self { ptr, srv, txt, addresses, ttl })
    }

    /// The unique node id this record advertises (from its TXT).
    #[must_use]
    pub fn node_id(&self) -> &str {
        &self.txt.node_id
    }

    /// The advertised reachable port (the SRV port).
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.srv.port
    }

    /// The list of advertised addresses.
    #[must_use]
    pub fn addresses(&self) -> Vec<IpAddr> {
        self.addresses.iter().map(|a| a.addr).collect()
    }

    /// The advertised host name as a dotted string.
    #[must_use]
    pub fn hostname(&self) -> String {
        self.srv.target.to_dotted()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ver() -> Version {
        Version { major: 1, minor: 4, patch: 0 }
    }

    fn txt() -> TxtRecord {
        TxtRecord {
            node_id: "hub-kitchen".to_owned(),
            role: NodeRole::Primary,
            version: ver(),
            api_port: 8123,
        }
    }

    #[test]
    fn txt_encode_has_all_keys() {
        let e = txt().encode();
        assert_eq!(
            e,
            vec![
                "id=hub-kitchen".to_owned(),
                "role=primary".to_owned(),
                "v=1.4.0".to_owned(),
                "api=8123".to_owned(),
            ]
        );
    }

    #[test]
    fn txt_round_trip() {
        let original = txt();
        let back = TxtRecord::decode(&original.encode()).expect("decodes");
        assert_eq!(back, original);
    }

    #[test]
    fn txt_tolerates_unknown_keys() {
        let mut entries = txt().encode();
        entries.push("future=whatever".to_owned());
        let back = TxtRecord::decode(&entries).expect("decodes ignoring unknown");
        assert_eq!(back, txt());
    }

    #[test]
    fn txt_rejects_missing_required_key() {
        let entries = vec!["role=primary".to_owned(), "v=1.4.0".to_owned(), "api=8123".to_owned()];
        assert_eq!(
            TxtRecord::decode(&entries),
            Err(RecordError::MissingTxtKey("id"))
        );
    }

    #[test]
    fn txt_rejects_malformed_entry() {
        let entries = vec!["id-hub".to_owned()];
        assert_eq!(
            TxtRecord::decode(&entries),
            Err(RecordError::MalformedTxtEntry("id-hub".to_owned()))
        );
    }

    #[test]
    fn txt_rejects_duplicate_key() {
        let entries = vec![
            "id=a".to_owned(),
            "id=b".to_owned(),
            "role=primary".to_owned(),
            "v=1.4.0".to_owned(),
            "api=8123".to_owned(),
        ];
        assert_eq!(
            TxtRecord::decode(&entries),
            Err(RecordError::DuplicateTxtKey("id".to_owned()))
        );
    }

    #[test]
    fn txt_rejects_bad_role_version_port() {
        let base = txt().encode();
        let mut bad_role = base.clone();
        bad_role[1] = "role=overlord".to_owned();
        assert_eq!(
            TxtRecord::decode(&bad_role),
            Err(RecordError::UnknownRole("overlord".to_owned()))
        );
        let mut bad_ver = base.clone();
        bad_ver[2] = "v=1.x".to_owned();
        assert_eq!(
            TxtRecord::decode(&bad_ver),
            Err(RecordError::BadVersion("1.x".to_owned()))
        );
        let mut bad_port = base;
        bad_port[3] = "api=99999".to_owned();
        assert_eq!(
            TxtRecord::decode(&bad_port),
            Err(RecordError::BadApiPort("99999".to_owned()))
        );
    }

    #[test]
    fn build_service_record_wires_names() {
        let rec = ServiceRecord::build(
            "hub-kitchen",
            "kitchen.local",
            txt(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))],
            8123,
            120,
        )
        .expect("builds");
        assert_eq!(rec.ptr.service.to_dotted(), "_cavehome._tcp.local");
        assert_eq!(rec.ptr.instance.to_dotted(), "hub-kitchen._cavehome._tcp.local");
        assert_eq!(rec.srv.target.to_dotted(), "kitchen.local");
        assert_eq!(rec.srv.port, 8123);
        assert_eq!(rec.node_id(), "hub-kitchen");
        assert_eq!(rec.hostname(), "kitchen.local");
        assert_eq!(rec.addresses(), vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))]);
    }

    #[test]
    fn build_rejects_empty_fields_and_zero_port() {
        assert_eq!(
            ServiceRecord::build("", "h", txt(), vec![], 8123, 120),
            Err(RecordError::EmptyField("instance"))
        );
        assert_eq!(
            ServiceRecord::build("hub", "", txt(), vec![], 8123, 120),
            Err(RecordError::EmptyField("host"))
        );
        assert_eq!(
            ServiceRecord::build("hub", "h.local", txt(), vec![], 0, 120),
            Err(RecordError::ZeroPort)
        );
    }

    #[test]
    fn ptr_instance_names_encode_to_wire() {
        let rec = ServiceRecord::build(
            "hub-kitchen",
            "kitchen.local",
            txt(),
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            8123,
            120,
        )
        .expect("builds");
        // The instance name survives a wire round-trip via the DnsName codec.
        let wire = rec.ptr.instance.encode().expect("encodes");
        let (back, _) = DnsName::decode(&wire, 0).expect("decodes");
        assert!(back.eq_ignore_case(&rec.ptr.instance));
    }
}
