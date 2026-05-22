# ADR-001 — cave-home scope and positioning

## Status

**Draft** — pending review and approval by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

The OSS smart-home stack is fragmented. A serious home today runs:

- **Home Assistant** (Python) as the automation engine.
- **Zigbee2MQTT** (Node.js) or ZHA for Zigbee.
- **node-zwave-js** (TypeScript) for Z-Wave.
- **Matter / project-chip** (C++) for Matter devices.
- **Mosquitto** (C) as the MQTT broker.
- **Frigate** (Python + ffmpeg + Coral / GPU) for cameras.
- **Scrypted** (TypeScript) to bridge HomeKit / Google Home.
- **ESPHome** (Python + C++) to flash and talk to ESP devices.
- **Rhasspy / Assist** (Python + whisper.cpp + piper) for voice.

That's six runtimes (Python, Node, TS, C++, Go, shell), three package
managers, two container supervisors, and N add-on update paths.
Installation is a multi-evening project. Upgrades break in surprising
places. Backup/restore covers Home Assistant config but not Z2M state,
not Frigate clips, not the Mosquitto retained set. There is no single
"smart-home is healthy" signal.

This pain is real, well-documented in the HA / r/homeassistant
communities, and not solved by any incumbent.

### What "good" looks like

A smart-home hub is "good" when, for a competent owner:

1. It installs from a single image and is automating within an
   afternoon.
2. It speaks all the major device protocols (Zigbee, Matter, Z-Wave,
   MQTT, ESPHome, Tasmota, BLE) without a separate add-on per
   protocol.
3. It does object-detection NVR and local voice **on the same box**,
   not on a sidecar.
4. Backup, restore, and atomic upgrade cover **everything**: config,
   history DB, broker state, camera recordings, automations.
5. It does not depend on any cloud account.

No existing OSS project hits all five at once today.

## Decision

We will build **cave-home** as a new, standalone OSS project: a
**single unified Rust binary** that is a Rust reimplementation of the
smart-home OSS stack, sovereign and privacy-first, with scope tightly
bounded to *smart-home only* (automation, devices, voice, NVR,
dashboards) — explicitly excluding NAS / media-server / photo-backup /
app-catalogue / VPN, which belong to entirely different projects.

Concretely:

1. **Single unified Rust binary.** All stacks (broker, Zigbee,
   Matter, Z-Wave, automation, camera, voice, portal, CLI) compile
   into one process. Cave Runtime's "consolidated binary" pattern
   applied to the smart-home stack.
2. **Smart-home scope only.** The Charter §3 list is the entire
   surface. NAS / media / photos / catalogue / VPN are explicitly
   out (Charter §4).
3. **Privacy-first.** Stricter than the Cave Runtime sovereignty
   rule because smart-home data is more sensitive (presence,
   voice, video).
4. **Cave-family discipline.** Line-by-line upstream parity, TDD
   strict, always-latest, no backcompat.
5. **Fully independent of Cave Runtime.** No code sharing, no crate
   reuse, no path/git dependency. The two projects share only the
   `cave-` brand prefix.
6. **Pre-alpha.** This ADR commits us to the *direction*, not a
   release date.

## Alternatives considered

### Home Assistant OS + add-ons (the incumbent stack)

- **Pro:** Largest ecosystem in OSS smart-home. ~2000 integrations.
  Mature community, huge device catalogue.
- **Con / why not:** Python performance ceiling on automation
  evaluation, esp. with templated automations. Supervisor / add-on
  architecture spawns containers per protocol — Z2M, Matter Server,
  Frigate, MQTT all separate. Backup covers HA config but not the
  full stack. Add-on update cadence is unsynchronised; one upgrade
  routinely breaks another. Apache-licensed core, but the dependency
  matrix is enormous and inconsistently licensed.
- **Use case if you don't pick cave-home:** still the right pick
  today for breadth-of-integrations. cave-home explicitly does not
  try to match the 2000-integration count in M1 — see ROADMAP.

### openHAB

- **Pro:** Long-standing, OSS, Java/Kotlin runtime.
- **Con / why not:** Community has been shrinking for years, device
  support lags HA significantly, JVM footprint is heavy for small
  boxes. Less momentum on Matter and modern voice.
- **Use case if you don't pick cave-home:** primarily JVM-house
  homelabs.

### Hubitat Elevation

