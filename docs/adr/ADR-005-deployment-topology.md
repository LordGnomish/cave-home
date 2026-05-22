# ADR-005 — Deployment topology / multi-node bootstrap

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).
Decision: candidate **(c) Hybrid** — OS image (default) + CLI
(advanced) + Portal "Add node" UI.

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

Charter §5 declares cave-home is a **server-class, bare-metal,
multi-node** platform: a typical home deployment is 1–N nodes
(primary hub + optional failover + optional ML / GPU node).
ADR-004 locks in the orchestration mechanism (K3s line-by-line
Rust port) but says nothing about **how a fresh box gets to the
point of joining the cluster**.

The user-facing bootstrap question is open and is what this ADR
will close:

1. How does a homeowner take a fresh Pi 5 / NUC / mini-PC out of
   the box and end up with cave-home running on it?
2. How does that same homeowner add a second / third node to the
   cluster (failover, GPU off-load)?
3. How does an advanced user (homelabber persona §2.3) script
   that same flow over SSH for unattended provisioning?

Constraints that hold:

- **Charter §1 vision.** "Set up in an afternoon" — bootstrap
  cannot require kubectl knowledge.
- **Charter §6.2 / ADR-003.** Linux 7.1+ floor, modern systemd,
  cgroup v2 mandatory.
- **Charter §9.** No cloud account / vendor service in the
  bootstrap path (no GitHub-mediated join tokens, no Tailscale
  required, etc.).
- **ADR-004.** The orchestration substrate is K3s-derived;
  whatever bootstrap mechanism we choose must produce a working
  cave-home K3s control plane on the first node and a joined
  worker on subsequent nodes.

## Decision

**Candidate (c) Hybrid** — cave-home ships **all three** entry
points, and they share one underlying mechanism:

1. **OS image (the default consumer path).** A cave-home OS image
   — Home Assistant OS-class — is the headline install method
   for the family / cluster-owner personas. Flash, boot, the
   box announces itself on the LAN, the user adds it from the
   Portal.
2. **CLI (the advanced power-user path).** `cavehome init` /
   `cavehome join <token>` / `cavehome status` / `cavehome
   destroy` for scripted deploys and homelabber-persona users
   running cave-home on top of a Debian / NixOS / Arch box.
3. **Portal "Add node" UI (the seam-hider).** A wizard that
   renders the CLI flow as QR codes / token sharing / IP
   pickers, so the headline persona never sees the CLI but the
   advanced user can still drive the same primitives by hand.

All three converge on the same `cave-home-cluster` +
`cave-home-node-discovery` crates: the CLI is the primary
surface; the OS image bakes them in and pre-runs `cavehome init`
on first boot; the Portal wizard speaks to the CLI under the
covers.

## Candidate shapes

### (a) CLI bootstrap

Traditional K3s pattern, ported to cave-home's CLI surface:

```
# On the first box:
cavehome init

# Output: prints a join token and a discovery hint.

# On subsequent boxes:
cavehome join --token=… --hub=…
```

- **Pro:** Minimum viable. The crates this needs
  (`cave-home-cluster` for lifecycle, `cave-home-node-discovery`
  for LAN mDNS) are exactly what the workspace already scaffolds.
- **Pro:** Scripts cleanly for the homelabber persona.
- **Con:** Requires the homeowner to be comfortable on a shell.
  Worst case for the family / non-technical end-user.

### (b) OS image (cave-home OS)

Build a cave-home OS image — Home Assistant OS-class — that boots
straight into the cave-home stack, advertises itself on the LAN,
and is added through the Portal:

```
1. Homeowner flashes cave-home.img onto a USB / SD card.
2. Boots the target box; it auto-DHCPs and announces itself.
3. From the Portal on the existing hub: "Add node" → picks the
   discovered box → enters / scans an enrolment token.
4. The new node joins the cluster, downloads role config,
   becomes operational.
```

- **Pro:** Best end-user UX. Mirrors the Home Assistant OS
  install experience the persona §2.2 (family user) is already
  used to.
- **Pro:** Image carries the kernel / userland known to satisfy
  ADR-003 — no risk of a user installing onto an Ubuntu LTS
  below the floor.
- **Pro:** Locked-down attack surface (immutable rootfs class).
- **Con:** Big scope: needs an image build pipeline (Yocto /
  Buildroot / OpenWrt Image Builder class), per-architecture
  artefacts (ARM64 + x86-64), signed releases, OTA story.
- **Con:** Less flexible for the homelabber who already has a
  Debian / NixOS box and just wants cave-home on it.

### (c) Hybrid — OS image (default) + CLI (advanced) + Portal UI (abstracts both)

cave-home ships both. The OS image is the **default consumer
path**; the CLI is the **advanced power-user path**; the Portal
"Add node" workflow renders the CLI flow as a wizard with QR
codes / token sharing / IP pickers, so the homeowner never sees
the CLI but the homelabber can drive the same primitives by hand.

