# cave-home — Charter (DRAFT)

**Status:** Draft — pending review by Burak Tartan (founder).
**Created:** 2026-05-14
**Last updated:** 2026-05-14

> This document is the short version of "what cave-home is and isn't".
> When something else in the repo disagrees with the charter, the
> charter wins until it's amended via an ADR.

---

## 1. Vision

> **The safest, most privacy-respecting, single unified Rust binary
> OSS smart-home hub running on a bare-metal multi-node home-server
> cluster — and the most comprehensive sovereign hub in the OSS
> smart-home world, unifying Home Assistant + Zigbee2MQTT +
> Mosquitto + Matter + Z-Wave JS + Frigate + Scrypted + ESPHome +
> whisper.cpp + piper + UniFi + Hue + Busch-Jaeger free@home / KNX
> + EVCC + SunSpec + Valetudo + Music Assistant + ntfy + AdGuard +
> InfluxDB + Mycroft / Rhasspy / OVOS + Grocy + Radicale into one
> Rust binary.**

One process per node, one config, one upgrade path, one cluster.
No Python supervisor, no half-dozen add-on containers, no cloud
account. Smart-home only (broadly construed: §3 lists the
pillars; §4 lists what stays out) — **no NAS, no Plex, no
Immich**.

## 2. Target users

cave-home's **first-rank** target user is the **non-technical home
resident / family member**. Grandma adds a device in five minutes;
the kid runs the "Evening" scene; the spouse checks a camera from
the kitchen — none of those interactions require knowledge of K3s,
Kubernetes, containers, MQTT topics, or Zigbee channels (Charter
§6.3 grandma-friendly UX mandate, ADR-007).

The **second-rank** personas are the power users who need an
escape hatch. They get one via the Portal **"Developer view"**
toggle (Settings) — off by default, hidden from the mobile app
entirely.

cave-home is built for the following overlapping personas, in
priority order:

1. **The non-technical family member.** The headline persona.
   Wants the home to "just work" — voice control, scenes,
   notifications when a door opens, a camera feed on the phone
   when the doorbell rings. Will never look at a manifest or a
   container log.
2. **The privacy-sensitive family head.** Wants the camera /
   voice / presence data to stay on hardware they own. Reads
   release notes; not willing to depend on Nest / Alexa /
   SmartThings cloud. Cares that recovery + backups work
   without filing a support ticket.
3. **The technical smart-home owner.** Has (or is about to have)
   Zigbee, Matter, Z-Wave, ESPHome, and a couple of IP cameras.
   Tired of running six containers to make them cooperate.
   *Primary power user: Burak Tartan, Iphofen / Germany.*
4. **The homelabber / maker.** Builds ESPHome devices, writes
   custom integrations, hand-rolls automations. Wants a fast,
   scriptable substrate to extend, not a black box. Uses the
   CLI (ADR-005) and the Developer view toggle.
5. **The home-server cluster owner.** Already owns two or three
   mini PCs / Pi 5s / server-class boxes at home and wants a
   smart-home cluster on them. cave-home offers multi-node
   bootstrap, native cluster management, GPU off-load (Frigate
   inference), and active-passive failover. Single-box
   deployment is still supported as a smaller subset of the
   same architecture (see §5).

Explicit non-target: enterprises and datacenters. cave-home is
**server-class** (see §5) but for *homes*; the enterprise / sovereign
IDP audience is Cave Runtime's, not cave-home's.

## 3. Scope — IN

cave-home will ship, as first-class pillars:

- **Automation engine — the heart of cave-home.** A line-by-line
  Rust port of Home Assistant core (Apache-2.0, port method:
  line-by-line per ADR-002): state machine, event bus, automation
  rules, scripts, templates, **and the Energy Dashboard** as a
  first-class module of the same port. "HA core port = cave-home's
  heart" is non-negotiable; every other pillar is plumbed into it.
