# ADR-006 — Mobile companion app stack

## Status

**Draft** — pending finalisation by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

Charter §3 lists "Mobile companion app" as a first-class pillar.
The founder confirmed on 2026-05-14 that this is permanently
in-scope: *"haklısın mobile üzerinde uygulama ile cave home control
edilebilmeli."* The Charter §3 status is therefore "confirmed in
scope by founder 2026-05-14".

What's not yet decided is the **technology stack** for the iOS +
Android companion app. The decision matters because it shapes:

- How much Rust code can be shared with the cave-home back-end
  (cave-home is otherwise an all-Rust workspace).
- Native UX quality (animations, push notifications, background
  tasks, biometric auth, camera live-view performance).
- Developer velocity and ecosystem support.
- Long-term maintenance load — two app stores, two SDK update
  cadences, two reviewer pipelines.

Constraints that hold:

- **Charter §6 golden rule.** Whatever upstream the mobile stack
  pulls from is subject to the line-by-line / clean-room port
  rules. (For mobile *runtime* dependencies, this is a softer
  rule than for the core Rust crates — but it still applies to
  any port-shaped reuse.)
- **Charter §9 privacy-first.** No third-party push relay. Push
  notifications must traverse a cave-home back-end, not Firebase
  Cloud Messaging or Apple Push Notification service-as-account
  except where unavoidable for the OS-level pop-up.
- **Single-binary mandate (Charter §5)** does *not* apply on
  mobile — the mobile app is a separate artefact by nature. But
  per-platform code should be kept as thin as practical.

## Decision

**Pending.** Five candidate stacks recorded below. Founder will
pick one; until this ADR moves to Accepted, the `cave-home-mobile`
crate stays as an empty placeholder, no platform-specific scaffold
lands in the repo, and PRs that pre-commit to a stack are held.

## Candidate stacks

### (a) Rust + Tauri 2.x

A pure-Rust core wrapped in Tauri 2.x, which gained iOS / Android
support in late 2024. Same toolchain the rest of cave-home uses.

- **Pro:** Single-language workspace (cave-home is already all-
  Rust); shared crates with the back-end (state types, automation
  DSL, validation) Just Work via Cargo.
- **Pro:** Fast iteration; one CI matrix.
- **Pro:** Smallest mental load for cave-home contributors.
- **Con:** Mobile support is the youngest of the candidates;
  some platform features (push, geofencing, background tasks)
  still require platform-specific bridges.
- **Con:** Native UX feel can lag a true native app, especially
  on iOS where users notice deviations.

### (b) Flutter (Dart) — **recommended**

Google-backed, mature, large plugin ecosystem.

- **Pro:** Native-class UX, hot reload, broad community.
- **Pro:** Mature plugins for push, geofencing, biometrics, etc.
- **Pro:** Strongest fit for **ADR-007 grandma-friendly UX**
  mandate — OS-conformant design language, native widgets,
  broad i18n tooling for the TR + EN + DE M1 requirement.
- **Con:** Separate language (Dart); Rust core requires an FFI
  bridge (Dart-FFI or flutter-rust-bridge).
- **Con:** Two-language maintenance load.

> **Note (2026-05-14):** ADR-007 raised the bar on native-class
> UX. Flutter is currently the **recommended** candidate against
> that bar — it lands closer to the headline persona's
> expectations than (a) Tauri (whose mobile WebView UX is still
> a risk on iOS) and carries less maintenance load than (d) KMM
> (split native UI per platform). Decision still rests with the
> founder; the recommendation is recorded so contributors don't
> spin up Tauri-based mobile prototypes in the meantime.

### (c) React Native (TypeScript)

Meta-backed, large community, accessible to web developers.

- **Pro:** Large hiring pool; familiar to web devs.
- **Pro:** Mature push / background / biometrics modules.
- **Con:** TypeScript core; same FFI-bridge cost as Flutter.
- **Con:** Native-bridge performance historically the weakest
  among the four serious options for compute-heavy paths.

### (d) Kotlin Multiplatform Mobile (KMM)

Android-first, JetBrains-driven, native UI per platform (SwiftUI
on iOS).

- **Pro:** Best-native UX (real native UI on each platform).
- **Pro:** Modern, well-tooled.
- **Con:** Separate codebase for UI on each platform — the
  *shared* part is business logic only.
- **Con:** SwiftUI learning curve for the iOS UI; smaller plugin
  ecosystem than RN / Flutter.

### (e) Rust + egui / iced — experimental

A pure-Rust UI toolkit (egui or iced) cross-compiled to mobile.

- **Pro:** Maximum single-language alignment.
- **Con:** Mobile support is experimental and not production-
  ready as of 2026-05.
- **Not seriously evaluated** today; rationale captured for
  completeness.

## Consequences — to be filled in once the decision is made

When the founder picks one of (a) / (b) / (c) / (d), this section
will be filled with the accepted trade-offs. (e) is recorded but
not a serious contender at this point.

## Open questions

1. **How much of the cave-home-mobile crate is intended to be
   shared with the desktop Portal UI?** If significant, that
   tilts toward Tauri (a); if not, Flutter / KMM look stronger.
2. **Push-notification transport.** Apple APNs and Google FCM
   are the OS-level mechanisms; cave-home is privacy-first
   (Charter §9). The mobile stack does not change *whether* we
   route through them, but it does change the integration cost.
3. **Geofencing accuracy expectations.** Owntracks-class
   geofencing fits into the §3 pillar list. The mobile stack
   choice influences how cleanly we can implement it across
   iOS / Android.
4. **Multi-node UI.** The mobile app must surface the cluster
   shape (primary hub / failover / ML node — Charter §5).
   That's UX work first, stack-choice work second.

## Notes

The `cave-home-mobile` crate scaffolds as an **empty
placeholder** in this commit. It exists in the workspace so
that, once ADR-006 is Accepted, the chosen-stack scaffold can
land without re-plumbing Cargo.toml.

The crate's role description today is deliberately generic:
"companion-app shared business logic + state sync + Portal API
client". Whether that Rust code is consumed by Tauri (a), via
FFI by Flutter / RN / KMM (b/c/d), or sidelined entirely if
the stack ends up native-only, is the subject of the eventual
Accepted decision.
