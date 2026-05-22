# cave-home

> A single-binary, sovereign, privacy-first OSS smart-home hub written in Rust.

**cave-home** unifies the OSS smart-home stack — Home Assistant,
Zigbee2MQTT, Mosquitto, Matter, Z-Wave JS, Frigate, Scrypted, ESPHome,
whisper.cpp, piper — into **one Rust binary** that you install once
and upgrade as one thing. Smart-home only: no NAS, no Plex, no Immich.

It's the part of the "cave-" family that targets **homes that are
already automated** — Zigbee bulbs, Matter locks, IP cameras, ESP
devices — and wants them on a sovereign, OSS, cloud-free box.

> **Independence note.** cave-home is **not** related to or built on
> [Cave Runtime](https://github.com/cave-runtime). The two projects
> share only the `cave-` brand prefix; there is no shared code, no
> shared crates, no path/git dependency, and no shared release cadence.
> See Charter §5.1.

---

## Status

**Pre-alpha — scaffolding only.** Charter is draft, ADR-001 is draft,
ADR-002 (Apache-2.0) is accepted. There is no shipping code yet; the
workspace holds placeholder crates only.

Start with:

- [`docs/CHARTER.md`](docs/CHARTER.md) — what cave-home is and isn't.
- [`docs/adr/ADR-001-cave-home-scope.md`](docs/adr/ADR-001-cave-home-scope.md) — why, and what we considered.
- [`docs/adr/ADR-002-license-decision.md`](docs/adr/ADR-002-license-decision.md) — Apache-2.0 + clean-room mandate for GPL upstreams.
- [`docs/upstream/REFERENCES.md`](docs/upstream/REFERENCES.md) — the upstream stack and per-upstream port method.

---

## Why one more smart-home project?

The OSS smart-home stack today is a constellation: Home Assistant
(Python), Zigbee2MQTT (Node.js), node-zwave-js (TypeScript),
project-chip (C++), Mosquitto (C), Frigate (Python + ffmpeg), Scrypted
(TypeScript), ESPHome (Python + C++), Rhasspy / Assist (Python +
whisper.cpp + piper).

That stack works — until it doesn't. Six runtimes. Three package
managers. Two container supervisors. N add-on update paths. Backups
that cover one piece but not the others. A single "is everything
healthy?" answer that doesn't exist.

cave-home is the bet that **one process, one config, one upgrade,
one backup** is worth a full rewrite. Specifically:

1. **Single Rust binary.** Every stack — broker, Zigbee, Matter,
   Z-Wave, automation engine, camera, voice, portal, CLI — compiles
   into one process. No supervisor, no sidecars.
2. **Privacy-first.** Zero cloud dependency in the critical path.
   No telemetry default-on. STT, TTS, object detection all local.
3. **Sovereign.** Setup, login, recovery work on a network with no
   internet access.
4. **Cave-family discipline.** Line-by-line upstream parity + TDD
   for permissive upstreams; **clean-room reimplementation** from
   public spec for GPL / EPL upstreams (Zigbee2MQTT, Tasmota,
   Mosquitto). No stubs.

---

## Target user

cave-home is built for three overlapping personas:

- **The technical smart-home owner** with Zigbee, Matter, Z-Wave,
  and ESPHome devices, tired of running six containers to make them
  cooperate.
- **The privacy-sensitive family** that wants cameras, voice, and
  presence data to stay on hardware they own.
- **The homelabber / maker** building ESPHome devices and custom
  integrations who wants a fast, scriptable substrate.

Out of scope: enterprises, datacenters, building-management systems.

---

## Pillars (planned)

| Pillar               | Replaces                                              |
| -------------------- | ----------------------------------------------------- |
| Automation engine    | Home Assistant core                                   |
| Zigbee               | Zigbee2MQTT / ZHA                                     |
| Matter               | project-chip / Matter Server                          |
| Z-Wave               | node-zwave-js                                         |
| MQTT broker          | Mosquitto / EMQX (embedded)                           |
| Camera / NVR         | Frigate                                               |
| HomeKit / Google / Alexa | Scrypted                                          |
| Voice                | Rhasspy / Assist (whisper.cpp + piper)                |
| ESPHome adapter      | ESPHome native API                                    |
| Tasmota adapter      | Tasmota MQTT command schema                           |
| Dashboards           | Lovelace (built-in Portal)                            |
| Location             | Owntracks                                             |
| Mobile companion     | Home Assistant Companion                              |

See [`ROADMAP.md`](ROADMAP.md) for the 12-month milestone plan.

---

## Licence

**Apache-2.0** — see [`LICENSE`](LICENSE) and
[`ADR-002`](docs/adr/ADR-002-license-decision.md).

GPL / EPL upstreams (Zigbee2MQTT, Tasmota, Mosquitto, parts of
ESPHome) are reimplemented under a **clean-room** rule from public
spec only, so copyleft does not bleed into the cave-home tree. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the contributor protocol
and [`docs/upstream/REFERENCES.md`](docs/upstream/REFERENCES.md) for
the per-upstream port-method matrix.

---

## Contributing

cave-home is being scaffolded in the open. See
[`CONTRIBUTING.md`](CONTRIBUTING.md). The fastest way to help today
is to read the charter draft and open an issue with feedback.
