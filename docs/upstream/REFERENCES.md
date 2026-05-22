# Upstream references

Source-of-truth list of the smart-home OSS upstreams cave-home tracks
or considers. The Charter golden rule (§6) requires *line-by-line
parity* for permissive upstreams; the **clean-room rule** (Charter
§6.1, ADR-002) replaces line-by-line for GPL / EPL upstreams.

The `Port method` column below classifies every upstream per the
matrix in ADR-002.

Last updated: 2026-05-14.

---

## Smart-home upstream stack

| Upstream                                | Licence              | Lang              | cave-home role                                            | Port method                                                                 |
| --------------------------------------- | -------------------- | ----------------- | --------------------------------------------------------- | --------------------------------------------------------------------------- |
| home-assistant/core                     | Apache-2.0           | Python            | Automation engine + integrations + state machine + bus    | line-by-line                                                                |
| Koenkk/zigbee2mqtt                      | GPL-3.0              | Node.js           | Zigbee stack (zigbee-herdsman-class)                      | **clean-room** (Zigbee 3.0 spec + zigbee-herdsman public API docs)          |
| project-chip/connectedhomeip            | Apache-2.0           | C++               | Matter stack                                              | line-by-line                                                                |
| zwave-js/node-zwave-js                  | MIT                  | TypeScript        | Z-Wave stack                                              | line-by-line                                                                |
| eclipse/mosquitto                       | EPL-2.0 + EDL-1.0    | C                 | Embedded MQTT broker                                      | **clean-room** (MQTT 3.1.1 + 5.0 specs — safe path for EPL)                 |
| blakeblackshear/frigate                 | MIT                  | Python            | NVR + object-detection pipeline                           | line-by-line                                                                |
| koush/scrypted                          | Apache-2.0           | TypeScript        | HomeKit accessory / Google Home / Alexa bridges           | line-by-line                                                                |
| esphome/esphome                         | MIT + GPL (mixed)    | C++ / Python      | ESP firmware tools + native API client                    | **hybrid** — MIT parts line-by-line; GPL parts clean-room from public spec  |
| arendst/Tasmota                         | GPL-3.0              | C++               | Tasmota MQTT firmware adapter (MQTT command schema only)  | **clean-room** (Tasmota MQTT command-schema doc)                            |
| ggerganov/whisper.cpp                   | MIT                  | C++               | Local STT                                                 | line-by-line                                                                |
| rhasspy/piper                           | MIT                  | C++               | Local TTS                                                 | line-by-line                                                                |
| owntracks/recorder                      | mixed (per-file)     | Go                | Location tracking                                         | per-file licence audit, then line-by-line where MIT/Apache/permissive       |
| home-assistant/operating-system         | Apache-2.0           | shell / Yocto     | Reference OS build patterns (not a runtime dependency)    | reference only                                                              |

### Orchestration upstreams (ADR-004)

cave-home's orchestration layer is an in-binary, line-by-line Rust
port of K3s. Sub-crates use the `cave-home-X-rs` suffix to mark the
K3s-derived chain (distinct from any same-named crate in Cave
Runtime, which derives from vanilla `kubernetes/kubernetes` — see
Charter §5.1). Concrete-upstream sub-crates (e.g. flannel CNI per
**ADR-008**) drop the `-rs` suffix and name the upstream instead
(`cave-home-cni-flannel`).