- **Pro:** Polished UX, runs local (no cloud required for core
  automation), strong Z-Wave / Zigbee out of the box.
- **Con / why not:** Closed-source, hardware-locked appliance.
  Fails Charter §9 (privacy-first / OSS-first) by construction.

### Cloud hubs (Google Home, Amazon Alexa, Samsung SmartThings)

- **Pro:** Fastest setup, broad device compatibility.
- **Con / why not:** Cloud-dependent by design. Smart-home data
  flows through someone else's account. Periodic device deprecations
  forced by upstream. The opposite of cave-home's mission.

### "Just write better HA add-ons"

- **Pro:** Lowest project cost — keep HA as engine, write better
  Z2M / Frigate replacements as add-ons.
- **Con / why not:** Does not solve the multi-runtime, multi-supervisor,
  multi-backup problem. The integration mess is structural, not
  per-add-on. Charter §5 (single unified binary) is the deliberate
  answer to this.

### Licence decision (resolved in ADR-002)

The smart-home upstream landscape mixes licences (Apache-2.0, MIT,
EPL-2.0, GPL-3.0). The licence call cave-home itself adopts
constrains, per upstream, whether we may line-by-line port or must
reimplement from spec only.

**Licence decision recorded in ADR-002, 2026-05-14: cave-home is
Apache-2.0, with a clean-room reimplementation mandate for every GPL
/ AGPL / EPL upstream.** The per-upstream port-method matrix lives
in `docs/upstream/REFERENCES.md`; the contributor protocol lives in
CONTRIBUTING.md; Charter §6.1 carries the rule itself.

## Consequences

### Accepted costs

- **Massive scope.** Home Assistant core alone is ~1M LOC + ~2000
  integrations. Z2M, Matter, Z-Wave JS are each substantial.
  cave-home in M1–M5 will *not* match HA's integration count; the
  ROADMAP starts with MQTT + minimal devices and grows protocol by
  protocol.
- **GPL upstream complexity.** Per ADR-002, Zigbee2MQTT and Tasmota
  cannot be ported line-by-line; they are clean-room reimplemented
  from public spec. This shapes who can contribute to those crates
  (see CONTRIBUTING.md "clean-room rule").
- **Duplication with Cave Runtime.** Because cave-home does not reuse
  Cave Runtime crates, primitives like MQTT, auth, and observability
  exist in both trees. This is a deliberate decision (Charter §5.1)
  and accepted as the cost of independent evolution.
- **Grandma-friendly UX mandate covers every pillar.** ADR-007
  (Accepted 2026-05-14) elevated the no-technical-leakage UX rule
  to Charter §6.3. Every pillar in §3 is in scope for that rule —
  no implementation choice (MQTT broker shape, Zigbee join flow,
  K3s pod surface) may breach the mandate even if it's technically
  the most natural path.

### Accepted gains

- **One binary, one upgrade, one backup.** The structural problem
  the alternatives can't solve.
- **Performance ceiling raised.** Rust automation evaluation and
  hot-path code are not bound by Python's GIL or Node's event loop.
- **Privacy story is clean.** No add-on container has a side channel
  to the internet; the whole binary is one trust boundary.
- **Independent evolution.** cave-home's tempo is **not** coupled to
  Cave Runtime's. Cave Runtime's 2026-05-21 OSS launch is **not**
  binding on cave-home. Licence and roadmap decisions on either side
  do not bleed across.
- **Licence cleanliness across the family.** cave-home's licence
  choice does not constrain Cave Runtime's choice and vice versa.

### Follow-on ADRs explicitly required

- **ADR-002 — Licence.** *(Accepted 2026-05-14: Apache-2.0 +
  clean-room mandate for GPL / EPL upstreams. See
  `docs/adr/ADR-002-license-decision.md`.)*
- **ADR-003 — Linux 7.1+ kernel floor + no backcompat.**
  *(Accepted 2026-05-14: see
  `docs/adr/ADR-003-linux-71-no-backcompat.md`.)*
- **ADR-004 — Orchestration layer.** *(Accepted 2026-05-14:
  K3s line-by-line Rust port inside the unified binary. See
  `docs/adr/ADR-004-orchestration-layer.md`.)*
- **ADR-005 — Deployment topology / multi-node bootstrap.**
  *(Accepted 2026-05-14: Hybrid — OS image + CLI + Portal
  wizard. See `docs/adr/ADR-005-deployment-topology.md`.)*
