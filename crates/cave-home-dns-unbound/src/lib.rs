//! `cave-home-dns-unbound` — the local DNS resolution-decision core (ADR-022).
//!
//! This crate is the **brain** of the home's own recursive/local DNS resolver:
//! it models domain names and records, decides how a query is answered from the
//! household's local zones, routes the rest to the right upstream, gates who is
//! allowed to ask, and caches answers — all as pure logic with no network and
//! no external crates.
//!
//! `cave-home-dns-adguard` sits *above* this layer as the filtering / policy
//! brain; this crate is the resolver beneath it. They share no code.
//!
//! # Port method (line-by-line of documented behaviour, ADR-022)
//!
//! Unbound is BSD-licensed. Rather than copy its C source, the config and
//! resolution model here is implemented first-party from Unbound's **public**
//! `unbound.conf(5)` documentation (`local-zone` / `local-data`,
//! `forward-zone` / `stub-zone`, `access-control`, `cache-min/max-ttl`) and the
//! public DNS RFCs. The behaviour matches; the code is original.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`name`] — validated, normalised [`DnsName`] (lower-case, dot-trimmed,
//!   RFC 1035 / 1123 label rules).
//! - [`record`] — [`RecordType`] and [`RecordData`], with A/AAAA parsing via
//!   [`std::net::IpAddr`].
//! - [`local_zone`] — the [`LocalZone`] decision engine for every documented
//!   local-zone type (static / transparent / redirect / refuse / deny /
//!   always-nxdomain).
//! - [`forward`] — forward/stub-zone [`ForwardTable`] longest-suffix routing.
//! - [`access`] — first-party [`Cidr`] containment + [`AccessControl`]
//!   longest-prefix decision.
//! - [`cache`] — a pure TTL [`ResponseCache`] with min/max clamping and
//!   caller-supplied clock.
//! - [`reverse`] — PTR name↔IP mapping.
//! - [`label`] — localised, jargon-free UX (Charter §6.3, ADR-007).
//!
//! The **DNS server transport** (UDP/TCP/DoT/DoH), the real recursive-resolution
//! algorithm (root hints, delegation following, DNSSEC), upstream query I/O and
//! cave-home-core integration are network/crypto-bound and deferred to Phase 1b
//! — each is enumerated in `parity.manifest.toml` `[[unmapped]]` with an
//! ADR-022 disposition. Per Charter §9 there is a **permanent** entry: no cloud
//! DNS.
//!
//! # Example
//!
//! ```
//! use cave_home_dns_unbound::{
//!     DnsName, LocalZone, LocalZoneType, LocalDecision, Record, RecordType,
//! };
//!
//! // A static local zone for the household's own devices.
//! let apex = DnsName::parse("home.arpa").expect("apex");
//! let mut zone = LocalZone::new(apex, LocalZoneType::Static);
//! zone.add(Record::address("printer.home.arpa", RecordType::A, "192.168.1.50").expect("rec"));
//!
//! // A known device resolves locally…
//! let query = DnsName::parse("PRINTER.home.arpa").expect("q"); // case-insensitive
//! match zone.decide(&query, RecordType::A) {
//!     LocalDecision::Answer(recs) => assert_eq!(recs[0].data.to_text(), "192.168.1.50"),
//!     other => panic!("expected a local answer, got {other:?}"),
//! }
//!
//! // …and an unknown name in this authoritative zone is reported as missing,
//! // never leaked to the wider internet.
//! let unknown = DnsName::parse("toaster.home.arpa").expect("q");
//! assert_eq!(zone.decide(&unknown, RecordType::A), LocalDecision::NxDomain);
//! ```

pub mod access;
pub mod cache;
pub mod forward;
pub mod label;
pub mod local_zone;
pub mod name;
pub mod record;
pub mod reverse;

pub use access::{AccessAction, AccessControl, AccessRule, Cidr};
pub use cache::{CacheEntry, ResponseCache, TtlClamp};
pub use forward::{ForwardTable, ForwardZone, RouteKind};
pub use label::{
    Lang, device_not_allowed, local_names_on, name_not_answered, name_points_to,
    outside_lookups_private,
};
pub use local_zone::{LocalDecision, LocalZone, LocalZoneType};
pub use name::{DnsName, NameError};
pub use record::{Record, RecordData, RecordError, RecordType, format_ip, parse_ip};
pub use reverse::{ReverseZone, ptr_name};