- **Integrations layer.** Zigbee, Z-Wave, Matter, MQTT, BLE, a HomeKit
  accessory bridge, Google Home / Alexa adapters.
- **Device protocol stacks.** Zigbee (Z2M / ZHA-class), Matter (chip-
  class), Z-Wave JS-class, ESPHome native API, Tasmota MQTT.
- **MQTT broker.** Embedded Mosquitto-class broker, with optional
  bring-your-own external broker.
- **Camera / NVR.** Frigate-class: RTSP ingest, object-detection
  inference, recording, clip extraction.
- **Dashboards.** Built-in Portal as a Lovelace-class alternative,
  mobile-friendly out of the box.
- **Voice assistant.** Local STT (whisper-class) + local TTS
  (piper-class) + wake-word + intent routing — Rhasspy / Home Assistant
  "Year of Voice" pattern, cloud-free.
- **Location tracking.** Owntracks-class.
- **Mobile companion app** *(confirmed in scope by founder
  2026-05-14)*. Native iOS + Android companion: push notifications,
  geofencing, voice control, camera live view, automation triggers,
  multi-node management UI. Cave-home back-end only — no third-party
  push relay. Stack choice (Tauri / Flutter / RN / KMM) is finalised
  in ADR-006.
- **Backup / restore.** Config snapshots + history-database snapshots.

### 3.1 Ecosystem ports — first-class pillars (ADR-009 / 010 / 011)

The following three vendor / standard ecosystems are **first-class
pillars** alongside the protocol stacks above, not optional add-ons.
They are common in the headline-persona's home (Charter §2 personas
1–2) and are too central to leave to community plugins.

- **UniFi (Ubiquiti) ecosystem** — *ADR-009 Accepted 2026-05-15.*
  Network (switches / APs), Protect (cameras, integrated with the
  Frigate pillar), Access (door control), Talk (VoIP intercom).
  Apache-2.0 line-by-line port of the HA UniFi-* integrations
  plus a direct port of the public REST / WebSocket API surface.
- **Philips Hue** — *ADR-010 Accepted 2026-05-15.* Apache-2.0
  line-by-line port of the HA Hue integration against the
  official Philips Hue API; **plus** a clean-room
  Hue-Bridge-emulator (advanced mode, derived from diyhue public
  protocol docs — diyhue itself is GPL and may not be read per
  Charter §6.1). Native Hue Zigbee bulbs still talk through the
  cave-home Zigbee stack — the bridge integration is for users
  with an existing Hue Bridge.
- **Busch-Jaeger free@home + KNX-IP** — *ADR-011 Accepted
  2026-05-15.* Apache-2.0 line-by-line port of the free@home
  REST API client; MIT line-by-line port of `xknx`; Apache-2.0
  port of the HA KNX integration; clean-room port of the KNXd
  gateway daemon (GPL, may not be read). free@home is hybrid on
  top of KNX-IP, so the KNX work is a **bonus deliverable** of
  the free@home pillar.

### 3.2 Category pillars — wave 1 (ADR-012 .. ADR-031, all Accepted 2026-05-15)

Founder's wholesale-approval dispatch of 2026-05-15 promotes a
further 20 smart-home OSS categories from "future-dispatch" to
**first-class cave-home pillars**. The "Eklenti Mağazası"
(separate add-on store) framing is **dropped**: the headline
persona finds every household need inside cave-home itself, with
one install, one upgrade, one backup, one dashboard.

