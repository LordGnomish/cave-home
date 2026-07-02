# ADR-032 — ESPHome native-API integration (hub-side)

## Status

**Accepted** — 2026-05-31. Phase-1 ships the **native-API wire codec**
(`cave-home-esphome`); the transport, encryption and message bodies are
Phase-1b / Phase-2.

## Context

ESPHome is the most popular way to build a custom ESP32/ESP8266 sensor
or actuator and put it on a home network. cave-home households will have
ESPHome devices, and the natural way to talk to them is the same way Home
Assistant does: the **ESPHome native API**, a small length-prefixed
protobuf protocol on TCP port 6053.

A licensing line has to be drawn. The `esphome/esphome` *firmware* (the
YAML compiler, the on-device component framework, the 500+ on-device
drivers, the on-device web UI / captive portal / OTA) is **GPL-3.0** —
license-incompatible with cave-home's Apache-2.0 (ADR-002). cave-home is
the **hub**, not a device firmware: it needs to *speak* the native API,
not *be* an ESPHome. The native-API wire format itself is a public
protocol — the plaintext frame layout and the `api.proto` message-id
table — and the reference client `aioesphomeapi` is MIT.

## Decision

`cave-home-esphome` — the **pure-logic native-API wire codec**, built as
a behavioural reimplementation of the public wire format. Phase-1 MVP:

- **Frame codec** — the plaintext frame `<0x00> <varint payload-len>
  <varint message-type> <payload>`; encode + streaming-aware decode
  (reports bytes-consumed and an `Incomplete` state for partial buffers).
  A `0x01` preamble (Noise/encrypted) is detected and reported.
- **Varint** — the protobuf base-128 (LEB128) unsigned varint.
- **Message registry** — `MessageType` for the core `api.proto` block,
  ids `1..=29` (handshake, device-info, list-entities, subscribe-states,
  state responses, subscribe-logs).
- **Entity model** — `EntityKind`, `EntityCategory` (0/1/2), `EntityInfo`
  with the FNV-1 `key()` ESPHome derives from an `object_id`.
- **FNV-1 hash** — `fnv1_hash`, the device-side entity-key function.
- **EN/DE/TR labels** — grandma-friendly names per entity kind (ADR-007).

Port method: **behavioural reimplementation** of the public native-API
plaintext wire format + spec-based protobuf varint, cross-checked against
the MIT `aioesphomeapi` client. The GPL-3.0 `esphome/esphome` firmware /
Python codegen was **NOT** read at any point (clean-room recusal,
Charter §6.1 / ADR-002).

### Deferred (enumerated in `parity.manifest.toml`)

- **Phase-1b**: the TCP transport (frames the byte stream off the
  socket); the Noise-encrypted frame helper (the `0x01` path:
  handshake + per-frame ChaCha20-Poly1305).
- **Phase-2**: full protobuf message bodies; the connection state machine
  (Hello → Connect → DeviceInfo → ListEntities → SubscribeStates) +
  reconnection; mDNS discovery of `_esphomelib._tcp`; cave-home-core
  integration (surface ESPHome entities as cave-home devices).

### Permanently out of scope (scope-cut)

The ESPHome **firmware** side — YAML config compiler, on-device component
framework + drivers, on-device OTA / web UI / captive portal. That is the
GPL-3.0 device firmware; cave-home is the hub and does not reimplement it.

## Consequences

### Accepted gains

- A clean, license-safe path to ESPHome devices: the wire engine is real,
  tested and dependency-free, ready for the transport to drive it.
- The framing/registry/key layers are settled, so the Phase-2 message
  bodies and state machine have a stable foundation to fill in.

### Costs / risks

- No live device traffic yet — the transport and message bodies are still
  to come, so the Phase-1 surface is codec-only.
- The Noise (encrypted) path is detected but not handled until Phase-1b;
  devices with an encryption key set cannot yet be reached.

## Provenance

- ESPHome native-API plaintext frame format + `api.proto` message-id
  table — public protocol facts.
- `aioesphomeapi` (MIT) — public client behaviour, reference only.
- `esphome/esphome` firmware + codegen (GPL-3.0) — **NOT read**.
