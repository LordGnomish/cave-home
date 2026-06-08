# Coverage matrix — cave-home-integrations

**Declared:** fill=0.35 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 11/11 mapped symbols found in source · 47 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Integration manifest: domain, name, dependencies, after_dependencies, iot_class, config_flow | src/integration.rs::{Integration,IotClass,ConfigFlow} | yes |
| Integration registry (register/get/all) keyed by domain | src/integration.rs::Registry | yes |
| Config entry instance: data, lifecycle state, unique-id, disabled_by | src/config_entry.rs::{ConfigEntry,DisabledBy} | yes |
| Unique-id dedupe (already_configured) — same device not added twice | src/config_entry.rs::is_duplicate | yes |
| Dependency + after-dependency topological setup order | src/resolver.rs::resolve_setup_order | yes |
| Dependency cycle detection + missing-dependency error | src/resolver.rs::ResolveError::{Cycle,MissingDependency} | yes |
| Discovery signal model (transport + service key + properties) and stable unique-id derivation | src/discovery.rs::{Discovered,Transport} | yes |
| Discovery-to-integration matcher + already-configured dedupe (only NEW devices offered) | src/discovery.rs::{candidates,new_candidates} | yes |
| Config-entry lifecycle state machine (NotLoaded/SettingUp/Loaded/SetupError/SetupRetry/Migrating/Unloading/Failed) as pure transitions | src/lifecycle.rs::{State,Transition,next} | yes |
| Transient (SetupRetry) vs permanent (SetupError) setup-failure classification | src/lifecycle.rs::Failure::{is_transient,outcome_state} | yes |
| Capability/platform set per integration + 'what can this hub do' aggregation across loaded entries | src/capability.rs::Capability + src/capability.rs::HubCapabilities + src/resolver.rs::hub_capabilities | yes |
| Grandma-friendly EN/DE/TR messaging, no implementation jargon (Charter §6.3, ADR-007) | src/label.rs::{found_new,connected,retrying,needs_attention,removed,already_added} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Async setup execution (actually calling each integration's connect/setup routine) | phase-1b | Engine decides the order and legal transitions; running them is async + I/O-bound. Runtime-bound, no new decision logic. |
| Config-flow wizard backend (multi-step add-a-device dialogs) | phase-1b | Descriptor already declares config-flow type; step-by-step form/validation backend lands with Portal add-device UI. |
| OAuth / account-link flows for cloud integrations | phase-1b | Account-bound and network-bound (token exchange, refresh). Per-integration flow on top of config-flow backend. |
| Discovery transports — mDNS / SSDP / DHCP listeners | phase-1b | Network-bound multicast/broadcast listeners. Transport-agnostic by design; transports emit signals this crate's matcher already consumes. |
| cave-home-core entity/platform wiring | phase-1b | Forwarding loaded entry's capabilities into core as live entities lands once core's entity API stabilises. |
| ADR-004 orchestration-layer (K3s) hand-off for HACS-style add-ons | phase-2 | Third-party add-ons run as workloads on K3s orchestration layer; cross-crate, runtime-bound concern. |
| Pre-existing-config schema-version compatibility shims | permanent | Charter §7/§8: ship one current schema only. Migration IS modelled; multi-version legacy-schema shims explicitly out of scope. |

## Drift notes
None — every claimed symbol exists in source. All 11 mapped specifications verified in their declared files. All 7 unmapped items carry explicit phase/permanent dispositions with clear rationale. Declared honest_ratio (1.00) is supported: gap_total (7) all ADR-justified, unjustified_gap = 0.
