<!-- SPDX-License-Identifier: Apache-2.0 -->
# cave-home-coredns-rs

The **embedded cluster DNS** — the in-process CoreDNS that K3s runs for
Kubernetes service discovery (ADR-004, Orchestration Phase 3).

CoreDNS is a DNS server assembled from a *chain of plugins* configured by a
Caddy-style `Corefile`. This crate is the **decision core** of that server — the
DNS wire protocol and the per-plugin answer logic, pure `std`-only code testable
without a socket, a cluster, a clock, or a crypto backend — **plus the real
server layer** that runs it: UDP and TCP listeners, the `Corefile`→chain
builder, and the Kubernetes API indexer.

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
| `build` | Lower a `Corefile` server block → live `Chain` in canonical `plugin.cfg` order |
| `server` | Real UDP + TCP listeners (RFC 1035 §4.2), the resolver actor, hot reload |
| `k8s` | Kubernetes API indexer — `ServiceList`/`Endpoints` JSON → populated plugin, `ApiSource` seam |

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
- **§5.1 — no `cave-home` crate deps.** The `kubernetes` plugin resolves over a
  snapshot converted from the API's `ServiceList`/`Endpoints` JSON, fed through
  the `ApiSource` seam; the watched apiserver HTTP client behind that seam is
  deferred.
- **Strict isolation.** `cave-runtime` keeps its own `cave-dns`; this crate does
  not reference `cave-runtime`.

## Running it

```rust,no_run
use std::sync::Arc;
use cave_home_coredns_rs::{Corefile, Resolver, serve_udp, serve_tcp};
use tokio::net::{TcpListener, UdpSocket};

# async fn run() -> std::io::Result<()> {
let block = Corefile::parse(". {\n forward . 1.1.1.1\n}").unwrap().servers.pop().unwrap();
let resolver = Resolver::spawn(block);

let udp = Arc::new(UdpSocket::bind("0.0.0.0:53").await?);
let tcp = TcpListener::bind("0.0.0.0:53").await?;
tokio::spawn(serve_udp(udp, resolver.clone()));
tokio::spawn(serve_tcp(tcp, resolver));
# Ok(()) }
```

## Deferred (see the parity manifest)

Encrypted transports (TLS/DoT, QUIC/DoQ, DoH), the real `forward` network
client, the live Kubernetes API HTTP watch client, DNSSEC online signing, the
Prometheus HTTP exposition, zone-file watch/reload, AXFR/IXFR, and EDNS(0)/OPT.
Each is an I/O or crypto shell around the logic implemented here, with an
ADR-004 phase disposition in `parity.manifest.toml`. (The plaintext UDP/TCP
listeners, the `Corefile`→`Chain` wiring, and the Kubernetes API conversion are
**no longer** deferred — they live in `server`, `build`, and `k8s`.)