| #  | Category                              | ADR     | Primary crate(s)                                                                                    |
| -- | ------------------------------------- | ------- | --------------------------------------------------------------------------------------------------- |
| 1  | Climate / HVAC / Heat Pump            | ADR-012 | `cave-home-hvac`                                                                                    |
| 2  | Water / Irrigation                    | ADR-013 | `cave-home-water`                                                                                   |
| 3  | Lighting (WLED + LED strips)          | ADR-014 | `cave-home-lighting-wled` *(clean-room)*                                                            |
| 4  | Cover / Garage / Awning               | ADR-015 | `cave-home-cover`                                                                                   |
| 5  | Lock integration                      | ADR-016 | `cave-home-lock`                                                                                    |
| 6  | Vacuum (Valetudo)                     | ADR-017 | `cave-home-vacuum`                                                                                  |
| 7  | Doorbell / Intercom + Alarm panel     | ADR-018 | `cave-home-doorbell`, `cave-home-alarm`                                                             |
| 8  | Air-quality sensors                   | ADR-019 | `cave-home-air-quality`                                                                             |
| 9  | Multi-room audio                      | ADR-020 | `cave-home-audio-mass`, `cave-home-audio-snapcast` *(clean-room)*, `cave-home-audio-mopidy`         |
| 10 | Notifications                         | ADR-021 | `cave-home-notify`                                                                                  |
| 11 | DNS + ad-blocking                     | ADR-022 | `cave-home-dns-adguard` *(clean-room)*, `cave-home-dns-unbound`                                     |
| 12 | Time-series history database          | ADR-023 | `cave-home-history`                                                                                 |
| 13 | Voice framework expansion             | ADR-024 | *(expands existing `cave-home-voice`)*                                                              |
| 14 | Wellness / health integrations        | ADR-025 | `cave-home-wellness`                                                                                |
| 15 | Household management (Grocy)          | ADR-026 | `cave-home-household`                                                                               |
| 16 | Calendar / PIM (Radicale)             | ADR-027 | `cave-home-calendar` *(clean-room)*                                                                 |
| 17 | TV / Display integration              | ADR-028 | `cave-home-display`                                                                                 |
| 18 | Garden / outdoor                      | ADR-029 | `cave-home-garden`                                                                                  |
| 19 | Pool / spa *(deferred, scaffold only)*| ADR-030 | `cave-home-pool`                                                                                    |
| 20 | Wearable / sleep *(deferred, scaffold)* | ADR-031 | `cave-home-wearable`                                                                                |

Each row's ADR records the upstream(s), the port method
(line-by-line for permissive upstreams, **clean-room** for the
GPL / EPL upstreams marked above per Charter §6.1), and the
M8–M11 milestone the work lands in.

The crates listed above are **scaffolded** in this commit
(Cargo.toml metadata + empty `src/lib.rs` per Charter §6.1
clean-room markers where applicable). Real implementation work
follows ROADMAP M8–M11 sequencing.

### 3.3 Charter §4 update — Eklenti Mağazası dropped

The category pillars in §3.2 mean there is **no separate
"add-on store"** for the headline persona. Everything they need
ships in the unified binary. Third-party HACS-style add-ons
remain supported via the ADR-004 orchestration layer for
community contributions outside the §3.2 pillar list. §4 below
still lists the explicit non-targets (NAS / media server /
photo backup / multi-tenant enterprise).

## 4. Scope — OUT

cave-home will explicitly **not** ship:

- **File server / NAS** (Samba, TrueNAS-class). Out.
- **Media server** (Jellyfin, Plex). Out.
- **Photo backup** (Immich-class). Out.
- **VPN gateway** (Tailscale-class). Out as a *first-class pillar*;
  reachable as an *external* integration in front of the hub.
  (WireGuard *primitives* are used internally by §3.2 categories
  — e.g. remote access — but cave-home does not advertise itself
  as a Tailscale-class mesh provider.)
- **Multi-tenant enterprise** access control.

> **Note (Charter v6, 2026-05-15):** the previous "Self-hosted
> app catalogue (Umbrel, CasaOS-class)" non-target has been
> **removed** because the §3.2 wave-1 pillars subsume the value
> proposition of an app catalogue. HACS-style third-party add-ons
> continue to be supported via the ADR-004 orchestration layer
> for contributions outside the §3.2 pillar list.