| Upstream                            | Licence    | Lang | cave-home role                                              | Port method   |
| ----------------------------------- | ---------- | ---- | ----------------------------------------------------------- | ------------- |
| k3s-io/k3s                          | Apache-2.0 | Go   | Main orchestration umbrella (single-binary K8s pattern)     | line-by-line  |
| containerd/containerd               | Apache-2.0 | Go   | Container runtime                                           | line-by-line  |
| kubernetes/kubernetes (K3s-vendored) | Apache-2.0 | Go   | apiserver / scheduler / controller-manager / kubelet (K3s's vendored copy, not vanilla) | line-by-line |
| k3s-io/kine                         | Apache-2.0 | Go   | etcd-compatible SQLite / Postgres backend (K3s's single-binary trick) | line-by-line |
| flannel-io/flannel                  | Apache-2.0 | Go   | CNI (K3s default; **ADR-008**: line-by-line port into `cave-home-cni-flannel`) | line-by-line  |
| traefik/traefik                     | MIT        | Go   | Ingress (K3s default; optional in cave-home)                | line-by-line  |
| rancher/klipper-lb                  | Apache-2.0 | Go   | ServiceLB                                                   | line-by-line  |
| rancher/local-path-provisioner      | Apache-2.0 | Go   | Storage provisioner                                         | line-by-line  |

### Ecosystem-port upstreams (ADR-009 / 010 / 011)

Three vendor / standard ecosystems are first-class pillars per
Charter §3.1: **UniFi**, **Philips Hue**, **Busch-Jaeger free@home
+ KNX-IP**. Each ADR records the exact derivation chain; the
table below is the source-of-truth list of upstreams those ports
read from.

| Upstream                                              | Licence    | Lang       | cave-home role                                                                    | Port method                                                                          |
| ----------------------------------------------------- | ---------- | ---------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| home-assistant/core — `unifi` integration             | Apache-2.0 | Python     | UniFi Network client (switches / APs / port telemetry) — `cave-home-unifi-network` | line-by-line                                                                         |
| home-assistant/core — `unifiprotect` integration      | Apache-2.0 | Python     | UniFi Protect cameras — `cave-home-unifi-protect`                                  | line-by-line                                                                         |
| home-assistant/core — UniFi Access integration        | Apache-2.0 | Python     | UniFi Access door control — `cave-home-unifi-access`                               | line-by-line                                                                         |
| Ubiquiti public REST + WebSocket                       | Public API | —          | Direct API surface for all four UniFi-* crates                                     | line-by-line (API client)                                                            |
| home-assistant/core — `hue` integration               | Apache-2.0 | Python     | Philips Hue Bridge client — `cave-home-hue`                                        | line-by-line                                                                         |
| Philips Hue API v1 + v2 (developer portal)             | Public API | —          | Hue Bridge wire format for both `cave-home-hue` and `cave-home-hue-bridge-emu`     | line-by-line (client) + spec-based (emulator)                                        |
| diyhue/diyhue                                          | GPL-3.0    | Python     | Reference for the Hue Bridge emulator — **source NOT read**                        | **clean-room** (`cave-home-hue-bridge-emu`)                                          |
| home-assistant/core — `free_at_home` integration      | Apache-2.0 | Python     | Busch-Jaeger free@home REST client — `cave-home-free-home`                         | line-by-line                                                                         |
| `free-at-home` (python lib)                            | Apache-2.0 | Python     | free@home REST surface reference                                                   | line-by-line                                                                         |
| XKNX/xknx                                              | MIT        | Python     | KNX-IP transport — `cave-home-knx` (transport layer)                               | line-by-line                                                                         |
| home-assistant/core — `knx` integration               | Apache-2.0 | Python     | KNX entity / automation integration — `cave-home-knx` (entity layer)               | line-by-line                                                                         |
| knxd/knxd                                              | GPL-3.0    | C++        | KNX gateway daemon reference — **source NOT read**                                 | **clean-room** (`cave-home-knx` gateway-daemon surface, from KNX Association spec)   |
| KNX Association public spec                            | Vendor spec | —         | KNX wire format (publicly readable portion)                                        | spec-based                                                                           |

### Charter v6 category upstreams (ADR-012 .. ADR-031)

Twenty smart-home OSS categories promoted to first-class pillars by
founder wholesale approval 2026-05-15. Licence values are
best-knowledge as of writing and flagged `[ASSUMPTION]` where not
yet legally audited; clean-room markers are normative (Charter §6.1).

| Upstream                                                                            | Licence                                       | Lang        | cave-home role                                                                                 | Port method                                                       | ADR     |
| ----------------------------------------------------------------------------------- | --------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------- |
| home-assistant/core (climate-domain integrations)                                   | Apache-2.0                                    | Python      | HVAC / heat-pump / climate                                                                     | line-by-line                                                      | ADR-012 |
| Open3EClient (Viessmann ViCare)                                                     | MIT [ASSUMPTION]                              | Python      | Viessmann heat-pump REST client                                                                | line-by-line                                                      | ADR-012 |
| home-assistant/core (opensprinkler)                                                 | Apache-2.0                                    | Python      | Irrigation + sprinkler control                                                                 | line-by-line                                                      | ADR-013 |
| home-assistant/core (mbus / modbus)                                                 | Apache-2.0                                    | Python      | Smart water-meter integration                                                                  | line-by-line                                                      | ADR-013 |
| Aircoookie/WLED                                                                     | EUPL-1.2 [ASSUMPTION; treated as copyleft]    | C++         | Addressable LED-strip firmware (reference only)                                                | **clean-room** (source NOT read)                                  | ADR-014 |
| OpenGarage HA integration                                                           | Apache-2.0                                    | Python      | Garage controller                                                                              | line-by-line                                                      | ADR-015 |
| ESPHome cover bindings                                                              | MIT                                           | C++/Python  | ESP-based cover/blind firmware                                                                 | line-by-line                                                      | ADR-015 |
| Somfy RTS protocol                                                                  | Proprietary (public spec / RE write-ups)      | —           | Somfy awning / blind RF protocol                                                               | **clean-room** (public spec only; RE-repos NOT read)              | ADR-015 |
| home-assistant/core (lock-domain: nuki, switchbot, ...)                             | Apache-2.0                                    | Python      | Residential smart locks                                                                        | line-by-line                                                      | ADR-016 |
| Nuki / SwitchBot public REST APIs                                                   | Public API                                    | —           | Lock vendor REST surfaces                                                                      | line-by-line (API client)                                         | ADR-016 |
| Hypfer/Valetudo                                                                     | MIT                                           | JS/Go       | Cloud-free Xiaomi / Roborock / Dreame vacuum firmware                                          | line-by-line                                                      | ADR-017 |
| home-assistant/core (reolink, doorbird, ring RTSP)                                  | Apache-2.0                                    | Python      | Doorbell / intercom integrations                                                               | line-by-line                                                      | ADR-018 |
| home-assistant/core (alarmdecoder, alarm_control_panel)                             | Apache-2.0                                    | Python      | Alarm panel integrations                                                                       | line-by-line                                                      | ADR-018 |
| home-assistant/core (airgradient, awair, airthings, ikea_vindriktning)              | Apache-2.0                                    | Python      | Air-quality sensor integrations                                                                | line-by-line                                                      | ADR-019 |
| music-assistant/server                                                              | Apache-2.0                                    | Python      | Multi-room audio orchestration                                                                 | line-by-line                                                      | ADR-020 |
| badaix/snapcast                                                                     | GPL-3.0                                       | C++         | Multi-room audio sync (reference only)                                                         | **clean-room** (source NOT read; protocol docs only)              | ADR-020 |
| mopidy/mopidy                                                                       | Apache-2.0                                    | Python      | Local music server                                                                             | line-by-line                                                      | ADR-020 |
| binwiederhier/ntfy                                                                  | Apache-2.0                                    | Go          | Self-hosted HTTP push notifications                                                            | line-by-line                                                      | ADR-021 |
| gotify/server                                                                       | MIT                                           | Go          | Self-hosted notification server                                                                | line-by-line                                                      | ADR-021 |
| caronc/apprise                                                                      | MIT                                           | Python      | Multi-destination notification router                                                          | line-by-line                                                      | ADR-021 |
| AdguardTeam/AdGuardHome                                                             | GPL                                           | Go          | DNS ad-blocking + parental controls (reference only)                                           | **clean-room** (source NOT read; admin-API docs only)             | ADR-022 |
| NLnetLabs/unbound                                                                   | BSD-3-Clause                                  | C           | Recursive DNS resolver                                                                         | line-by-line                                                      | ADR-022 |
| influxdata/influxdb (2.x)                                                           | MIT [ASSUMPTION; 2.x pinned]                  | Go          | Time-series sensor history (embedded default)                                                  | line-by-line                                                      | ADR-023 |
| timescale/timescaledb                                                               | Apache-2.0                                    | C           | TimescaleDB (Postgres extension) — opt-in backend                                              | line-by-line (consumed as runtime dep)                            | ADR-023 |
| VictoriaMetrics/VictoriaMetrics                                                     | Apache-2.0                                    | Go          | High-cardinality time-series — opt-in alternative                                              | line-by-line                                                      | ADR-023 |
| MycroftAI/mycroft-core                                                              | Apache-2.0                                    | Python      | Voice framework (historical; superseded by OVOS)                                               | line-by-line                                                      | ADR-024 |
| OpenVoiceOS/ovos-core                                                               | Apache-2.0                                    | Python      | Voice framework continuation                                                                   | line-by-line                                                      | ADR-024 |
| rhasspy/rhasspy                                                                     | MIT                                           | Python      | Offline wake-word + intent routing                                                             | line-by-line                                                      | ADR-024 |
| home-assistant/core (withings, garmin_connect, fitbit, oura)                        | Apache-2.0                                    | Python      | Wellness / health-data integrations                                                            | line-by-line                                                      | ADR-025 |
| grocy/grocy                                                                         | MIT                                           | PHP         | Household management (food / medicine / battery / chores)                                      | line-by-line (behavioural)                                        | ADR-026 |
| Kozea/Radicale                                                                      | GPL-3.0                                       | Python      | CalDAV / CardDAV server (reference only)                                                       | **clean-room** (source NOT read; RFC 4791/6352/5545/6350)         | ADR-027 |
| home-assistant/core (caldav client)                                                 | Apache-2.0                                    | Python      | CalDAV client consuming the cave-home-calendar server                                          | line-by-line                                                      | ADR-027 |
| home-assistant/core (webostv, samsungtv, androidtv, cast)                           | Apache-2.0                                    | Python      | TV / display integrations                                                                      | line-by-line                                                      | ADR-028 |
| home-assistant/core (husqvarna_automower, worx_landroid, weather integrations)      | Apache-2.0                                    | Python      | Garden + weather integrations                                                                  | line-by-line                                                      | ADR-029 |
| home-assistant/core (hayward_omnilogic, pentair_intellicenter)                      | Apache-2.0                                    | Python      | Pool / spa integrations — *deferred to M11+*                                                   | line-by-line (when implemented)                                   | ADR-030 |
| home-assistant/core (eight_sleep, sleep_number)                                     | Apache-2.0                                    | Python      | Sleep-system actuators — *deferred to M11+*                                                    | line-by-line (when implemented)                                   | ADR-031 |

### Solar / energy management upstreams

Tracked separately because the solar pillar is opt-in and arrives in
phases (M3.5 Tier 1 → M5 Tier 2 in ROADMAP.md).

| Upstream                                        | Licence              | Lang   | cave-home role                                                              | Port method                                                          |
| ----------------------------------------------- | -------------------- | ------ | --------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| evcc-io/evcc                                    | Apache-2.0           | Go     | Solar surplus management + EV charge + heat-pump load shifting              | line-by-line                                                         |
| SunSpec Alliance models                         | Public spec          | spec   | Vendor-agnostic inverter monitoring via Modbus (SMA / Fronius / SolarEdge / Huawei / Goodwe / Kostal) | spec-based                                                  |
| Hoymiles HM/HMS/HMT (refs: lumapu/ahoy + OpenDTU) | GPL-3.0 (refs only) | C++    | Hoymiles microinverter support                                              | **clean-room** (Hoymiles wire protocol + AhoyDTU public API docs)    |
| Forecast.Solar / PVGIS                          | Public API           | —      | Solar production forecasting                                                | API client                                                           |

### Notes on the port-method matrix

- **line-by-line.** Used for permissive upstreams (Apache-2.0, MIT,
  BSD). NOTICE / attribution preserved per upstream requirements.
- **clean-room.** Mandated by ADR-002 for GPL / AGPL / EPL upstreams.
  Contributor must not have read upstream source; reimplementation
  works from public spec / RFC / wire-format / public API docs
  only. Test fixtures written from scratch. See CONTRIBUTING.md for
  the contributor protocol.
- **hybrid.** Per-file licence audit; permissive files line-by-line,
  copyleft files clean-room. ESPHome is the present example.
- **reference only.** Not consumed at runtime; used for build / OS /
  packaging patterns. No source porting.

---

## Alternatives considered (do not integrate)

These are listed for users deciding which project fits their needs —
they are alternatives to *building cave-home at all*, evaluated in
ADR-001.

| Project           | Why relevant                                      | Link                                       |
| ----------------- | -------------------------------------------------- | ------------------------------------------ |
| Home Assistant OS | The incumbent OSS smart-home OS                    | https://www.home-assistant.io/             |
| openHAB           | JVM-based OSS smart-home runtime                   | https://www.openhab.org/                   |
| Hubitat Elevation | Closed-source local hub appliance                  | https://hubitat.com/                       |
| Google Home       | Cloud hub                                          | https://home.google.com/                   |
| Amazon Alexa      | Cloud hub                                          | https://alexa.amazon.com/                  |
| SmartThings       | Cloud hub                                          | https://www.smartthings.com/               |

---

## Foundational upstreams

OS-level and tooling upstreams cave-home depends on. The
always-latest mandate (Charter §7) applies.

| Area               | Upstream         | Link                                |
| ------------------ | ---------------- | ----------------------------------- |
| Kernel             | Linux mainline   | https://www.kernel.org/             |
| Init               | systemd          | https://systemd.io/                 |
| Language           | Rust (stable)    | https://www.rust-lang.org/          |
| Database (cand.)   | SQLite           | https://www.sqlite.org/             |
| Inference (cand.)  | ONNX Runtime     | https://onnxruntime.ai/             |
| Media (cand.)      | FFmpeg           | https://ffmpeg.org/                 |
