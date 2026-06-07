# cave-home-unifi — real UniFi transport + API surface (handoff)

**Branch:** `feature/unifi-real-api` (worktree `../cave-home-unifi-real`)
**Date:** 2026-06-07
**Status:** complete, all green, **merged --no-ff to the local integration base, not pushed**

## What this is

The Phase-1b **wire layer** the three UniFi domain cores deferred under ADR-009.
`cave-home-unifi-network`, `-access` and `-protect` each shipped a pure, std-only
decision engine and said the same thing in their crate docs: *"the controller
REST login, the WebSocket event transport and the actual API calls are
network-bound and deferred to Phase 1b; they feed their wire formats onto the
models in this crate and reuse the engine unchanged."*

**This crate is that Phase 1b.** It is a new workspace member, `cave-home-unifi`,
that unifies all three behind one console client over a **real** `reqwest` +
`rustls` transport and a **real** `tokio-tungstenite` WebSocket engine.

## Architecture

```
                 ┌───────────────────────────────────────────────┐
                 │              cave-home-unifi                    │
                 │                                                 │
  Credentials ─► │  auth ─► ConsoleClient<T: HttpTransport> ◄─ Console (UniFi OS │ Legacy)
                 │            │  login + 401 re-auth + metrics      │
                 │            ├─ network::NetworkApi  ──► cave-home-unifi-network
                 │            └─ protect::ProtectApi  ──► cave-home-unifi-protect
                 │                                                 │
  AccessConfig ► │  access::AccessClient<T>  (bearer, :12445) ──► cave-home-unifi-access
                 │                                                 │
                 │  ws::EventPump  ◄─ WsConnector (Tungstenite│Mock)
                 │  metrics::Metrics  (Prometheus)   render::*  (EN/DE/TR)
                 │                                                 │
                 │  transport: HttpTransport seam                  │
                 │     ReqwestTransport (real, self-signed OK)     │
                 │     MockTransport    (offline unit tests)       │
                 └───────────────────────────────────────────────┘
```

The whole stack is built on the `HttpTransport` async seam (mirrors
`cave-home-tesla`). The **real** `ReqwestTransport` accepts the console's
self-signed certificate (every UniFi OS console / Cloud Key ships one; Charter §9
keeps us off the Ubiquiti cloud that would otherwise broker trust). `MockTransport`
makes the auth flow + all three API surfaces unit-testable with zero network, and
the e2e suite swaps in the real transport against `wiremock`.

## Module map (`crates/cave-home-unifi/src`)

| module | what |
|---|---|
| `error.rs` | `UnifiError` + status→variant classification (401→reauth, 429→backoff, 5xx→transient) |
| `transport.rs` | `HttpTransport` seam, real `ReqwestTransport`, `MockTransport` |
| `console.rs` | `Console` — UniFi OS vs Legacy URL/port/prefix routing (REST + WS) |
| `auth.rs` | `Credentials` (password / API key), `Session` (cookie + CSRF, incl. CSRF decoded from the UniFi OS `TOKEN` JWT) |
| `client.rs` | `ConsoleClient` — login, authorized request, **transparent 401 re-auth + retry**, metrics timing |
| `network/` | Network Controller REST: sites, clients, devices, events, health; block/unblock/reconnect/PoE; `execute(&domain Command)` |
| `access/` | Access developer REST (bearer, :12445): doors, visitors, events, **intercom answer**; notification parsing (intercom/doorbell/granted/denied) |
| `protect/` | Protect REST over the console session: bootstrap, cameras, **live RTSPS URL**, events, recordings; 8-byte binary update-packet header |
| `ws.rs` | Real-time engine: per-pillar `WsRequest`, `WsConnection`/`WsConnector` seam, real `TungsteniteConnector` (rustls, self-signed-tolerant, no `unsafe`), `MockWsConnector`, `EventPump` |
| `metrics.rs` | Prometheus exposition (requests/errors/logins/reauth/ws events) |
| `render.rs` | Grandma-friendly EN/DE/TR rendering for the CLI |