- **ADR-006 — Mobile companion app stack.** *(Draft: Tauri /
  Flutter / RN / KMM under discussion; Flutter currently flagged
  as recommended in light of ADR-007. See
  `docs/adr/ADR-006-mobile-app-stack.md`.)*
- **ADR-007 — Grandma-friendly UX mandate.** *(Accepted
  2026-05-14: every UI surface uses home-world vocabulary;
  technical stack hidden behind a Settings → "Developer view"
  toggle. See `docs/adr/ADR-007-grandma-friendly-ux.md` and
  `docs/ui-language.md`.)*
- **ADR-008 — CNI choice.** *(Accepted 2026-05-14: flannel
  line-by-line port in `cave-home-cni-flannel`; Cilium deferred
  to a future opt-in ADR. See `docs/adr/ADR-008-cni-flannel.md`.)*
- **ADR-009 — UniFi (Ubiquiti) ecosystem port.** *(Accepted
  2026-05-15: four-crate line-by-line port of HA UniFi-*
  integrations — Network / Protect / Access / Talk. See
  `docs/adr/ADR-009-unifi-ecosystem-port.md`.)*
- **ADR-010 — Philips Hue port + Hue-Bridge emulator.**
  *(Accepted 2026-05-15: `cave-home-hue` line-by-line from HA
  Hue integration; `cave-home-hue-bridge-emu` clean-room from
  Philips developer-portal docs — diyhue GPL not read. See
  `docs/adr/ADR-010-philips-hue-port.md`.)*
- **ADR-011 — Busch-Jaeger free@home + KNX-IP port.**
  *(Accepted 2026-05-15: `cave-home-free-home` line-by-line;
  `cave-home-knx` mixed — xknx line-by-line, HA `knx`
  line-by-line, KNXd clean-room. See
  `docs/adr/ADR-011-free-home-knx-port.md`.)*

### Charter v6 expansion (ADR-012 .. ADR-031, all Accepted 2026-05-15)

Founder wholesale approval 2026-05-15 promoted twenty smart-home
OSS categories to first-class pillars (Charter §3.2). Each carries
its own ADR with port-method classification.

- **ADR-012 — HVAC / heat-pump / climate.** `cave-home-hvac`,
  line-by-line port of HA climate-domain integrations
  (Viessmann Open3EClient, Daikin, LG, Bosch, Mitsubishi,
  Samsung). See `docs/adr/ADR-012-hvac.md`.
- **ADR-013 — Water / irrigation / leak detection.**
  `cave-home-water`, line-by-line port of OpenSprinkler + HA
  M-Bus / Modbus water-meter integrations. See
  `docs/adr/ADR-013-water-irrigation.md`.
- **ADR-014 — Lighting (WLED).** `cave-home-lighting-wled`,
  **clean-room** from WLED JSON API + UDP realtime public docs;
  WLED source NOT read. See `docs/adr/ADR-014-lighting-wled.md`.
- **ADR-015 — Cover / garage / awning.** `cave-home-cover`,
  hybrid port — OpenGarage line-by-line, ESPHome bindings
  line-by-line, Somfy RTS **clean-room**. See
  `docs/adr/ADR-015-cover-garage.md`.
- **ADR-016 — Smart-lock integration.** `cave-home-lock`,
  line-by-line port of Nuki + SwitchBot + ESPHome lock
  components + Z-Wave / Zigbee / Matter lock-domain bindings.
  See `docs/adr/ADR-016-lock.md`.
- **ADR-017 — Robot vacuum (Valetudo).** `cave-home-vacuum`,
  line-by-line port of Valetudo + HA vacuum-domain
  integrations. See `docs/adr/ADR-017-vacuum.md`.
- **ADR-018 — Doorbell + alarm panel.** `cave-home-doorbell` +
  `cave-home-alarm`, line-by-line port of HA doorbell
  integrations (Reolink, DoorBird, Ring RTSP-only) + alarm
  panel integrations (AlarmDecoder, Bosch, ELK-M1). See
  `docs/adr/ADR-018-doorbell-alarm.md`.
- **ADR-019 — Air-quality sensors.** `cave-home-air-quality`,
  line-by-line port of HA AirGradient + Awair + IKEA
  Vindriktning + Airthings integrations. See
  `docs/adr/ADR-019-air-quality.md`.