Anything not on the §3 (including §3.1 + §3.2) list is out by
default. New pillars require an ADR.

## 5. Architecture — server-class, bare-metal, single unified Rust binary, multi-node

cave-home and Cave Runtime share an **architectural class**:

- **Server-class** software (not a workstation app, not a desktop
  daemon).
- **Bare-metal** boot path (provisioned onto dedicated Linux
  hardware, not run alongside a user desktop).
- **Multi-machine scale** — cluster topology is a first-class
  concern, not bolted on.

The two projects differ in **target audience**: Cave Runtime serves
the *business world* (enterprise sovereign IDP, datacenter, multi-
region HA/DR); cave-home serves the *home world* (smart-home cluster,
1–N nodes inside a single household). Both are server-class.

**Deployment topology.** cave-home runs on a **bare-metal multi-node
cluster** inside the home. Typical deployments:

- **Primary hub node.** Pi 5 / NUC / mini-PC class. Runs broker,
  Zigbee / Matter / Z-Wave radios, automation engine, Portal.
- **Optional secondary failover node.** Active-passive HA for the
  primary hub. Keeps the home automating during firmware updates,
  hardware swaps, and outages.
- **Optional ML / GPU node.** Off-loads Frigate object detection
  (and, longer-term, voice / scene inference) onto an accelerator-
  equipped box without dragging the primary hub into camera /
  inference resource pressure.

**Single-node mode is supported** as the smallest viable deployment
— but the architecture is **multi-node first**. This is precisely
why the orchestration layer (ADR-004: K3s line-by-line Rust port)
is non-negotiable; orchestration is therefore *equally* central to
cave-home — the structural counterpart to the HA-core *behavioural*
heart declared below.

The concrete deployment-bootstrap shape (OS image vs CLI vs Portal
add-node UI) is the subject of **ADR-005**.

cave-home is **one Rust binary per node**. Cave Runtime's "single
consolidated binary" pattern, applied to the smart-home stack:

- **The heart of cave-home is the Home Assistant core port** —
  `cave-home-automation` (and the state machine / event bus in
  `cave-home-core`) is a line-by-line Rust reimplementation of HA
  core under Apache-2.0. Every other pillar (broker, Zigbee, Matter,
  Z-Wave, camera, voice, portal, CLI, Energy) is plumbed into that
  port. When in doubt about behaviour, HA core's behaviour wins.
- All upstream stacks live as **crates in this Cargo workspace** and
  are linked into one binary at build time.
- **No sub-processes, no sidecars.** No Python supervisor, no Node.js
  helper, no separate ffmpeg fork-and-exec service. If a stack can't
  be expressed in-process safely, that's an ADR-level conversation.
- The binary is the unit of install, the unit of upgrade, and the unit
  of rollback.

> **Orchestration layer (ADR-004 Accepted 2026-05-14).** The
> `cave-home-orchestration` umbrella crate is a **line-by-line
> Rust port of the `k3s-io/k3s` upstream** (Apache-2.0). The
> single-unified-binary mandate is **fully preserved**: the core
> hub *and* the orchestration layer compile into the same single
> binary. Third-party add-on containers (HACS-style community
> extensions) run on this native, in-process K3s layer. No
> dependency on Cave Runtime's Kubernetes port — cave-home's
> orchestration crates are scratch reimplementations from K3s
> upstream; the two projects derive from different chains
> (cave-home from `k3s-io/k3s`, Cave Runtime from
> `kubernetes/kubernetes`) and share no code (§5.1).

### 5.1 Independence from Cave Runtime

cave-home is **a fully independent project from Cave Runtime**. There
is **no code sharing, no crate reuse, no path dependency, no git
dependency** between the two repos. Concretely:

- Cave Runtime crates (`cave-net`, `cave-auth`, `cave-portal-shell`,
  `cave-kernel`, etc.) are **not** consumed by cave-home.
