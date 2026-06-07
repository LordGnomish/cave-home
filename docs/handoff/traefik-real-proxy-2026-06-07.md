<!-- SPDX-License-Identifier: Apache-2.0 -->
# Handoff — cave-home-traefik-rs: real reverse-proxy + Ingress runtime

**Date:** 2026-06-07
**Branch:** `feature/traefik-real-proxy` (integration branch `integration/traefik-real-proxy`)
**Base:** `d726871` (`claude/cave-home-k3s-traefik-2026-06-07` — decision core + Ingress/Gateway translation)
**Worktree:** `../cave-home-traefik-proxy` (isolated; the honest-uplift loop owns the shared worktree)

## What this delivered

The crate was a std-only, dependency-free **routing-decision core** (rule parse/
match, router select, load balancer, middleware config, config validation,
Ingress + Gateway-API translation — 107 tests). This branch adds the **real
HTTP/HTTPS reverse-proxy + Kubernetes-Ingress runtime** the core was always
meant to drive, behind a default-on `runtime` Cargo feature so the pure core
still builds with `--no-default-features` (zero deps, 107 tests intact).

### New runtime modules (all TDD, test→fail→impl→pass, paired commits)

| Module | Responsibility |
|---|---|
| `wire` | hyper/`http` ⇄ core `RequestDescriptor`/`ResponseDescriptor` bridge (host/port, header join, cookie, short-circuit parts) |
| `forwarded` | hop-by-hop stripping (fixed set + `Connection`-token list) + `X-Forwarded-*` / `X-Real-Ip` |
| `backend` | upstream request-URI assembly (scheme/authority + rewritten path + query) |
| `circuit` | three-state circuit breaker (Closed/Open/HalfOpen), deterministic recovery clock |
| `retry` | bounded retry, capped exponential backoff, network-only retryability, server rotation |
| `ratelimit` | integer milli-token bucket + per-key `RateLimiter` |
| `auth` | Basic-auth parse + htpasswd verify (`{SHA}`/plaintext, constant-time; bcrypt/apr1 fail closed) + challenge |
| `cors` | preflight detection + allow-origin/credentials resolution + preflight/actual header sets |
| `compress` | `Accept-Encoding` q-value negotiation (gzip/deflate) + precompressed/min-size skip + real flate2 codecs |
| `tls` | rustls PEM load, `self_signed` (rcgen), SNI resolver (exact+wildcard) → `ServerConfig` |
| `acme` | RFC 8555: ES256 account key, JWK + RFC 7638 thumbprint, JWS signing, HTTP-01 key-auth, full order state machine over an `AcmeTransport` seam, renewal threshold |
| `discovery` | `EndpointSlice` → `Server` pool (named/numeric port, ready-only, IPv6) + ClusterIP |
| `controller` | reconcile Ingress → validated `DynamicConfig` + poison-safe `ConfigHolder` hot-swap |
| `metrics` | prometheus-client registry: `traefik_requests_total`, duration histogram, open-connections gauge |
| `dashboard` | serializable status `Snapshot` (routers/services/middlewares) → JSON |
| `server` | **the real listener**: tokio TCP + tokio-rustls TLS + hyper, route→middleware→LB→forward(retry)→metrics |

### 4-track status

- **Code/core:** all of the above.
- **Tests:** 193 lib + 3 integration (`tests/proxy_roundtrip.rs`) + 2 doctests. Strict TDD throughout.
- **Integration:** real loopback HTTP roundtrip through the proxy to an echo
  backend (verifies routing, forwarding, `X-Forwarded-For`, passHostHeader),
  404 on unmatched host, middleware redirect short-circuit. ACME exercised by a
  full mock-issuance test. Ingress→routing via `controller::reconcile`.
- **CLI/portal:** intentionally **absent** — this crate is hidden infra (Charter
  §6.3: the homeowner never sees ingress/router/proxy vocabulary), consistent
  with the other K3s crates (apiserver/scheduler/flannel/traefik).

## Acceptance vs ask

- `cargo test` PASS — 193 lib + 3 integration + 2 doctests, all green.
- Integration HTTP roundtrip via proxy — `proxies_request_to_backend_with_forwarded_headers`.
- Ingress resource → routing rule generation — `controller::tests::reconcile_builds_routable_config`.
- ACME mock test — `acme::tests::mock_issuance_full_flow`.
- TDD compliance — every module landed as a `test(...)` (RED) → `feat(...)` (GREEN) commit pair.
- `clippy --lib` warning-free (the enforced gate); pure core builds `--no-default-features`.
- LOC: ~3.4k new runtime LOC (incl. tests) over the 3.0k decision core. Port is a
  spec-based behavioural reimplementation of Traefik's documented proxy/middleware/
  provider semantics, not a 1:1 Go transliteration, so the LOC is deliberately
  far below upstream's Go surface for the same behaviour.

## Known limitations / next steps (honest)

1. **Body buffering, not streaming.** `server` collects request/response bodies
   into `Bytes` before forwarding. Correct and robust for ingress workloads;
   streaming (`Incoming` passthrough) is the next refinement for large payloads.
2. **ACME production transport.** The order state machine + crypto are real and
   mock-tested; the one remaining I/O adapter is a hyper-backed `AcmeTransport`
   impl (`application/jose+json`) — the seam is defined, just not wired to a
   socket. HTTP-01 token serving also needs an entrypoint route.
3. **Auth schemes.** `{SHA}` + plaintext htpasswd; bcrypt/apr1 fail closed (no
   offline bcrypt crate). Add a bcrypt verifier when a crate is vendored.
4. **Middleware enforcement wiring.** `auth`/`cors`/`compress`/`ratelimit` are
   implemented + unit-tested but not yet invoked inside `server::route_and_forward`
   (which currently applies the path/redirect/header chain). Hooking the typed
   enforcement (inspect `chain.middlewares`) into the request path is a small,
   well-scoped follow-up.
5. **HTTP/2 upstream + h2 serving.** ALPN advertises `h2`; serving is http/1 via
   `hyper::server::conn::http1`. Add `http2` when needed.
6. **Health-check transport.** `Server.healthy` is honoured by the LB; the active
   probe loop that maintains it is not yet implemented.

## How to merge

The base branch is loop-owned in a shared worktree — do **not** check it out
there. This work is on `feature/traefik-real-proxy` with a clean local `--no-ff`
merge on `integration/traefik-real-proxy`. Fast-forward the K3s-traefik branch to
the integration branch once the loop quiesces. No push was performed.

## Build / test

```
# full runtime
cargo test  -p cave-home-traefik-rs
cargo clippy -p cave-home-traefik-rs --lib   # enforced gate: warning-free

# pure decision core (no async deps)
cargo build -p cave-home-traefik-rs --no-default-features
cargo test  -p cave-home-traefik-rs --no-default-features --lib
```
