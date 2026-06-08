# Coverage matrix — cave-home-core

**Declared:** fill=0.46 · adr_justified=not-declared · honest=not-declared · port method: line-by-line per manifest.
**Verified:** 8/8 mapped symbols found in source · 9 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Context — event tracing primitive | src/context.rs::Context | yes |
| Event + EventOrigin — event data model | src/event.rs::Event + EventOrigin | yes |
| State — entity state snapshot | src/state.rs::State | yes |
| EntityId — entity identifier (domain.object_id) | src/state.rs::EntityId | yes |
| EventBus — pub/sub event routing (listen, listen_once, fire) | src/event_bus.rs::EventBus | yes |
| MATCH_ALL wildcard constant | src/event_bus.rs::MATCH_ALL | yes |
| StateMachine — authoritative state store (set, get, remove) | src/state_machine.rs::StateMachine | yes |
| EVENT_STATE_CHANGED constant | src/state_machine.rs::EVENT_STATE_CHANGED | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| HomeAssistant + bootstrap | phase-1b | Top-level runtime + executor; deferred until components can register |
| ServiceRegistry | phase-1b | Service call dispatch; deferred alongside integrations crate |
| Component lifecycle (async_setup / async_unload) | phase-1b | Needs integrations crate to host components |
| Config + config_entries | phase-1b | Config layer; Phase 1b priority |

## Drift notes
None — every claimed symbol exists in source. All 8 mapped entries verified. Manifest's fill_ratio 0.46 is supported by actual implementation (Context, Event, EventBus, State, StateMachine primitives + query API methods on StateMachine).

## Additional implementation scope
StateMachine includes unpromoted helper methods: entity_ids(), entity_ids_by_domain(), all() — these mirror HA's query API and expand the effective scope beyond the mapped section but remain unlisted in manifest. One additional test (entity_ids_all_and_domain_query) validates this extended API.
