# cave-home-freeathome

Busch-Jaeger **free@home** System Access Point (SysAP) **Local API client** — the
REST + WebSocket transport layer that [`cave-home-free-home`](../cave-home-free-home)
deferred to Phase 1b (ADR-011).

`cave-home-free-home` is the **brain** (pure logic, zero dependencies): it models
the free@home topology, decodes datapoint values and projects channels onto the
grandma-friendly device kinds. This crate is the **nervous system**: it talks to a
real SysAP over its documented local HTTPS API and feeds the brain. It re-uses
free-home's domain types rather than re-implementing them.

```
  SysAP  ──HTTPS REST───▶  rest + model   ─┐
    │                                       ├─▶  state cache  ─▶  core / mqtt bridge
    └────WSS push────────▶  event parser  ──┘
```

## API surface

- REST: `https://<SysAP>/fhapi/v1/api/rest` — `configuration`, `devicelist`,
  `device/{serial}`, `datapoint/{serial}/{channel}/{datapoint}` (GET/PUT).
- WebSocket: `wss://<SysAP>/fhapi/v1/api/ws` — live datapoint / device events.
- Auth: HTTP Basic today; client-certificate / mTLS is modelled (`AuthMethod`)
  and is the next hardening step.
- TLS: secure by default; `ClientConfig::with_insecure_tls(true)` accepts a
  self-signed SysAP certificate on a trusted LAN (REST and WS).

## Four tracks

1. **Backend** — `client` (async reqwest REST + tokio-tungstenite WS with
   backed-off reconnect), `auth`, `config`, `rest`, `model`, `event`, `state`,
   `reconnect`, `device` (the `FreeAtHomeDevice` trait + capability map).
2. **Portal** — `portal::viewmodel`: device tiles, detail views with per-kind
   controls, a sensor filter, and jargon-free EN/DE/TR vocabulary.
3. **CLI** — `cli`: `cave-home-ctl freeathome {list,get,set,watch}` parsing and
   command→REST mapping.
4. **Observability** — `metrics` (Prometheus text exposition) plus
   `observability/grafana-dashboard.json` (6 panels) and
   `observability/alerts.yaml` (4 alerts).

## Integration seams

- **cave-home-core**: `core_bridge::register` writes each device's state and
  attributes into the core `StateMachine` (domain = device-kind tag, object id =
  `freeathome_<serial>_<channel>`), so automations/voice/portal treat free@home
  devices like any other.
- **cave-home-mqtt**: `mqtt_bridge` republishes datapoint updates to
  `cave-home/freeathome/<serial>/state` (retained JSON) plus an availability
  topic.
- **cave-home-binary**: the CLI parser and command→REST mapping are complete and
  tested in-crate; wiring the `freeathome` subcommand into the single binary's
  dispatch is the one remaining integration step (consistent with how the other
  integration subcommands are not yet wired into the thin binary).

## Licence

Apache-2.0. Clean-room against the documented free@home local API; no
upstream GPL firmware/driver code is vendored.
