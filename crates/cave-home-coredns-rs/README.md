<!-- SPDX-License-Identifier: Apache-2.0 -->
# cave-home-coredns-rs

The **embedded cluster-DNS decision core** — the in-process CoreDNS that K3s
runs for Kubernetes service discovery (ADR-004, Orchestration Phase 3).

CoreDNS is a DNS server assembled from a *chain of plugins* configured by a
Caddy-style `Corefile`. This crate is the **decision core** of that server: the
DNS wire protocol and the per-plugin answer logic, implemented as pure,
`std`-only code so they can be tested exhaustively without a socket, a cluster,
a clock, or a crypto backend.

> Upstream: [`coredns/coredns`](https://github.com/coredns/coredns) (Apache-2.0).
> Port method: an **honest behavioural reimplementation** of documented CoreDNS
> plugin semantics and the DNS RFCs — **not** a verbatim line-by-line port. See
> [`parity.manifest.toml`](./parity.manifest.toml).

## What's here

| Module | Responsibility |
| --- | --- |
| `name` | RFC 1035 domain names: labels, 63/255 limits, case-insensitive compare, RFC 4034 canonical ordering |
| `wire` | DNS header/flags + the `Reader`/`Writer` cursors, RFC 1035 §4.1.4 name compression (loop-safe) |
| `rr` | RR types/classes + RDATA codecs (A/AAAA/NS/CNAME/PTR/SOA/MX/TXT/SRV + opaque unknown) |
| `message` | The four-section `Message`, query/reply builders, cross-section compression |
| `plugin` | The Caddy-style chain: `Plugin`/`Next` `NextOrFailure` contract, middleware, fallthrough |
| `hosts` | `hosts` plugin — hostfile A/AAAA + reverse PTR, NODATA/NXDOMAIN, fallthrough |
| `rewrite` | `rewrite` plugin — name/type/class rules, stop/continue, response restore |
| `cache` | `cache` plugin — positive + negative TTL caching, eviction (caller-supplied `now`) |
| `kubernetes` | `kubernetes` plugin — ClusterIP/headless/ExternalName/SRV/pod/reverse-PTR |
| `file` | `file` plugin — authoritative zone (delegation, CNAME chase, wildcards) + master-file parser |
| `forward` | `forward` plugin — upstream policy + `max_fails` health (network is injected) |
| `builtins` | `metrics`/`errors`/`ready` plugins + the `log` line formatter |
| `corefile` | The Caddy-style `Corefile` parser → config AST |
| `arpa` | `in-addr.arpa` / `ip6.arpa` reverse name ⇄ address |

## Example

```rust
use cave_home_coredns_rs::{Chain, Hosts, Message, Name, RecordType};

let chain = Chain::new(vec![Box::new(Hosts::parse("10.0.0.1 web.local"))]);
let reply = chain.handle(&Message::query(Name::parse("web.local").unwrap(), RecordType::A, 1));
assert_eq!(reply.answers.len(), 1);
```

## Charter notes

- **§6.3 — infrastructure.** Cluster DNS is hidden from the homeowner; this
  crate emits **no user-facing strings**. Errors model DNS wire vocabulary
  (`RCODE`s, parse failures), never the Portal.
- **§5.1 — no `cave-home` crate deps.** The `kubernetes` plugin resolves over an
  in-memory snapshot the caller supplies; the live apiserver watch is deferred.
- **Strict isolation.** `cave-runtime` keeps its own `cave-dns`; this crate does
  not reference `cave-runtime`.

## Deferred (see the parity manifest)

Socket transports (UDP/TCP/TLS/QUIC/DoH), the real `forward` network client, the
live Kubernetes API watch, DNSSEC online signing, the Prometheus HTTP exposition,
zone-file watch/reload, AXFR/IXFR, EDNS(0)/OPT, and the `Corefile`→`Chain`
wiring. Each is an I/O or crypto shell around the logic implemented here, with an
ADR-004 phase disposition in `parity.manifest.toml`.