- **Pro:** Captures both audiences. Family persona §2.2 gets
  the HA-OS experience; homelabber persona §2.3 gets the
  scriptable surface.
- **Pro:** Portal UI as the third entry point hides the seam.
  The user clicks "Add node", the Portal speaks to the CLI under
  the covers, the CLI manages K3s join tokens, mDNS, etc.
- **Con:** The biggest scope. Requires all of (a) AND (b) AND
  the Portal workflow that fronts them.
- **Con:** Two bootstrap paths mean two upgrade paths; OTA
  matters more.

## Consequences

### Accepted gains

- **Best end-user UX** for the headline personas (§2.2 family,
  §2.4 cluster owner): the OS image flash + Portal "Add node"
  flow mirrors the Home Assistant OS experience users already
  expect.
- **Scriptable for homelabbers** (§2.3): the same primitives
  exposed as a CLI mean unattended provisioning works out of
  the box.
- **One mechanism under the covers.** OS image, CLI, and Portal
  all converge on `cave-home-cluster` + `cave-home-node-
  discovery` — no parallel implementations, no drift.
- **OS image enforces ADR-003.** The flashed image carries a
  kernel / userland known to satisfy Linux 7.1+ / cgroup v2;
  the user cannot accidentally land below the floor.

- **Grandma-friendly user-facing flow** (ADR-007, Charter §6.3).
  Whether the user goes through the OS image, the CLI, or the
  Portal wizard, the **headline user-facing experience is QR-code
  / token-share / IP-picker based**. Raw join URLs, K3s tokens,
  TLS certificates, and node-bootstrap manifests **never** appear
  in user UI; they are abstracted as a QR code or a 6-digit code
  the user reads off the primary hub. The CLI surface remains
  available for §2.4 / §2.5 power users (and continues to print
  the raw token for scripted use), but Mobile + Portal "Add node"
  flows are grandma-friendly by mandate.

### Accepted costs

- **Biggest scope of the three candidates.** All of (a) + (b) +
  Portal wizard. Phased over M6 + M7 (see Implementation phases).
- **OS image is a sub-project.** Yocto / Buildroot / Image
  Builder-class pipeline, per-architecture artefacts (ARM64 + x86-
  64), signed releases, OTA story. Likely lives in a separate
  `cave-home-os-builder` repo or workspace crate (decided in
  M7 when work starts).
- **Two upgrade paths** (OS image OTA vs CLI-installed package).
  Both must honour ADR-009 (update / rollback) when that ADR
  lands.

### Implementation phases

The bootstrap work is sequenced across M6 (Phase A/B) and M7,
mirrored in `ROADMAP.md`:

- **M6 Phase A — CLI bootstrap (months 8–9, parallel to M4).**
  `cavehome init` / `join` / `status` / `destroy`. K3s join-token
  coordination. LAN auto-discovery (mDNS / Bonjour). The
  `cave-home-cluster` and `cave-home-node-discovery` crates are
  filled out in this phase. **Done when:** an existing Debian box
  becomes a cave-home node by running two CLI commands; a second
  box joins via `cavehome join` from any other CLI on the LAN.
- **M6 Phase B — Portal "Add node" UI (months 10–12, parallel
  to M5).** Web wizard, QR-code rendering, token sharing, IP
  picker. **Done when:** the homeowner adds a second node from
  the Portal without ever opening a terminal.
- **M7 — cave-home OS image build pipeline (months 12–15,
  post-v0.1).** Yocto / Buildroot / Image Builder-class. ARM64
  (Pi 5 + generic ARM) + x86-64. Signed releases, basic OTA
  flow. **Done when:** a homeowner flashes the image onto an SD
  card, boots a Pi 5, and adds it from the Portal of an
  existing cluster.

These phases are not strictly serial; Phase A is a prerequisite
for B (the wizard needs primitives to drive). M7 can start in
parallel with the end of M6 Phase B if scope allows.

## Open questions

1. **Join-token transport.** mDNS + short-code, QR-code printed on
   physical device, manual token paste, all three? Driven by the
   bootstrap shape and the UX target.
2. **Failover-node sync.** Does the secondary failover node
   replicate via K3s built-ins (kine snapshots) or via a
   cave-home-specific config-sync layer? Probably K3s built-ins,
   but worth recording.
3. **ML / GPU node onboarding.** Same path as a generic worker
   node, or does it need a hardware-introspection step (Coral /
   NVIDIA / OpenVINO detection) at join time?
4. **Recovery from total cluster loss.** What does "I lost the
   primary hub, here is a new box, restore my home" look like?
   This bleeds into ADR-009 (update / rollback model) and the
   backup story; record the boundary here once the bootstrap
   shape is chosen.

## Notes

This ADR is Accepted. The `cave-home-cluster` and
`cave-home-node-discovery` crates can now be written against
the (c) Hybrid mechanism. An `cave-home-os-builder` work-stream
will spin up when M7 starts; its precise shape (separate repo
vs workspace crate, Yocto vs Buildroot vs Image Builder) is the
subject of a follow-on ADR written at M7 kick-off.
