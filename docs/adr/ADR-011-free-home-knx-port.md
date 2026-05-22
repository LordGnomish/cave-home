# ADR-011 — Busch-Jaeger free@home + KNX-IP port

## Status

**Accepted** — 2026-05-15, finalised by Burak Tartan (founder).

Created: 2026-05-15
Supersedes: —
Superseded by: —

## Context

**Busch-Jaeger free@home** is the dominant building-automation
brand in German residential construction, which makes it
table-stakes for the Charter §2 persona 1–2 user living in
Iphofen / Germany (primary user: Burak Tartan). free@home is a
hybrid system: a residential REST-controllable surface on top of
the underlying **KNX-IP** building-automation bus.

Two layers, two ports:

1. **free@home REST API client.** Public REST surface exposed by
   the System-Access-Point unit; the `free-at-home` Python
   library (Apache-2.0) and the HA `free_at_home` integration
   are reference implementations.
2. **KNX-IP stack** — because free@home rides on KNX-IP, a real
   KNX port comes along **as a bonus deliverable**. The OSS
   surface here:
   - `xknx` (MIT) — Python KNX library, line-by-line portable.
   - HA `knx` integration (Apache-2.0) — line-by-line portable.
   - **KNXd** (GPL) — the canonical KNX gateway daemon;
     clean-room mandate applies (Charter §6.1 / ADR-002).

The KNX bonus matters beyond Burak's house: KNX is the
dominant European building-automation bus, so the port unlocks
cave-home for a meaningfully larger §2-persona-1 audience than
free@home alone would.

## Decision

Two cave-home crates:

1. **`cave-home-free-home`** — Apache-2.0 **line-by-line** port
   of the HA `free_at_home` integration + the public `free-at-
   home` Python library's REST client surface. Talks to the
   Busch-Jaeger System Access Point over the documented REST
   API.
2. **`cave-home-knx`** — **mixed-source** port:
   - **xknx (MIT)** → line-by-line into the KNX-IP transport
     layer of this crate.
   - **HA `knx` integration (Apache-2.0)** → line-by-line into
     the entity / automation-engine integration of this crate.
   - **KNXd (GPL)** → **clean-room** reimplementation of the
     KNX gateway daemon surface, **from the KNX specification +
     public protocol docs only** (Charter §6.1 / ADR-002).
     Contributors must not read KNXd source.

The KNX work is sequenced *after* the free@home work — the
free@home REST surface is the proximate Charter §2 deliverable;
the KNX bonus is delivered as `cave-home-knx` matures.

## Consequences

### Accepted gains

- **Headline persona's house is covered.** Burak's residential
  free@home installation works against cave-home day-one once
  this crate lands.
- **European audience unlocked.** KNX is the *de facto* European
  building-automation standard; supporting it lets cave-home
  speak to existing wired installations without retrofitting
  Zigbee / Matter into the walls.
- **Apache-2.0 + MIT bases.** The bulk of the work (free@home +
  xknx + HA knx) is line-by-line; only the KNXd-equivalent
  gateway surface carries clean-room overhead.

### Accepted costs

- **Three derivation chains in `cave-home-knx`.** Engineers
  must track which sub-surface came from which upstream and
  apply the right port discipline (line-by-line vs clean-room).
  Recorded in `[package.metadata.cave-home]` and in the crate's
  doc-comment.
- **Clean-room recusal for KNXd contributors.** Anyone who has
  read KNXd source is barred from the daemon-surface portion of
  `cave-home-knx`. Standard Charter §6.1 protocol.
- **KNX spec access.** The KNX Association publishes a portion
  of the spec but charges for the full Application Interworking
  spec. The clean-room work is constrained to whatever is
  publicly readable + the cleanly-licensed xknx surface.
- **Vendor concentration risk.** free@home is a Busch-Jaeger
  proprietary surface on top of KNX. Busch-Jaeger may change
  the System Access Point's REST contract; the crate tracks
  upstream stable and adjusts.

### Charter §6.3 / ADR-007 compliance

The UI never says "free@home System Access Point", "KNX
group address", "KNXd gateway", "ETS programming". The user
sees rooms / devices / scenes; KNX physical addresses and
group-object structure stay in Developer view.

## Alternatives considered

### (a) free@home only; defer KNX

Port free@home, skip the KNX bonus.

- **Rejected.** free@home internally bridges to KNX-IP; the
  marginal cost of exposing the underlying KNX surface as a
  first-class cave-home capability is small, and the audience
  unlock is meaningfully bigger.

### (b) KNX-IP only via xknx + HA knx; skip KNXd-clean-room

Stay permissive-only; do not implement the gateway-daemon
surface that KNXd provides.

- **Rejected.** KNXd's role is bridging KNX-TP (twisted-pair
  bus) to KNX-IP. Users with KNX-TP-only installations need
  *some* gateway; without the equivalent, cave-home can't reach
  them. Clean-room is acceptable for this.

### (c) Bundle a KNXd binary as a sidecar

Run KNXd as a sub-process.

- **Rejected.** Charter §5 single-binary mandate (no sidecars).
  Same logic as ADR-010 Hue option (b).

## Open questions

1. **ETS / commissioning surface.** ETS is the proprietary KNX
   commissioning tool. cave-home will *not* implement ETS;
   pre-commissioned KNX installations are the assumed input.
   Recorded so contributors don't volunteer ETS scope.
2. **Busch-Jaeger Welcome doorbell.** Welcome is a separate
   Busch-Jaeger product on a related bus; out of scope for
   ADR-011 (would need its own ADR if added later).
3. **Co-existence with `cave-home-zigbee` + `cave-home-zwave`.**
   A KNX-controlled blind and a Zigbee-controlled blind in the
   same room should render as one device entity in the Portal.
   Implementation is automation-engine work; the affordance is
   inherited from the HA core port.