- Where the two projects need the same primitive (MQTT, auth,
  observability), cave-home writes it from scratch.
- The two projects target different audiences (Cave Runtime →
  enterprise sovereign IDP; cave-home → home smart-home hub),
  different upgrade cadences, and different upstream-licence
  matrices.
- The shared `cave-` brand prefix is the *only* shared element.
  Story, release cycle, licence decision, and roadmap are independent.

Duplication is **accepted on purpose**, not a deficiency. It keeps
each project free to evolve at its own tempo.

## 6. The golden rule — line-by-line upstream parity + TDD

Inherited from Cave Runtime and **non-negotiable**:

> When cave-home integrates an upstream (Home Assistant core, ZHA /
> Z2M, ESPHome native API, Frigate, …), it does so with **line-by-line
> parity** to that upstream's current stable, verified by **strict
> TDD**. Stubs, placeholders, and "self-reported" feature parity are
> forbidden.

How it shows up in day-to-day work:

- Integration tests are written against real upstream behaviour, not
  against our own stubs.
- Mocks are reserved for hardware boundaries (a real radio, a real
  camera); they are not allowed in place of upstream behaviour.
- A feature is "done" only when it passes upstream's own behavioural
  expectations against our reimplementation. No TODOs in shipped code.

### 6.1 Clean-room mandate for GPL / copyleft upstreams

cave-home is Apache-2.0 (ADR-002). To keep the tree Apache-2.0-clean
against the GPL-licensed corners of the smart-home stack
(Zigbee2MQTT, Tasmota, parts of ESPHome) and the EPL-licensed
Mosquitto, the golden rule is qualified as follows:

> **For any GPL / AGPL / EPL or otherwise strong-copyleft upstream,
> the line-by-line rule is replaced by a clean-room reimplementation
> rule.** Contributors **do not read the upstream's source code**.
> They reimplement from the **public protocol specification, RFCs,
> wire-format analysis (e.g. Wireshark dissections), and public API
> documentation** only. Test fixtures are written from scratch — the
> upstream's tests are not ported. Each clean-room crate's ADR
> carries an explicit "implemented from spec; source not read"
> declaration.

CONTRIBUTING.md codifies the contributor protocol; per-upstream
port-method classification is recorded in
`docs/upstream/REFERENCES.md`.

### 6.2 Linux 7.1+ only, no backward compatibility

Inherited from Cave Runtime's no-backcompat mandate and adopted
verbatim:

> **cave-home runs on Linux 7.1+ kernels. Backward compatibility is
> unnecessary**: 32-bit ARM (old Pi 1–3), kernels older than 7.0,
> and legacy glibc / musl features are not supported via `#ifdef`.
> cgroup v2 is mandatory; io_uring, eBPF, and modern syscalls are
> freely used; KMS / DRM is modern; systemd / init1 is modern. Old
> hardware is not a target — the floor is **64-bit ARM (Pi 5 /
> Apple Silicon class) or modern x86-64**.

The rationale and the concrete kernel-feature dependencies are
recorded in **ADR-003**. §8 ("No backcompat") below refines the
*hardware* posture; §6.2 is the *kernel / userland* mandate at
golden-rule level.

### 6.3 Grandma-friendly UX mandate

Recorded in **ADR-007** (Accepted 2026-05-14) and elevated here
to golden-rule level so that no future implementation choice can
breach it without amending the charter:

> **The cave-home Portal and mobile app hide every implementation
> detail from the end-user.** The UI uses only *home-world*
> vocabulary — *device, room, automation, scene, solar production,
> security, family, hub*. The UI **never surfaces**: K3s /
> Kubernetes terminology (pod, deployment, kubelet, scheduler,
> apiserver, etcd, kine, RBAC, namespace), container or
> orchestration concepts, MQTT topic / QoS / retain, Modbus
> registers, Zigbee channel / PAN-ID, manifest YAML, Helm chart
> internals, certificate / token / join-URL strings. The
> implementation can be all of those — the **UI cannot**.

