# cave-home — Roadmap (multi-year: M0–M11+)

**Status:** Draft. Subject to charter approval and ADR-001 sign-off.
The opening hand for M1 (MQTT + core + Portal + minimal MQTT
integration) is still subject to founder approval; the alternative
("start from Zigbee to stress the line-by-line discipline") is logged
as ADR-001 open question #4.

**Horizon:** 2026-05 onward, multi-year. M0–M5 (12 months) deliver
the v0.1 core; M6 (months 8–12 parallel) delivers multi-node bootstrap;
M7 (months 12–15) delivers the cave-home OS image build pipeline.
M8–M10 (~year 2) ship the Charter §3.2 wave-1 category pillars (HVAC,
water, lighting, cover, lock, vacuum, doorbell, alarm, air-quality,
multi-room audio, notification, DNS, history, voice expansion,
wellness, household, calendar, display, garden). M11+ ships the
deferred pillars (pool, wearable / sleep) and the post-v1 backlog.

[ASSUMPTION: M8–M10 sequencing groups categories by engineering
affinity, not by founder priority order. Founder may want to
re-shuffle wave-1 sequencing — recorded as an open question.]

cave-home is a **server-class, bare-metal, multi-node sovereign
smart-home platform**. Single-node mode is supported as the smallest
viable deployment, but the architecture targets multi-node clusters
as a first-class concern. The target-audience split with Cave Runtime
is **business world vs home world**, not server-class vs not: both
projects are server-class.

cave-home is **independent of Cave Runtime**. It has its own
timeline, its own OSS launch, its own licence cadence. Cave Runtime's
2026-05-21 OSS launch is **not** binding on cave-home; nothing on
this roadmap is gated by the Runtime project.

The roadmap is organised in **milestones** rather than fixed dates.
Each milestone has a clear "done" criterion. Milestones run
sequentially; we don't start M(N+1) until M(N) is at parity.

---

## M0 — Scaffold & Charter (week 0)

- Repo, charter draft, ADR-001 draft.
- Workspace + 12 placeholder crates.
- Upstream reference matrix.
- Licence decision (ADR-002) — TBD.
- **Done when:** founder reviews & approves the charter, ADR-001,
  and the licence choice.

## M1 — MQTT broker + event bus + Portal (months 1–2)

- Embedded MQTT broker (port method depends on ADR-002).
- Core event bus + state machine skeleton (Home Assistant core
  semantics, Rust).
- Minimal device integration: MQTT-only devices (Tasmota-class,
  generic MQTT).
- Portal: minimal Lovelace-class dashboard, mobile-friendly.
- **Done when:** a fresh box boots, a Tasmota lamp pairs over MQTT,
  the Portal shows it, and an automation can toggle it from a
  state-change event.

## M2 — Zigbee + Orchestration Phase 1 (months 3–4)

- Zigbee stack (clean-room reimplementation of the
  zigbee-herdsman surface, from Zigbee 3.0 spec + public API
  docs).
- Coordinator support: at minimum, Sonoff ZBDongle-E and SMLIGHT
  SLZB-class.
- Mainstream device classes: lights, switches, sensors, locks.
- **Parallel — Orchestration Phase 1:** containerd
  (`cave-home-containerd-rs`) + kubelet (`cave-home-kubelet-rs`) +
  **flannel CNI** (`cave-home-cni-flannel`, locked by **ADR-008**)
  + kube-proxy (`cave-home-kube-proxy-rs`).
- **Done when:** a representative Zigbee 3.0 device set pairs,
  reports, and is controllable via the M1 automation engine, AND
  a pod scheduled by an out-of-tree control plane runs on the
  cave-home kubelet port with flannel providing pod networking.

## M2.5 — Orchestration Phase 1 (parallel track, starts in M2)

ADR-004 Accepted 2026-05-14: cave-home's orchestration layer is a
**line-by-line Rust port of `k3s-io/k3s`** inside the unified
binary. Phase 1 onwards is a **parallel track**, not a serial
milestone — smart-home pillars do not stall behind the
orchestration port.

- Phase 1 scope: container runtime + node-side primitives.
- Crates landed this phase: `cave-home-containerd-rs`,
  `cave-home-kubelet-rs`, `cave-home-kube-proxy-rs`,
  `cave-home-cni-rs`.
