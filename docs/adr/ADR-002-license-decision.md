# ADR-002 — Licence: Apache-2.0 + clean-room mandate for GPL upstreams

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

ADR-001 ("cave-home scope and positioning") left the licence call as
an explicit open question, because the smart-home upstream landscape
mixes incompatible licences:

| Cluster                                                       | Licence            |
| ------------------------------------------------------------- | ------------------ |
| home-assistant/core, project-chip, scrypted, esphome (parts)  | Apache-2.0         |
| node-zwave-js, frigate, whisper.cpp, piper                    | MIT                |
| **Zigbee2MQTT, Tasmota, esphome (parts)**                     | **GPL-3.0**        |
| eclipse/mosquitto                                             | EPL-2.0 + EDL-1.0  |

The licence cave-home itself adopts determines, for each upstream,
whether we may **port line-by-line** (which under GPL would propagate
copyleft to the whole tree) or must **reimplement from spec only**.

cave-home's goals from the Charter:

- §3 — broad pillar coverage (automation, Zigbee, Matter, Z-Wave,
  MQTT, camera, voice, …).
- §5 — single unified binary; everything links into one process.
- §6 — golden rule of line-by-line upstream parity + TDD.
- §9 — sovereign / OSS-first, no cloud lock-in.

Two licence directions were on the table:

- **GPL-3.0 / AGPL-3.0**: line-by-line port of every upstream is
  legally fine. Cost: a permissive-only ecosystem (Cargo crates,
  downstream packagers, commercial adopters) faces friction adopting
  cave-home; especially AGPL would scare a lot of homelabber-class
  reusers.
- **Apache-2.0**: permissive, patent grant, broad reuse and
  packaging. Cost: GPL upstreams (Z2M, Tasmota, parts of ESPHome)
  cannot be line-by-line ported — they must be clean-room
  reimplemented from public spec only.

## Decision

**cave-home is licensed under the Apache License 2.0 in its
entirety.** Every first-party crate, every documentation file, every
asset.

To make this licence choice safe in the face of the GPL upstreams
listed above, cave-home adopts a **clean-room reimplementation
mandate** for any GPL or other strong-copyleft upstream:

> For an upstream whose licence is GPL, AGPL, or otherwise
> incompatible with Apache-2.0 inclusion, cave-home contributors
> **do not read the upstream's source code**. They reimplement the
> functionality from the **public protocol specification, RFC,
> wire-format analysis, and public API documentation** only. They
> write their own test fixtures; they do not port the upstream's
> tests. Each clean-room crate's ADR carries an explicit
> "implemented from spec; source not read" declaration.

The matrix of per-upstream port methods is recorded in
`docs/upstream/REFERENCES.md`.

### Port-method classification

| Class             | Trigger                                       | Cave-home action                                                                 |
| ----------------- | --------------------------------------------- | -------------------------------------------------------------------------------- |
| **line-by-line**  | Upstream is Apache-2.0 / MIT / BSD            | Port directly under Charter §6; preserve NOTICE / attribution as required.       |
| **clean-room**    | Upstream is GPL-3.0 / AGPL-3.0 / strong copyleft | Reimplement from public spec only; contributor must not read upstream source. |
| **clean-room (EPL)** | Upstream is EPL-2.0 (file-level copyleft)  | Spec-based reimplementation is the documented safe path; do not vendor sources. |
| **hybrid**        | Upstream has files under mixed licences       | Per-file audit: permissive files line-by-line, copyleft files clean-room.        |
| **reference only**| Upstream is not consumed at runtime           | Used for build / OS / packaging patterns; no source porting.                     |

## Consequences

### Accepted costs

- **GPL upstreams require contributor discipline.** Zigbee2MQTT and
  Tasmota — both popular and central to the smart-home stack — must
  be reimplemented under the clean-room rule. Contributors who have
  read those repos in the past must self-select out of those crates.
- **Slower start on copyleft-only protocol stacks.** Clean-room
  reimplementation from Zigbee 3.0 / Tasmota MQTT spec is
  meaningfully slower than line-by-line porting. The roadmap
  accounts for this in M2 (Zigbee) where the discipline is most
  exercised.
- **Contribution surface narrows.** A reviewer who has read the
  upstream source cannot review that crate's clean-room PRs. We
  accept this trade for licence cleanliness.

### Accepted gains

- **Permissive licensing.** Apache-2.0 with patent grant — adopters
  (including commercial integrators, packagers, distros) can build
  on cave-home without copyleft worry.
- **Clean-room hygiene** is the same protocol commercial Apache-2.0
  projects already use against GPL competitors; it is well
  understood and defensible.
- **The cave-home tree is Apache-2.0-clean.** No accidental copyleft
  bleed; downstream tooling that depends on Apache-2.0-only
  ingestion (some corporate distros) can use cave-home without
  carve-outs.
- **No constraint on Cave Runtime.** ADR-001 §"Accepted gains"
  already noted this; ADR-002 makes it concrete: cave-home picking
  Apache-2.0 does not constrain Cave Runtime's own licence
  decision, and vice versa (Charter §5.1).

### Concrete follow-ups

1. **CHARTER.md §6** is amended to include the clean-room mandate
   verbatim.
2. **CONTRIBUTING.md** gains a "clean-room rule" section codifying
   the contributor protocol (do not read the GPL source; do not
   paste from a GPL repo; reviewers do not grep the GPL repo
   either).
3. **`docs/upstream/REFERENCES.md`** gains a `Port method` column
   on the upstream table, classifying every upstream per the
   matrix above.
4. **ADR-001** "Licence-mix warning" subsection is rewritten to
   point at ADR-002 as the answered decision.
5. **Every first-party crate** declares `license = "Apache-2.0"`
   (via workspace inheritance) and a `[package.metadata.cave-home]`
   block that records the upstream it tracks (if any) and the port
   method for that crate.

## Alternatives considered

### AGPL-3.0

- **Pro:** Strongest copyleft. Aligns with sovereignty values:
  anyone running cave-home as a network service must release
  modifications. Z2M and Tasmota would be portable line-by-line.
- **Con:** Hostile to commercial integrators and to a number of
  permissive-only Cargo distros. Would discourage exactly the
  homelabber-with-day-job audience cave-home is built for.
- **Not chosen.**

### GPL-3.0

- **Pro:** Resolves the GPL-upstream porting question by adoption.
- **Con:** Same friction for commercial reuse, without AGPL's
  network-service teeth. The middle ground that satisfies no one.
- **Not chosen.**

### Dual Apache-2.0 / GPL-3.0

- **Pro:** Lets downstream pick.
- **Con:** Operationally complex; every GPL-upstream port still
  needs the clean-room discipline anyway (because the *cave-home*
  copy is dual-licensed but the *upstream's* GPL doesn't allow
  Apache-2.0 redistribution of the ported code). No real win.
- **Not chosen.**

### Mozilla Public License 2.0

- **Pro:** File-level copyleft, friendlier than GPL.
- **Con:** Same problem with GPL upstreams (still incompatible
  with cave-home re-licensing GPL-derived ports), and worse
  ecosystem familiarity in the Cargo / Rust world than Apache-2.0.
- **Not chosen.**

## Notes

This ADR governs **first-party cave-home code**. It does not change
the obligations imposed by *runtime* third-party dependencies that
cave-home may eventually link (a Rust crate licensed under MPL, a
codec library licensed under LGPL, etc.). Runtime-link compatibility
is a separate per-dependency review and is out of scope for ADR-002.