## 4-track

1. **Crate** — `cave-home-unifi` (above).
2. **CLI** — `cave-home-cli` now depends on the crate; `main.rs` routes `unifi`
   through `run_matched` (was a no-op stub). New **`cavehomectl unifi live
   <devices|clients|cameras|doors>`** builds a real client from `CAVEHOME_UNIFI_*`
   / `CAVEHOME_ACCESS_*` env and renders live data. Bare `unifi` still prints the
   summary (cross-agent contract preserved).
3. **Portal** — `cave-home-portal::unifi` `/unifi` overview page (pure std-only
   view-model, like `energy.rs`): device/client/camera/door tiles + localised
   summary, fed plain counts by the client.
4. **Metrics** — in-crate `metrics.rs` Prometheus registry, asserted in the e2e.

### `unifi live` environment

```
CAVEHOME_UNIFI_HOST      console IP/host (Network + Protect)
CAVEHOME_UNIFI_API_KEY   API key  (or CAVEHOME_UNIFI_USER + CAVEHOME_UNIFI_PASS)
CAVEHOME_UNIFI_KIND      unifios (default) | legacy
CAVEHOME_UNIFI_SITE      site name (default: default)
CAVEHOME_ACCESS_HOST     UniFi Access appliance host  (for `live doors`)
CAVEHOME_ACCESS_TOKEN    UniFi Access developer API token
```

## Tests

- **114 tests** in `cave-home-unifi` (108 unit + 6 wiremock e2e), all green.
- Lib **clippy clean** (pedantic + nursery), all targets.
- CLI + portal suites green; my additions are clippy-clean (both crates carry
  pre-existing warnings elsewhere that I left untouched).

### Acceptance e2e (`tests/e2e.rs`, real reqwest transport vs wiremock)

- `e2e_login_then_device_list` — login → cookie+CSRF capture → **device list**
  (PoE + uplink mapped to domain).
- `e2e_camera_live_url_from_bootstrap` — bootstrap → **camera live RTSPS URL**
  (`rtsps://host:7441/{alias}?enableSrtp`).
- `e2e_intercom_answer_over_rest` — **intercom** answer via real `PUT` unlock.
- `e2e_intercom_event_via_ws_engine` — the **intercom event** decoded live by the
  real `EventPump` from an `access.remote_view` frame.
- `e2e_block_client_posts_real_command` — `cmd/stamgr` block over the wire.
- `e2e_unauthorized_then_reauth_and_retry` — 401 → re-login → retry, end to end.

## Real vs integration-only

Everything is **real code on the real transport**. The only paths not exercised
by an automated test are the ones that need live hardware:

- `ReqwestTransport` against a real HTTPS console (the e2e drives it over plain
  HTTP because that is the one knob `wiremock` exposes — same code path).
- `TungsteniteConnector` performing an actual WSS handshake (the engine logic is
  covered via `MockWsConnector`; the connector *constructs* in a unit test).

## Provenance / licence

Apache-2.0, first-party clean-room from the **community-documented** UniFi local
API surfaces (Network controller REST, Access developer REST/WS, Protect REST/WS).
No upstream source transcribed; no SHA pinned. Local-only (Charter §9) — no
Ubiquiti-cloud dependency anywhere.

## Follow-ups (not in scope here)

- Protect binary WS payload decode (zlib inflate + JSON) on top of the parsed
  `ProtectPacketHeader` — wire it into `EventPump` for live camera/doorbell state.
- Fold Access `DoorStatus.lock` into a live `AccessController` so the door-safety
  engine reconciles real lock state.
- Network `cmd/stamgr` is wired for block/unblock/reconnect/PoE; the remaining
  `Command` variants (WLAN enable, port-forward, device LED) return a typed
  "not yet wired" error and need their REST calls.
- Portal `/unifi` page is a view-model; wire it into the dashboard router.