- **ADR-020 — Multi-room audio.** `cave-home-audio-mass` +
  `cave-home-audio-snapcast` (**clean-room**) +
  `cave-home-audio-mopidy`. Music Assistant + Snapcast +
  Mopidy. See `docs/adr/ADR-020-multi-room-audio.md`.
- **ADR-021 — Notification stack.** `cave-home-notify`,
  line-by-line port of ntfy + gotify + Apprise. See
  `docs/adr/ADR-021-notification.md`.
- **ADR-022 — DNS + ad-blocking.** `cave-home-dns-adguard`
  (**clean-room**) + `cave-home-dns-unbound` (line-by-line).
  See `docs/adr/ADR-022-dns-adguard.md`.
- **ADR-023 — Time-series history database.**
  `cave-home-history`, line-by-line port of InfluxDB 2.0
  default + TimescaleDB + VictoriaMetrics opt-in. See
  `docs/adr/ADR-023-history-database.md`.
- **ADR-024 — Voice framework expansion.** *(expands existing
  `cave-home-voice` crate)* — line-by-line port of Mycroft / OVOS
  + Rhasspy on top of existing whisper.cpp + piper. See
  `docs/adr/ADR-024-voice-framework-expansion.md`.
- **ADR-025 — Wellness / health integrations.**
  `cave-home-wellness`, line-by-line port of HA Withings +
  Garmin + Fitbit + Oura integrations. See
  `docs/adr/ADR-025-wellness.md`.
- **ADR-026 — Household management (Grocy port).**
  `cave-home-household`, line-by-line port of Grocy (MIT).
  See `docs/adr/ADR-026-household-grocy.md`.
- **ADR-027 — Calendar / PIM (Radicale clean-room).**
  `cave-home-calendar`, **clean-room** CalDAV/CardDAV server
  from RFC 4791 + 6352 + 5545 + 6350; Radicale source NOT
  read. See `docs/adr/ADR-027-calendar-radicale.md`.
- **ADR-028 — TV / display integration.** `cave-home-display`,
  line-by-line port of HA webOS + Tizen + Android TV
  integrations + first-party Portal wall-panel mode. See
  `docs/adr/ADR-028-display.md`.
- **ADR-029 — Garden / outdoor.** `cave-home-garden`,
  line-by-line port of HA Husqvarna Automower + Worx Landroid
  + weather-station integrations. See
  `docs/adr/ADR-029-garden.md`.
- **ADR-030 — Pool / spa** *(deferred to M11+; scaffold only)*.
  `cave-home-pool` placeholder. See
  `docs/adr/ADR-030-pool-spa.md`.
- **ADR-031 — Wearable / sleep** *(deferred to M11+; scaffold
  only)*. `cave-home-wearable` placeholder. See
  `docs/adr/ADR-031-wearable-sleep.md`.

### Still-planned ADRs (renumbered to ADR-032+)

- **ADR-032 — Reference hardware profile.** Concrete BoM for v0.1.
- **ADR-033 — Camera inference backend.** Coral, NVIDIA, OpenVINO,
  pure-CPU; how the camera pillar negotiates accelerators.
- **ADR-034 — Update / rollback model.** Atomic, snapshot-aware
  updates with automatic rollback on health-check failure.

*(Previously-planned ADR-013 "History database" is now ADR-023
above; ADR-015 "Voice stack" is now ADR-024 above.)*

## Open questions to clarify with the founder

1. **Hardware floor.** Is "x86_64 or modern ARM64 with ≥ 8 GB RAM"
   the right reference? Camera + voice pillars push this higher; do
   we define a *tiered* reference (hub-only / hub+voice / hub+NVR)?
2. **Scope edge — energy management.** Energy dashboards (HA Energy)
   are popular and tightly coupled to automations. In scope as part
   of the automation engine, or a §3 pillar in its own right?
3. **MVP modules for M1.** Roadmap currently lists MQTT broker +
   core event bus + minimal MQTT integrations + Portal. Is that the
   right opening hand, or do we start with Zigbee integration to
   stress the line-by-line / clean-room discipline against a hard
   target from day one?
4. **Mobile app strategy.** *(Open question moved to ADR-006.)*
5. **Charter completion criteria.** Charter §10 is a placeholder —
   define what "v0.1 done" means concretely.
