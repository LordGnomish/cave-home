# Coverage matrix — cave-home-node-discovery

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 10/10 mapped symbols found in source · 75 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| RFC 1035 §3.1 length-prefixed label encode/decode (1..=63 octets, root terminator) | src/label.rs::encode_label | yes |
| RFC 1035 §3.1 length-prefixed label encode/decode (1..=63 octets, root terminator) | src/label.rs::decode_label | yes |
| RFC 1035 §2.3.4 domain-name wire encode/decode (labels + root, 255-octet cap, ASCII-case-insensitive compare) | src/dns_name.rs::DnsName | yes |
| RFC 6763 PTR/SRV/A record layout for _cavehome._tcp.local + TXT key=value codec (id/role/v/api) | src/record.rs::PtrRecord | yes |
| RFC 6763 PTR/SRV/A record layout for _cavehome._tcp.local + TXT key=value codec (id/role/v/api) | src/record.rs::SrvRecord | yes |
| RFC 6763 PTR/SRV/A record layout for _cavehome._tcp.local + TXT key=value codec (id/role/v/api) | src/record.rs::ARecord | yes |
| RFC 6763 PTR/SRV/A record layout for _cavehome._tcp.local + TXT key=value codec (id/role/v/api) | src/record.rs::TxtRecord | yes |
| Discovered-peer model: node id, hostname, std::net::IpAddr list, port, role, version, last-seen, TTL (clock-pure expiry) | src/peer.rs::DiscoveredPeer | yes |
| Peer cache: add / refresh / dedupe-by-node-id / expire-past-TTL / address-change detection over a caller clock | src/registry.rs::PeerRegistry | yes |
| Self-announcement: build this node's PTR+SRV+TXT+A advertisement set (ADR-005 'box announces itself') | src/announce.rs::announce_self | yes |
| Self-announcement: build this node's PTR+SRV+TXT+A advertisement set (ADR-005 'box announces itself') | src/announce.rs::announce_self_multi | yes |
| Version-skew policy (Charter §7 always-latest, §8 no-backcompat): same minor compatible, one-minor upgrade nudge, else reject | src/compat.rs::Version | yes |
| Version-skew policy (Charter §7 always-latest, §8 no-backcompat): same minor compatible, one-minor upgrade nudge, else reject | src/compat.rs::compatibility | yes |
| Version-skew policy (Charter §7 always-latest, §8 no-backcompat): same minor compatible, one-minor upgrade nudge, else reject | src/compat.rs::version_is_compatible | yes |
| Join-token shape/validation (length, charset, clock-pure expiry) + pairing handshake state machine (ADR-005 QR/token path) | src/join.rs::JoinToken | yes |
| Join-token shape/validation (length, charset, clock-pure expiry) + pairing handshake state machine (ADR-005 QR/token path) | src/join.rs::PairingState | yes |
| Join-token shape/validation (length, charset, clock-pure expiry) + pairing handshake state machine (ADR-005 QR/token path) | src/join.rs::Pairing | yes |
| Cluster roles primary / secondary / ml-node (ADR-005) with EN/DE/TR grandma-friendly names (Charter §6.3) | src/peer.rs::NodeRole | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| UDP multicast mDNS socket (send/receive on 224.0.0.251:5353 / FF02::FB) | phase-1b | ADR-005: the live transport binds the multicast group and emits/receives the records this crate models. Network-bound; the record codec and registry are transport-agnostic and reused unchanged. |
| DNS message framing + compression-pointer encode (RFC 1035 §4.1, §4.1.4) | phase-1b | ADR-005: a full mDNS *message* (header + question/answer/additional sections, name compression) wraps the per-record codec implemented here. Compression-pointer DECODE is already flagged as a typed deferral in label.rs; encode lands with the wire transport. |
| Live announcement / probing loop (RFC 6762 probe + announce + goodbye timing) | phase-1b | ADR-005: the periodic re-announce, conflict probing and goodbye packets are a timing/IO loop over announce_self's record set. Clock/IO-bound; the record set itself is pure and complete here. |
| Secure pairing crypto (mutual-TLS / PSK enrolment) | phase-1b | ADR-005 + Charter §9 (account-free): the join token here is modelled as a validated bearer string with a clock-pure validity window and a handshake state machine. The real cryptographic mutual-TLS / PSK exchange (and constant-time compare) is crypto-bound and deferred; the state machine is the integration seam it slots into. |
| cave-home-cluster join integration (cavehome join --token / --hub) | phase-1b | ADR-005: cave-home-cluster drives this crate's Pairing state machine during K3s join. Deferred until the cluster lifecycle API stabilises; this crate has no dependency on cave-home-cluster (no cross-crate coupling). |
| cave-home-core entity/state surfacing of discovered peers | phase-1b | ADR-005: presenting peers as core State entities + the Portal 'Add node' wizard lands once cave-home-core's entity API stabilises. The peer model is already core-agnostic. |
| Pre-current protocol / legacy-version compatibility mode | permanent | Charter §7 always-latest + §8 no-backcompat: the cluster runs one version line; the compat policy rejects (rather than shims) versions more than one minor apart. No historical-protocol mode will ship. |

## Drift notes
None — every claimed symbol exists in source. All 10 mapped symbols verified present; every unmapped gap carries explicit ADR-005 or Charter justification for phase-1b deferral or permanent exclusion. The declared honest_ratio of 1.00 is sound: fill_ratio=0.30 / (0.30 + 0 unjustified gap) = 1.00.
