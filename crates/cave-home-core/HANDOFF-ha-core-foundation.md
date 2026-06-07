# cave-home-core — HA Core architectural foundation (handoff)

**Branch:** `feature/ha-core-port-scaffold` (worktree `../cave-home-core-port`),
merged `--no-ff` into local integration branch. **Not pushed.**
**Upstream:** `github.com/home-assistant/core` (Apache-2.0 → cave-home Apache-2.0, port OK).
**Date:** 2026-06-07.

This ray is the **initial architecture phase** of the multi-month HA Core port.
It lands the bottom-of-the-graph abstractions every future domain port
(light, switch, climate, sensor, media_player, …) will build on. Per-domain
ports are subsequent dispatches.

## What landed (all real + TDD, no stubs)

The crate already shipped the four core primitives + state machine + service
registry (`context`, `event`, `event_bus`, `state`, `state_machine`,
`service` — 30 tests). This ray added the surrounding architecture:

| Module | Upstream | What |
| --- | --- | --- |
| `entity` | `helpers/entity.py` | `Entity` trait (HA property surface + defaults), `DeviceInfo`, `EntityCategory`, `state_snapshot` folding (`_async_write_ha_state` rules) |
| `util` | `util/__init__.py` | `slugify`, `ensure_unique_string` (`_2`/`_3` suffixing) |
| `area_registry` | `helpers/area_registry.py` | slug-id alloc, normalised dup rejection, rename-collision guard |
| `device_registry` | `helpers/device_registry.py` | identifier/connection matching + metadata merge, `via_device`→id, config-entry union |
| `entity_registry` | `helpers/entity_registry.py` | idempotent `(domain,platform,unique_id)`→`entity_id`, domain-scoped suffixing, disable/hide/rename |
| `template` | `helpers/template.py` | minijinja (a Jinja2 impl) + `states`/`is_state`/`state_attr`/`is_state_attr` over the live `StateMachine` |
| `automation` | `components/automation` + `helpers/{trigger,condition,script}` | event/state/numeric triggers, state/numeric/template/and/or/not conditions, action→`ServiceCall` with child-`Context` |
| `core_context` | `core.HomeAssistant` | `CoreContext` — the `hass` bundle (bus/states/services + registries), Clone-shared |
| `loader` | `loader.py` + `setup.py` | `Manifest`, `Integration` trait (the **plug-in seam** for cave-home-{freeathome,unifi,hue}), domain registry, topo `setup_order`, dependency-cascade `setup_all` |
| `config` | `config.py` + `core_config.py` | voluptuous-style `Schema` validator + typed `CoreConfig::from_yaml` (range checks, `imperial`→`us_customary`) |
| `helpers::zone` | `components/zone` | haversine `distance`/`contains`, `active_zone` (smallest non-passive) |
| `helpers::person` | `components/person` | presence precedence `home > zone > not_home > unknown`, tracker registry |
| `helpers::scene` | `components/homeassistant/scene` | `apply` writes targets under one shared child `Context` |

### The integration seam (freeathome / unifi / hue)

`loader::Integration` is the plug-in point. An external crate ships:

```rust
struct FreeAtHome { manifest: Manifest }
impl cave_home_core::Integration for FreeAtHome {
    fn manifest(&self) -> &Manifest { &self.manifest }
    fn setup(&self, ctx: &CoreContext) -> Result<bool, SetupError> {
        // register services, seed states, create devices/entities via ctx.*
        Ok(true)
    }
}
```

`IntegrationLoader` resolves dependency order, then `setup_all(&ctx)` drives
each `setup`, skipping integrations whose hard dependency failed.

## Acceptance criteria — status

- ✅ `cargo test` passes — **107 / 107** (was 30; +77 this ray), 0 ignored.
- ✅ Mock event-bus + service-call test — `core_context` bus observation,
  `automation` produces `ServiceCall`s, `service` registry events.
- ✅ `Entity` trait + domain-registry test — `entity::tests`,
  `loader::tests` (`domains()` is the domain registry).
- ✅ Automation rule trigger test (basic if-then) —
  `automation::tests::basic_if_then_produces_service_call` +
  `conditions_gate_the_action`.
- ✅ LOC ratio report — below.
- ✅ TDD git log — 7 RED→GREEN commit pairs (`test(...): RED` → `feat(...): GREEN`).

## LOC ratio report (HA Core Python vs Rust port)

Rust port LOC (this crate, `cargo`-counted):

```
This ray (new modules):     3,826 total  (~2,407 impl / ~1,419 test)
Pre-existing core:          1,133 total  (~691 impl)
cave-home-core whole:       4,959 total  ·  107 tests
```

Approximate upstream Python LOC for the *corresponding subsystems* (estimated
from the HA codebase; full files, incl. features deferred past this foundation):

```
core.py                                       ~3,500
helpers/entity.py                             ~1,600
helpers/entity_registry.py                    ~2,100
helpers/device_registry.py                    ~1,700
helpers/area_registry.py                        ~700
helpers/template.py                           ~2,900
automation + trigger + condition + script     ~5,500
config.py + core_config.py                    ~2,600
loader.py + setup.py                          ~2,800
components/zone | person | scene                ~2,650
util (slugify etc.)                              ~300
                                            ─────────
upstream subsystem surface (approx)          ~26,350
```

**Ratio:** ~4,959 Rust LOC against ~26.4 kLOC of upstream Python subsystem
surface ≈ **~19%** by LOC (or ~9% counting only the ~2.4k impl LOC added this
ray + 0.7k pre-existing impl against the upstream total). This is the
*architectural foundation*, not parity — the remaining ~80% is per-domain
platforms, the websocket/REST API, the async service-execution scheduler,
recorder/history, config-flow, and the long tail of trigger/condition/template
features intentionally deferred. The decision-core semantics that those layers
sit on are present and tested. (Upstream numbers are approximate.)

## Honesty notes / deferrals

- **Service *execution*** (`async_call` scheduling, target expansion,
  blocking/return_response) stays deferred — `automation` *emits* `ServiceCall`s;
  it does not run them. Matches the existing `service.rs` parity note.
- **Template surface** is the state-access globals (the automation/condition
  workhorses), not the full ~40 HA filters/tests. minijinja chosen over Tera
  because HA *is* Jinja2.
- **`AutomationMode`** is modelled for fidelity but not enforced (no async
  overlap scheduler in the sync core).
- **Config schema** is a voluptuous-style subset (4 scalar types + required/
  default/extra-policy), not full `vol`/JSON-Schema; `CoreConfig` is the one
  concrete typed block.
- Offline build: adds `minijinja` (cached 2.20.0) + `serde_yaml` (cached
  0.9.34); both resolve `--offline`.

## Clippy

`cargo clippy -p cave-home-core --lib` is the gate. New modules add **only**
`clippy::significant_drop_tightening` (nursery) on `parking_lot` guards — the
identical lint the pre-existing `event_bus`/`state_machine` already trip. No
new warning *category* introduced; `--lib` test+impl all green.

## Next dispatches

1. Per-domain entity ports (light/switch/climate/sensor/…) implementing `Entity`.
2. Service-execution scheduler (the deferred `async_call` half).
3. Websocket/REST API surface over `CoreContext`.
4. Wire `cave-home-{freeathome,unifi,hue}` to `Integration` + `setup_all`.
