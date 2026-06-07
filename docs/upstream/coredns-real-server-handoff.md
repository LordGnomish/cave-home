# CoreDNS real-server layer — handoff

Port of the **live DNS server** on top of the `cave-home-coredns-rs`
decision core: the UDP/TCP listeners, the `Corefile`→chain builder, hot
reload, and the Kubernetes API indexer. Upstream:
[`coredns/coredns`](https://github.com/coredns/coredns) `core/dnsserver` +
`plugin/kubernetes` (Apache-2.0).

Last updated: 2026-06-07. Branch: `feature/coredns-real-server` (based on
`claude/cave-home-k3s-coredns-2026-06-07` HEAD `e216e98`, where the decision
core lives — **not** `main`). Built in the isolated worktree
`../cave-home-coredns-server`. Local `--no-ff` merge only; **no push**.

---

## What this branch added

Before: the crate was a pure, `std`-only **decision core** — wire codec,
plugin chain, and the per-plugin answer logic (hosts/rewrite/cache/kubernetes/
file/forward/errors/ready/metrics) + the `Corefile` parser. 125 tests, no
socket, no chain assembly, no API integration.

After: the same core **plus a real server**.

| Module | Upstream analogue | What it does |
| --- | --- | --- |
| `src/build.rs` | `core/dnsserver/{register,zdirectives}.go` | `DIRECTIVES` = the canonical `plugin.cfg` priority order (ported verbatim); `build_chain`/`build_chain_with` sort a `ServerBlock`'s directives by that order and instantiate each supported plugin. |
| `src/server.rs` | `core/dnsserver/server_{udp,tcp}.go` + `reload` | `Resolver` actor (owns the `!Send` chain on a dedicated current-thread runtime), `serve_udp` (RFC 1035 §4.2.1 + TC truncation), `serve_tcp` (RFC 1035 §4.2.2 / RFC 7766 length framing), `reload` + `update_endpoints`. |
| `src/k8s.rs` | `plugin/kubernetes/{controller,object}.go` | `kubernetes_from_api` converts `ServiceList`/`Endpoints` JSON → populated `Kubernetes` plugin (ToService/ToEndpoints); `ApiSource` transport seam + `StaticSource`; `K8sSnapshot` carries watch data into the resolver. |
| `tests/dns_server.rs` | — | End-to-end: cluster A/SRV resolution over real UDP & TCP sockets, NXDOMAIN for misses. |

Stats: **+1689 LOC** across the four files, **37 new tests** (11 build + 11
server + 10 k8s + 5 e2e). Crate total 125 → **163 tests** (157 lib + 5
integration + 1 doctest), all green. `fill_ratio` 0.35 → **0.45**,
`honest_ratio` **1.00**. New deps: `tokio` (net/rt/sync/io-util), `serde`,
`serde_json` (all resolve offline).

---

## The one non-obvious design decision

**The plugin chain is `!Send`.** Plugins carry interior-mutable counters
(`Cell`/`RefCell` in `Metrics`/`Errors`/`Ready`), and `plugin.rs` has a blanket
`impl Plugin for Rc<P>` — so `Box<dyn Plugin>` is neither `Send` nor `Sync`, and
the decision core is frozen (don't add `Send` bounds to the trait).

Consequence: the chain cannot be shared across worker threads, nor even *moved*
to another thread. The server therefore uses an **actor**:

- `Resolver::spawn(block)` starts a dedicated OS thread running its own
  current-thread tokio runtime. That thread **builds** the chain (so the
  `!Send` value is never moved across a thread boundary) and owns it forever.
- Listeners (`serve_udp`/`serve_tcp`) hold a cloneable `Resolver` handle (an
  `mpsc::Sender`, which *is* `Send`/`Sync`) and send decoded `Message`s +
  `oneshot` reply channels. They `tokio::spawn` freely on any runtime.
- The actor processes requests sequentially. `Chain::handle` is microseconds of
  pure CPU, so this is fine; it also gives reload a natural home (swap the chain
  between queries).

**Reload / live K8s data also rides this seam.** A `!Send` chain can't cross the
channel, so the actor instead keeps the *inputs* — the `ServerBlock` and the
latest `K8sSnapshot` (both plain `Send` data) — and **rebuilds** the chain when
either changes. `reload(block)` and `update_endpoints(snapshot)` are messages
that trigger a `build_chain_with` on the actor thread. For our immutable-plugin
design, "apply a watch update" == "rebuild the chain with the new snapshot".

---

## Seams for the next agent (still deferred, with ADR-004 disposition)

1. **The live K8s watch client.** `k8s::ApiSource` is the trait; `StaticSource`
   is the test impl. The real impl is a watched HTTP/2 streaming client against
   the apiserver that produces the `ServiceList`/`Endpoints`/`watch` documents,
   then calls `Resolver::update_endpoints` on each delta. Belongs with a
   `cave-home-apiserver-rs` integration — this crate stays apiserver-agnostic
   (Charter §5.1, no `cave-home` crate deps). A poll loop calling
   `list_services`/`list_endpoints` → `kubernetes_from_source` → `K8sSnapshot`
   is a valid first cut before true watch.
2. **Encrypted transports** (TLS/DoT, QUIC/DoQ, DoH). Additional framing over
   the same `Resolver`; no new answer logic.
3. **`forward` network client.** `forward::Forward` already takes the upstream
   exchange as an injected `Transport`; wire a real UDP/TCP client into it.
4. **A config-watch loop** that calls `Resolver::reload` on `Corefile` change
   (inotify / periodic poll). Reload itself is done.
5. **EDNS(0)/OPT** — the UDP path uses the classic 512-octet limit; OPT
   buffer-size negotiation lands with the transport that needs it.

Builder coverage today: `errors`, `ready`, `prometheus`, `cache`, `rewrite`,
`hosts`, `kubernetes`, `file`, `forward`. A known-but-unimplemented directive
(`dnssec`, `tls`, …) is a loud `WireError::Config`, never a silent drop.

---

## How to verify

```sh
cd ../cave-home-coredns-server
cargo test  -p cave-home-coredns-rs            # 157 lib + 5 e2e + 1 doctest
cargo clippy -p cave-home-coredns-rs --lib     # 0 warnings (the gate)
cargo doc    -p cave-home-coredns-rs --no-deps # 0 warnings
```

The e2e (`tests/dns_server.rs`) binds ephemeral loopback sockets and resolves
`web.default.svc.cluster.local` over both UDP and TCP through one shared
resolver fed a `K8sSnapshot` — the exact path a K3s node exercises.
