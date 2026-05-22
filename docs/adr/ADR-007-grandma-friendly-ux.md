# ADR-007 — Grandma-friendly UX mandate

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

Charter §2 already declared cave-home's first-rank target user is
the **non-technical home resident / family member**. The
implementation underneath, however, is a multi-pillar stack —
K3s + Home Assistant core + Zigbee + Matter + Z-Wave + Frigate
camera + voice + 11 orchestration sub-crates + 4 solar crates. If
even a sliver of that stack surfaces in the UI, the headline
persona's adoption is zero on day one.

The risk is concrete: existing smart-home platforms regularly
leak implementation terminology into UX (Home Assistant's
"Integrations" / "Automations YAML" / "Add-Ons", Umbrel's
docker container surface, TrueNAS's pool / dataset / vdev
vocabulary). cave-home cannot afford the same drift — Charter §2
is meaningless without a corresponding UI mandate.

This ADR fixes that gap before the Portal and Mobile app have
landed any UI surface. The mandate is elevated to **Charter §6.3**
(golden-rule level) so that no future implementation decision can
breach it without amending the charter.

## Decision

**The cave-home Portal and Mobile app hide every implementation
detail from the end-user.** The UI uses only home-world
vocabulary; the technical stack underneath is never named in the
user-visible surface.

Three concrete rules:

1. **Vocabulary lock.** UI strings are drawn from a normative
   home-world vocabulary (device, room, automation, scene, solar
   production, security, family, hub). The full translation
   matrix lives in `docs/ui-language.md` and is updated whenever
   a new technical concept needs surfacing. Any UI surface
   shipping a term not on the table requires a docs PR adding it.
2. **No technical leakage.** The UI **never** surfaces: K3s /
   Kubernetes terminology (pod, deployment, kubelet, scheduler,
   apiserver, etcd, kine, RBAC, namespace), container or
   orchestration concepts, MQTT topic / QoS / retain, Modbus
   registers, Zigbee channel / PAN-ID, manifest YAML, Helm chart
   internals, raw certificate / token / join-URL strings. The
   implementation can be all of those — the UI cannot.
3. **Expert escape hatch.** A **Settings → "Developer view"
   toggle** unlocks technical pages (cluster topology, container
   logs, manifest editor, raw MQTT traffic). The toggle is
   **off by default**. The Mobile app does **not** expose the
   toggle at all — Developer view is Portal-only.

### Concrete UX flows (illustrative)

The mandate is best understood by example. The flows below are
**how the headline persona experiences these operations**; what
runs under the covers is the existing stack from earlier ADRs.

#### Adding a device

> "Cihazı eve ekle" → cave-home scans Zigbee / Z-Wave / Matter /
> Wi-Fi automatically (5s) → device picker with image + name
> suggestion → "Bu lambayı 'Salon' odasına ekle?" → Onayla.

Pairing keys, network keys, channels are **never** asked of the
user.

#### Adding a second hub (ADR-005 hybrid implementation)

> "Yeni hub ekle" → primary hub renders a QR code → user powers
> on the new hub (flashed cave-home OS Pi) → new hub reads the
> primary's QR through its camera, *or* the user shows the new
> hub's QR to the primary → join is automatic.

The token, certificate, and join URL are **never** shown to the
user.

#### Error messages

✅ "Hub'ın internete bağlanamadı. Wi-Fi şifreni kontrol et."
✅ "Cihaz 5 dakikadır cevap vermiyor. Pili değişebilir mi?"
❌ "DNS resolution failed for upstream A record; NXDOMAIN
   returned for ntp.cave-home.local."

Stack traces / kernel messages / pod IDs surface only in
**Developer view**, never in user-facing error toasts.

### i18n mandate

**TR + EN + DE from M1.** Burak's home (Iphofen / Germany) is
mixed-language; the headline persona in a similar home cannot
tolerate an English-only first release. All Portal / Mobile
strings are i18n-aware from day one; M1 ships with all three
locales, and the home-world vocabulary table in
`docs/ui-language.md` is the canonical source for translations.

## Consequences

### Accepted gains

- **Adoption headroom.** The headline persona can use cave-home
  without prior knowledge of any underlying technology.
- **NPS / user satisfaction** are aligned with the Charter §1
  vision ("safest, most privacy-respecting"). A privacy-first
  hub the family can't operate is privacy-first in name only.
- **Brand consistency.** "cave-home" can be talked about by
  family-friendly press / reviewers without explaining
  Kubernetes.

### Accepted costs

- **Every feature is designed in two vocabularies.** Engineers
  build the technical surface; UX writes the home-world
  rendition. Iteration cost rises; some features take longer
  because the UI cannot ship until the translation lands.
- **Bug reports become harder.** A user saying "the hub
  disconnected" is signal-poor compared to "kubelet returned
  NotReady". Mitigation: **Developer view** can attach an
  anonymised debug bundle to a user-initiated report. Default
  reports stay in home-world vocabulary.
- **Some power-user workflows hide behind a toggle.** Homelab
  users (§2.4) and cluster owners (§2.5) lose first-class UI
  prominence. Accepted because the headline persona's first
  impression is non-negotiable.

## Alternatives considered

### (a) Full expert UI (Home Assistant-style)

Every implementation surface is exposed in the UI; users learn
the vocabulary.

- **Rejected.** Hostile to the headline persona (Charter §2).
  Home Assistant's market is exactly users who *do* climb that
  learning curve; cave-home is for those who shouldn't have to.

### (b) Hybrid: home-world default + expert toggle *(chosen)*

Default UI is home-world vocabulary only; Developer view toggle
opens technical pages for power users.

- **Chosen.** Captures both audiences without compromising the
  headline experience.

### (c) Home-world UI only, no expert mode at all

Strict no-expert-surface posture.

- **Rejected.** Burak (§2.3) and homelabbers (§2.4) are
  legitimate audiences that need raw access; no escape hatch
  pushes them off the platform.

## Notes — relationship to earlier ADRs

The grandma-friendly mandate is the **UX projection** of earlier
architectural decisions:

- ADR-001 (scope) — the smart-home pillars exist *for* the
  family-resident persona; the grandma-friendly UX is what makes
  that scope coherent in practice.
- ADR-004 (K3s line-by-line port) — K3s is the *implementation*;
  the UI never says "K3s", "pod", "kubelet", etc. ADR-004's
  Consequences are amended to record this.
- ADR-005 (Hybrid deployment) — the OS-image / CLI / Portal
  flows all converge on a user-facing QR-code / token-share /
  IP-picker surface. Raw join URLs are Developer-view-only.
- ADR-006 (Mobile app stack) — the grandma-friendly mandate
  raises the bar on native-class UX. The Mobile-app stack
  decision should weigh this; ADR-006 alternatives now flag
  **(b) Flutter as the recommended candidate** (native-class
  UX, OS-conformant design language, broad i18n tooling).