- **Done when:** a pod scheduled by an out-of-tree control plane
  (e.g. a temporary K3s upstream binary used for validation) runs
  on the cave-home kubelet port, with cave-home-cni-rs providing
  pod networking.

Phases 2–4 run parallel to M3 / M4 / M5 respectively; see those
milestones.

## M3 — Automation engine port (months 5–6)

- Line-by-line port of Home Assistant core automation: triggers,
  conditions, actions, scripts, templates.
- History database (SQLite per ADR-005 candidate; final pick in
  that ADR).
- Backup / restore of config + history.
- **Parallel — Orchestration Phase 2:** cluster control plane.
  Crates: `cave-home-apiserver-rs`, `cave-home-scheduler-rs`,
  `cave-home-controller-manager-rs` (porting K3s's *vendored*
  Kubernetes wrappers, not vanilla `kubernetes/kubernetes`).
- **Done when:** automation parity (above) AND the cave-home
  in-binary control plane can accept manifests and schedule them
  onto a cave-home node.

## M3.5 — Solar Tier 1 (interstitial)

Surplus management arrives with EVCC; the rest of Tier 1 covers
vendor-agnostic inverter monitoring and production forecasting.

- **EVCC port** (`cave-home-solar-evcc`): line-by-line port of
  evcc-io/evcc (Apache-2.0). Surplus management, EV charging, heat-
  pump load shifting tied into the M3 automation engine.
- **SunSpec models** (`cave-home-solar-sunspec`): spec-based Rust
  implementation of SunSpec Alliance public Modbus models, covering
  the major inverter vendors (SMA, Fronius, SolarEdge, Huawei,
  Goodwe, Kostal) without per-vendor code.
- **Forecast** (`cave-home-solar-forecast`): API client for
  Forecast.Solar + PVGIS public APIs, feeding the automation engine
  with day-ahead production estimates.
- **Done when:** a SunSpec-class inverter is discovered + monitored
  end-to-end, EVCC governs an EV charger from surplus, and a
  Forecast.Solar prediction is used in an automation.

## M4 — Matter + Z-Wave (months 7–9)

- Matter stack (line-by-line from project-chip).
- Z-Wave stack (line-by-line from node-zwave-js).
- BLE integration for proximity / sensor classes.
- HomeKit accessory bridge (line-by-line from Scrypted).
- **Parallel — Orchestration Phase 3:** kine — K3s's SQLite /
  Postgres-backed etcd replacement, the trick that makes the
  whole stack fit in one binary.
  Crate: `cave-home-kine-rs`.
- **Done when:** Matter + Z-Wave + BLE devices coexist on one box
  AND the cave-home control plane is running on kine instead of
  an external etcd.

## M5 — Camera / NVR + Voice + Mobile + Solar Tier 2 + Orch. Phase 4 (months 10–12)

- Frigate-class NVR: RTSP ingest, object-detection inference,
  recording, clip extraction.
- Voice pipeline: whisper.cpp STT + piper TTS + wake-word + intent
  routing into M3 automation engine.
- Mobile companion app: push notifications, geofencing, quick
  controls. Push relay is cave-home-back-end-only — no third party.
