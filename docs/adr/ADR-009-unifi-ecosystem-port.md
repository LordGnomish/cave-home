# ADR-009 — UniFi (Ubiquiti) ecosystem port

## Status

**Accepted** — 2026-05-15, finalised by Burak Tartan (founder).

Created: 2026-05-15
Supersedes: —
Superseded by: —

## Context

Charter §2 persona 1–2 (the non-technical family + the
privacy-sensitive family head) overwhelmingly run **Ubiquiti
UniFi** at home: a UniFi Dream Machine / Cloud Gateway / UDM-Pro
as the router, a UniFi switch, one or two UniFi APs, and (where
cameras are present) a UniFi Protect-class NVR or G4 cameras.

The cave-home Frigate-class camera pillar (Charter §3) overlaps
with UniFi Protect; the network observability that lets a family
know "Wi-Fi is healthy" overlaps with UniFi Network; the door /
access concerns overlap with UniFi Access; and intercom /
doorbell needs overlap with UniFi Talk.

Leaving these to community-plugin contracts would put the
headline persona's most visible home-network surface outside
cave-home's first-class pillars. ADR-009 closes that gap.

Constraints that hold:

- **ADR-002** — Apache-2.0 line-by-line porting is the default
  for permissive upstreams. The HA UniFi-* integrations are
  Apache-2.0 (HA core is Apache-2.0).
- **ADR-004 / ADR-008** — orchestration + flannel are already
  locked; the UniFi pillar consumes them, does not replace them.
- **ADR-007 / Charter §6.3** — the UI never says "UniFi
  controller", "API endpoint", "WebSocket subscription";
  cave-home renders UniFi state in home-world vocabulary
  (Wi-Fi, switch, kamera, kapı, interkom).

## Decision

Port the **HA UniFi-\* integrations** (Apache-2.0) line-by-line
into four cave-home sub-crates, one per UniFi product surface,
all living in the workspace and linked into the unified binary:

1. **`cave-home-unifi-network`** — port of the HA `unifi`
   integration. Surfaces switch / AP / port telemetry, client
   tracking, Wi-Fi network state. Plumbs into cave-home's
   automation engine as a device-class entity.
2. **`cave-home-unifi-protect`** — port of the HA `unifiprotect`
   integration. Surfaces UniFi Protect cameras, doorbells,
   motion / smart detections. Where a user runs both UniFi
   Protect and Frigate, the two render through the same camera
   pillar surface in the Portal.
3. **`cave-home-unifi-access`** — port of the (smaller, newer)
   HA UniFi Access integration plus direct REST API client.
   Surfaces door locks, hub status, access events.
4. **`cave-home-unifi-talk`** — UniFi Talk VoIP intercom. The
   public API surface is **limited**; this crate ports what HA
   exposes and stops there. Full Talk parity is a future
   stretch goal once Ubiquiti opens more of the API.

All four are **Apache-2.0 line-by-line** ports — both the HA
integrations and the public Ubiquiti REST / WebSocket surface
they call.

## Consequences

### Accepted gains

- **Headline persona is covered.** UniFi is what's actually in
  the §2 family's home; cave-home now speaks to all of it as
  first-class.
- **Camera convergence.** Users with UniFi Protect cameras and
  Frigate cameras see one camera pillar in the Portal, not two.
- **Grandma-friendly UX preserved.** Per ADR-007 / Charter §6.3,
  the UI never names UniFi controller endpoints, REST API
  surfaces, or WebSocket subscriptions — it shows "Wi-Fi
  sağlıklı", "Salon kamerası", "Ön kapı".
- **Apache-2.0 throughout.** No clean-room overhead for this
  ecosystem.

### Accepted costs

- **Four sub-crates, four port surfaces.** Each Ubiquiti product
  has its own auth model, its own pagination quirks, its own
  WebSocket dialect. Engineering cost scales with surface
  count.
- **UniFi Talk is an under-documented API.** The crate's port
  ceiling is whatever Ubiquiti makes public; parity with the
  native Talk app is not promised.
- **Vendor lock-in admission.** Charter §9 (privacy-first /
  OSS-first) is about *cave-home* not requiring a vendor
  account in its own critical path — porting UniFi does not
  violate that, but it does mean a cave-home user who buys
  UniFi hardware is buying into Ubiquiti's cloud model
  separately. This is recorded here so the limitation is
  visible.
- **Cave Runtime independence (Charter §5.1).** Cave Runtime
  does not ship UniFi crates; if it ever does, the cave-home
  port stays scratch-reimplemented — no code shared.

## Alternatives considered

### (a) Defer UniFi to community add-ons

Wait until the cave-home add-on / HACS-class ecosystem is
mature, then let community ports of HA UniFi land there.

- **Rejected.** UniFi is too common in the headline persona's
  home to be a second-class surface. Add-on quality varies; the
  first-impression UX matters for §2 persona 1.

### (b) Port only UniFi Network + Protect; defer Access + Talk

Capture the 80% (network + cameras) and skip the niche
products.

- **Rejected.** Access + Talk are small surfaces; the
  marginal cost of porting them with Network + Protect is low,
  and a "smart-home cluster owner" (§2.5) with an
  Access-enabled house benefits day one.

### (c) Single `cave-home-unifi` crate, four modules inside

Avoid the four-sub-crate proliferation.

- **Rejected.** The four surfaces have meaningfully different
  release cadences and API stability; sub-crates let each move
  at its own tempo without rebuilding the whole UniFi pillar.

## Open questions

1. **UniFi controller co-location.** Should cave-home support
   running an embedded UniFi-style controller (via the
   line-by-line port), or only client-mode against a Ubiquiti-
   hosted controller? Recorded for a future ADR; initial Phase
   is client-mode only.
2. **UniFi Protect ↔ Frigate UX overlap.** Both render cameras
   to the same Portal surface; do automation triggers fire from
   the underlying inference (Frigate detector vs UniFi smart
   detection) transparently, or does the user pick? Defer to
   the camera-inference ADR (ADR-014).
