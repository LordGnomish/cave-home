# cave-home-freeathome — real REST + WebSocket port report

Branch: `feature/freeathome-real-api` (based on the scaffold branch
`claude/cave-home-freeathome-scaffold-2026-06-07`). Strict TDD, no push.

## What this work added on top of the scaffold

The scaffold already carried the real async transport skeleton (reqwest REST,
tokio-tungstenite WebSocket, HTTP Basic auth, event codec, reconnect backoff,
state cache, core/MQTT bridges, metrics, CLI parser, portal viewmodel). This
branch made it **actually talk to a real SysAP** and closed the acceptance gaps:

1. **Real spec compliance for REST datapoint/device paths** *(correctness fix)*.
   The scaffold addressed datapoints as
   `datapoint/{serial}/{channel}/{datapoint}` (slash-separated, no SysAP UUID),
   which does **not** match the fhapi v1 local API and would never reach a real
   device. Verified against the ABB developer portal and the reference client
   [`lion-and-bear/freeathome-local-api-client`](https://github.com/lion-and-bear/freeathome-local-api-client)
   (`src/system-access-point.ts`), the real paths are:
   - `device/{sysApUuid}/{serial}`
   - `datapoint/{sysApUuid}/{serial}.{channel}.{datapoint}` — **dot-separated**
   `RestRequest` now carries the UUID; the client resolves it from the
   `devicelist`/`configuration` response (`SysAp.id`), caches it (shared across
   clones), and honours a `ClientConfig::with_sysap_uuid` override.

2. **Device discovery** (`discovery::discover` + `FreeAtHomeClient::discover`):
   walks the parsed SysAP topology into one typed `Device` per in-scope channel
   (Aktoren + Sensoren), each carrying function, room and last-reported state.
   `Device::primary_value` / `Device::display_state` project the reported-state
   datapoint (`InfoOnOff` / `InfoBlindPosition` / `InfoCurrentTemperature`) onto
   a household-facing token. This is what makes `cavehomectl freeathome
   list-devices` and the portal Devices page real.

3. **`ClientConfig::with_origin`**: a full-origin override (`http://host:port`)
   used by reverse-proxied SysAP deployments and by the mock-server e2e tests
   (WS scheme follows the origin: `http`→`ws`, `https`→`wss`).

4. **wiremock + real-socket WebSocket e2e** (`tests/e2e_sysap.rs`): the real
   client driven over actual sockets against a mock SysAP — see below.

5. **`list-devices` CLI alias** matching the requested command name.

## Spec-compliance matrix (verified against authoritative sources)

| Surface | fhapi v1 reality | This crate |
|---|---|---|
| REST base | `https://<host>/fhapi/v1/api/rest/<route>` | ✅ |
| WS | `wss://<host>/fhapi/v1/api/ws` | ✅ |
| Auth | HTTP Basic | ✅ |
| `GET configuration` | keyed by SysAP UUID | ✅ parsed + UUID cached |
| `GET devicelist` | `{uuid: [serial,…]}` | ✅ |
| `GET device/{uuid}/{serial}` | UUID in path | ✅ (was `device/{serial}`) |
| `GET/PUT datapoint/{uuid}/{serial}.{ch}.{dp}` | UUID + **dots** | ✅ (was slashes, no UUID) |
| WS datapoint key | `{serial}/{ch}/{dp}` — **slashes** | ✅ (correctly differs from REST) |
| Local SysAP UUID | empty GUID `0000…0` | ✅ resolved from response |
| Self-signed cert | SysAP ships one | ✅ `with_insecure_tls` (REST + WS) |
| Reconnect | client retries | ✅ capped exponential backoff |

The slash-vs-dot split is a real free@home quirk: the WS push uses slash-joined
datapoint keys, the REST endpoint uses dot-joined addresses. Both are honoured.

## e2e coverage (`tests/e2e_sysap.rs`, over real sockets)

- `rest_device_list_and_discovery_e2e` — wiremock serves `devicelist` +
  `configuration`; the real reqwest client lists serials, discovers typed
  devices (light/cover with kinds, room, state), and the Basic auth header is
  asserted present on every recorded request.
- `rest_read_and_write_datapoint_e2e` — UUID auto-resolved from `devicelist`,
  then GET/PUT datapoint over the **spec-compliant dotted path**, asserting the
  PUT body.
- `rest_unauthorized_is_auth_error_and_counted` — 401 → `FreeAtHomeError::Auth`
  + `freeathome_auth_failures_total` incremented.
- `websocket_live_event_e2e` — a real `tokio-tungstenite` server accepts the
  client's live subscription and pushes a datapoint frame; the client decodes it
  to a typed event and counts the state change.

## LOC

| | LOC |
|---|---|
| Production code | 2262 |
| Tests (unit + e2e) | 1157 |
| **Total** | **3419** |
| Tests | **114** (110 unit + 4 e2e) |

**Port ratio.** This is a spec-based port (the upstream is the documented fhapi
v1 API + the reference TS client, not a single source file), so a line-for-line
ratio is not meaningful. The relevant ratio is **test:code = 1157:2262 ≈ 0.51**,
and the public API surface covers 100% of the documented Phase-1b transport
endpoints (configuration, devicelist, device, datapoint GET/PUT, WS push) — see
the matrix above.

## Gates

- `cargo test -p cave-home-freeathome` → 114 pass (offline).
- `cargo clippy -p cave-home-freeathome --lib --tests` → zero findings in this
  crate (remaining workspace findings are pre-existing `cave-home-core` baseline,
  untouched here).
- `cargo fmt -p cave-home-freeathome` → clean.

## Not done (deliberate scope boundaries)

- **Binary wiring**: the crate is standalone (in-crate CLI parser + portal
  viewmodel + metrics), not wired into `cave-home-binary`/`cave-home-portal` —
  matching the device-crate pattern and avoiding churn on loop-shared crates.
- **mTLS / client-cert auth**: the `AuthMethod::ClientCert` seam exists but only
  Basic produces a header (SysAP local API is Basic + self-signed TLS).
- **Scene/timer programming API, virtualdevice**: out of Phase-1b transport.
