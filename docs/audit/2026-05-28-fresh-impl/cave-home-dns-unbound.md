# Coverage matrix — cave-home-dns-unbound

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method spec-based (unbound.conf(5) documented behaviour + DNS RFCs).
**Verified:** 8/8 mapped symbols found in source · 46 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| RFC 1035/1123 name syntax (1–63-octet labels, ≤253-octet name, case/trailing-dot normalisation) | src/name.rs::DnsName | yes |
| DNS record-type registry (A/AAAA/CNAME/MX/TXT/PTR/SRV/NS/SOA) + address parsing | src/record.rs::RecordType, RecordData | yes |
| Local-zone type semantics (static/transparent/redirect/refuse/deny/always_nxdomain decision) | src/local_zone.rs::LocalZone::decide | yes |
| Forward/stub-zone longest-suffix upstream routing | src/forward.rs::ForwardTable::route | yes |
| Access-control CIDR containment + longest-prefix decision | src/access.rs::Cidr, AccessControl | yes |
| TTL caching with cache-min/max-ttl clamping (RFC 2181 §8) | src/cache.rs::ResponseCache | yes |
| RFC 1035 §3.5 in-addr.arpa + RFC 3596 §2.5 ip6.arpa reverse (PTR) mapping | src/reverse.rs::ptr_name, ReverseZone | yes |
| Grandma-friendly EN/DE/TR UX (Charter §6.3, ADR-007) — jargon-free phrasing | src/label.rs (module) | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| DNS server transport (UDP/TCP + DoT/DoH) | phase-1b | Network/TLS-bound wire listener; feeds parsed (name,type) into this decision core. No decision logic. |
| Real recursive-resolution algorithm (root hints, delegation, glue, QNAME minimisation) | phase-1b | Iterative recursion against root + delegation chain is network-bound I/O. Phase 1 routes passthrough decision to chosen upstream (forward.rs). |
| DNSSEC validation (signature chain, trust anchors) | phase-1b | Validating RRSIG/DNSKEY chains is crypto-bound, depends on wire-format records from transport layer. |
| Upstream query I/O + failover / EDNS | phase-1b | Socket I/O to routed upstream. forward.rs decides which upstreams a query routes to; actually sending and timing out is deferred. |
| cave-home-core entity/state integration | phase-1b | Resolver status + per-device policy lands when cave-home-core's entity API stabilises. Decision core is core-agnostic. |

## Drift notes
None — every claimed symbol exists in source. Manifest carries ADR-022 notation: all 5 unmapped items are Phase 1b (network/crypto-bound) or depend on core stabilisation. fill_ratio 0.30 reflects decision core + local-zone engine + forward/stub routing + access-control + TTL cache + PTR mapping + UX; wire server, recursive algorithm, DNSSEC, and upstream I/O remain (all justified). Honest ratio 1.00 is supported: 0.30 / (0.30 + 0 unjustified gap) = 100% declared capability vs. unjustified missing capability.