- **Solar Tier 2 (opportunistic, priority driven by Burak's hardware):**
  Hoymiles microinverter support (`cave-home-solar-hoymiles`) —
  clean-room reimplementation from Hoymiles wire protocol +
  AhoyDTU public API docs (refs lumapu/ahoy + tbnobody/OpenDTU
  are GPL-3.0 and therefore *reference only*; contributors must not
  read those sources). If Burak's installed hardware needs Tier 2
  earlier, this work shifts forward.
- **Parallel — Orchestration Phase 4:** K3s-spec ancillary
  components.
  Crates: `cave-home-helm-controller-rs`, `cave-home-klipper-lb-rs`,
  `cave-home-traefik-rs` (optional ingress). A
  `local-path-provisioner` crate is queued for after M5 if storage
  add-ons demand it.
- **Done when:** a single box runs broker + Zigbee + automations +
  one IP camera with object detection + local voice assistant
  end-to-end, with the mobile app driving it, AND a representative
  third-party add-on installs via Helm onto the cave-home
  orchestration layer. Tier 2 solar is a stretch deliverable, not
  a blocker for M5 "done"; the same applies to traefik ingress.

## M5.5 — Three ecosystem ports (parallel to M2–M5)

ADR-009 (UniFi), ADR-010 (Hue), ADR-011 (Busch-Jaeger free@home +
KNX-IP) all Accepted 2026-05-15. The three are charter §3.1
first-class pillars; this milestone is a **parallel track**
running across M2–M5 rather than a serial gate.

- **UniFi** — four sub-crates (`cave-home-unifi-network`,
  `cave-home-unifi-protect`, `cave-home-unifi-access`,
  `cave-home-unifi-talk`). All Apache-2.0 line-by-line ports of
  the HA UniFi-\* integrations + Ubiquiti public REST/WebSocket.
  UniFi Protect cameras converge with the Frigate-class camera
  pillar onto a single Portal surface (M5 camera milestone).
- **Philips Hue** — two crates. `cave-home-hue` (Apache-2.0
  line-by-line from HA `hue` + Philips Hue API). `cave-home-hue-
  bridge-emu` (**clean-room** from Philips developer-portal docs
  — diyhue GPL source NOT read; advanced-mode behind Settings →
  Developer view per ADR-007).
- **Busch-Jaeger free@home + KNX-IP** — two crates.
  `cave-home-free-home` (Apache-2.0 line-by-line) lands first
  because Burak's house runs free@home. `cave-home-knx`
  (mixed-source: xknx MIT + HA `knx` Apache-2.0 line-by-line
  for transport + entities; KNXd-equivalent gateway daemon
  **clean-room** from KNX Association public spec) follows.

Grandma-friendly UX (ADR-007 / Charter §6.3) applies: the UI
shows Wi-Fi, kameralar, kapı, lambalar, oda sahneleri — never
"UniFi controller endpoint", "Hue Bridge API", "KNX group
address". Those names live in Developer view only.

**Done when:** Burak's Iphofen house (UniFi + free@home) is
running end-to-end against cave-home; a representative Hue
Bridge install is controllable from cave-home; a KNX-TP
installation reachable via the `cave-home-knx` gateway daemon
surface.

## M6 — Multi-node bootstrap (ADR-005 Accepted 2026-05-14)

ADR-005 picked **candidate (c) Hybrid**: OS image + CLI + Portal
"Add node" wizard, all converging on the same `cave-home-cluster` +
`cave-home-node-discovery` crates. M6 has two phases, both
**parallel tracks** to M4 / M5 respectively:

### M6 Phase A — CLI bootstrap (parallel to M4, months 8–9)

- `cavehome init` / `cavehome join <token>` / `cavehome status` /
  `cavehome destroy`.
- K3s join-token coordination (uses the cave-home K3s port from
  ADR-004).
- LAN auto-discovery (mDNS / Bonjour + cave-home-specific
  protocol).
- `cave-home-cluster` and `cave-home-node-discovery` crates get
  filled in from their current empty-placeholder state.
- **Done when:** an existing Debian / NixOS / Arch box becomes
  a cave-home node by running two CLI commands; a second box
  joins the cluster via `cavehome join` over the LAN.

### M6 Phase B — Portal "Add node" wizard (parallel to M5, months 10–12)

- Portal wizard for adding a node: discovered-device picker,
  QR-code rendering, token-sharing flow.
- Speaks to the CLI primitives under the covers — no parallel
  implementation.
- **Done when:** the homeowner adds a second node from the
  Portal without ever opening a terminal.

## M7 — cave-home OS image build pipeline (months 12–15, post-v0.1)

The cave-home OS image is the consumer-facing install path
(ADR-005 candidate (c) default). Built per-architecture (ARM64
for Pi 5 / generic ARM, x86-64 for NUCs / mini-PCs).

- Build pipeline: Yocto / Buildroot / OpenWrt Image Builder-class
  (final choice in a follow-on ADR written at M7 kick-off; the
  `cave-home-os-builder` work-stream may live as a separate repo
  or as a workspace crate, also decided then).
- Image carries a Linux 7.1+ kernel / cgroup v2 userland per
  ADR-003.
- Signed releases; basic OTA flow.
- First-boot UX: image auto-DHCPs, announces itself on the LAN
  via `cave-home-node-discovery`, is added from an existing
  cluster's Portal "Add node" wizard.
- **Done when:** a homeowner flashes the image onto an SD card,
  boots a Pi 5, and adds it from the Portal of an existing
  cluster.

## M8 — Charter v6 wave 1: home-comfort cluster (~3 months)

First sub-wave of the Charter §3.2 expansion. Engineering affinity:
sensors + actuators that compose into the daily-comfort experience.

- **HVAC** (ADR-012) — `cave-home-hvac`.
- **Water** (ADR-013) — `cave-home-water`.
- **Lighting WLED** (ADR-014) — `cave-home-lighting-wled` (clean-room).
- **Cover / Garage / Awning** (ADR-015) — `cave-home-cover`.
- **Lock** (ADR-016) — `cave-home-lock`.

**Done when:** Burak's Viessmann heat pump, OpenSprinkler garden
irrigation, and Nuki front-door lock all run end-to-end against
cave-home; a WLED strip and a Somfy awning each respond to an
automation trigger.

## M9 — Charter v6 wave 2: security + audio cluster (~3 months)

- **Vacuum** (ADR-017) — `cave-home-vacuum`.
- **Doorbell + Alarm** (ADR-018) — `cave-home-doorbell` +
  `cave-home-alarm`.
- **Air-quality** (ADR-019) — `cave-home-air-quality`.
- **Multi-room audio** (ADR-020) — `cave-home-audio-mass` +
  `cave-home-audio-snapcast` (clean-room) + `cave-home-audio-mopidy`.
- **Notification** (ADR-021) — `cave-home-notify`.

**Done when:** the headline §2 persona has a working
intercom-doorbell + alarm-armed + vacuum + multi-room music
experience entirely on cave-home; CO₂-spike automations
trigger ventilation; family-shared notifications route through
the cave-home back-end (no FCM in the control plane).

## M10 — Charter v6 wave 3: data + family cluster (~3 months)

- **DNS / ad-blocking** (ADR-022) — `cave-home-dns-adguard`
  (clean-room) + `cave-home-dns-unbound`.
- **History DB** (ADR-023) — `cave-home-history`.
- **Voice framework expansion** (ADR-024) — expands existing
  `cave-home-voice` crate (no new crate).
- **Wellness** (ADR-025) — `cave-home-wellness`.
- **Household (Grocy)** (ADR-026) — `cave-home-household`.
- **Calendar (Radicale)** (ADR-027) — `cave-home-calendar`
  (clean-room).
- **TV / display** (ADR-028) — `cave-home-display`.
- **Garden** (ADR-029) — `cave-home-garden`.

**Done when:** five-year sensor-history queries work; "Hey
cave-home, akşam moduna geç" routes through the expanded voice
stack; family CalDAV is self-hosted; the kitchen wall-tablet
Portal-mode renders; LG / Samsung TVs respond to voice.

## M11+ — Deferred pillars + post-v1 backlog

- **Pool / spa** (ADR-030) — `cave-home-pool`.
- **Wearable / sleep** (ADR-031) — `cave-home-wearable`.
- Cilium-class CNI (deferred from ADR-008).
- ADR-032 — Reference hardware profile (BoM for v0.1 finalised).
- ADR-033 — Camera inference backend (Coral / NVIDIA / OpenVINO).
- ADR-034 — Update / rollback model (atomic + snapshot-aware).
- Other category catch-ups (vendor sub-modules, locale catalogues,
  power-user features rejected from M1–M10).

## Explicitly NOT on the 12-month roadmap

- File server / NAS, media server, photo backup, app catalogue, VPN
  gateway. *(Charter §4 out-of-scope.)*
- Multi-tenant / multi-household access control.
- Matching Home Assistant's full ~2000-integration count. M1–M5
  delivers a curated subset that exercises the integration
  discipline properly. Breadth comes after the foundations.

## Cross-cutting non-negotiables

These apply to every milestone:

- **Charter §6 — golden rule.** Line-by-line upstream parity + TDD
  strict for the upstreams we integrate. The per-upstream port
  method is set in ADR-002.
- **Charter §7 — always-latest mandate.** Every upstream we track
  follows its current stable; the Rust toolchain follows `stable`.
- **Charter §8 — no backcompat.** Modern hardware, modern kernel,
  modern radios.
- **Charter §9 — privacy-first.** No cloud in the critical path,
  no telemetry default-on, local-first STT/TTS/inference.