Expert / power-user escape hatch: a **Settings → "Developer view"
toggle** unlocks technical pages (cluster topology, container
logs, manifest editor, raw MQTT traffic). The toggle is
**off by default**. The mobile app does **not** expose the
toggle at all — developer view is Portal-only.

The full UI-language translation matrix is maintained in
`docs/ui-language.md` and is **normative**: any UI surface that
ships a term not on that table needs a docs PR adding it.

Implications captured in adjacent ADRs:

- ADR-004 (orchestration / K3s) — K3s lives entirely under the
  hood; no UI surface mentions it.
- ADR-005 (deployment / multi-node bootstrap) — the (c) Hybrid
  flow's user-facing path is QR-code / token-share / IP-picker;
  raw join-URLs are never shown.
- ADR-006 (mobile app stack) — the stack choice is constrained
  by this mandate (native-class UX, OS-conformant design
  language).
- i18n (TR + EN + DE) is **mandatory from M1** so that the
  charter §2 personas in mixed-language homes (Iphofen /
  Germany) get a usable product on day one.

## 7. Always-latest mandate

Every upstream we port tracks its **current stable**, not a snapshot.
The Rust toolchain follows the same rule: `rust-toolchain.toml` pins
`stable`, not a specific version.

If an upstream goes through a breaking change, we adopt it; if we
disagree, we contribute upstream or write an ADR justifying a
documented deviation. We never silently freeze a fork.

## 8. No backcompat

cave-home is greenfield. §6.2 sets the **kernel / userland** floor
(Linux 7.1+, no backcompat — accepted in ADR-003). This section sets
the **hardware** floor that goes with it:

- **64-bit modern hardware.** A small mini-PC or low-power x86 / ARM64
  box with ≥ 8 GB RAM is the floor (smart-home workloads need less
  RAM than a home-server). Camera/NVR pillars assume hardware
  inference (iGPU or accelerator).
- **64-bit only.** 32-bit ARM (Pi 1–3 class) is **not** a target —
  see ADR-003. The ARM64 floor is Pi 5 / Apple Silicon class.
- **A dedicated box.** cave-home owns the hardware; you do not run it
  alongside your desktop.
- **Modern radios.** Zigbee 3.0+ coordinators, Matter-capable
  controllers, Z-Wave 700+ — we do not ship workarounds for hardware
  EOL'd by the upstream community.

If you have a use case below the floor, please file an issue
describing it. We don't ship retrofitted backcompat shims.

## 9. Privacy-first / OSS-first

This is **stricter** than Cave Runtime's sovereignty rule, because
smart-home data (presence, voice, video) is far more sensitive than
generic platform data.

- **Zero cloud dependency in the critical path.** Setup, login,
  recovery, automations, voice, camera — all work on a network with
  no internet access.
- **No telemetry default-on.** Opt-in only, and only to a sovereign
  endpoint a user can disable in one click.
- **Local-first always.** Voice STT/TTS, object detection, automation
  evaluation — all on-device. Cloud is opt-in *augmentation*, never
  a precondition.
- **All first-party code is OSS.** Licence is recorded in `LICENSE`
  and rationalised in ADR-002.

## 10. Charter completion criteria — TBD

> *Placeholder — to be finalised with Burak.*
>
> Intent: enumerate, before v0.1 ships, the concrete state for "charter
> satisfied": reference hardware bill of materials, the in-scope
> pillars that must be implemented end-to-end for the first release,
> licence decision recorded, reproducible build available, external
> installer verified by a non-maintainer.

---

## Amendment process

The charter is amended via numbered ADRs in `docs/adr/`. A change to
the charter is itself an ADR (e.g. "Amend Charter §3 to add foo").
The current document is rewritten to reflect the accepted ADR — we
don't keep diff-style amendments inline.
