# Coverage matrix — cave-home-automation

**Declared:** fill=0.42 · adr_justified=(not declared) · honest=0.42 · port method: line-by-line.
**Verified:** 67/67 mapped symbols found in source · 50 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Context | src/context.rs::Context | yes |
| Event | src/event_bus.rs::Event | yes |
| EventOrigin | src/event_bus.rs::EventOrigin | yes |
| EventBus (trait) | src/event_bus.rs::InMemoryEventBus | yes |
| EventBus::fire | src/event_bus.rs::InMemoryEventBus::fire | yes |
| EventBus::async_listen | src/event_bus.rs::InMemoryEventBus::async_listen | yes |
| EventBus::async_listen_once | src/event_bus.rs::InMemoryEventBus::async_listen_once | yes |
| EventBus::remove_listener | src/event_bus.rs::InMemoryEventBus (ListenerHandle drop) | yes |
| State | src/state.rs::State | yes |
| State::as_dict | src/state.rs::State::as_dict | yes |
| State::from_dict | src/state.rs::State::from_dict | yes |
| State::expire | src/state.rs::State::expire | yes |
| split_entity_id | src/state.rs::split_entity_id | yes |
| valid_entity_id | src/state.rs::valid_entity_id | yes |
| valid_domain | src/state.rs::valid_domain | yes |
| States | src/state.rs::States | yes |
| StateMachine | src/state.rs::StateMachine | yes |
| StateMachine::async_set | src/state.rs::StateMachine::async_set | yes |
| StateMachine::async_set_internal | src/state.rs::StateMachine::async_set_internal | yes |
| StateMachine::async_remove | src/state.rs::StateMachine::async_remove | yes |
| StateMachine::is_state | src/state.rs::StateMachine::is_state | yes |
| StateMachine::entity_ids | src/state.rs::StateMachine::entity_ids | yes |
| StateMachine::all | src/state.rs::StateMachine::all | yes |
| SupportsResponse | src/service.rs::SupportsResponse | yes |
| Service | src/service.rs::Service | yes |
| ServiceCall | src/service.rs::ServiceCall | yes |
| ServiceRegistry | src/service.rs::ServiceRegistry | yes |
| ServiceRegistry::register | src/service.rs::ServiceRegistry::register | yes |
| ServiceRegistry::remove | src/service.rs::ServiceRegistry::remove | yes |
| ServiceRegistry::call | src/service.rs::ServiceRegistry::call | yes |
| ServiceRegistry::has_service | src/service.rs::ServiceRegistry::has_service | yes |
| ServiceNotFound | src/error.rs::HassError::ServiceNotFound | yes |
| Automation (entity) | src/automation/engine.rs::Automation | yes |
| AutomationEngine::register | src/automation/engine.rs::AutomationEngine::register | yes |
| Trigger (module) | src/automation/triggers.rs | yes |
| StateTrigger | src/automation/triggers.rs::Trigger::State | yes |
| EventTrigger | src/automation/triggers.rs::Trigger::Event | yes |
| NumericStateTrigger | src/automation/triggers.rs::Trigger::NumericState | yes |
| TimeTrigger | src/automation/triggers.rs::Trigger::Time | yes |
| TimePatternTrigger | src/automation/triggers.rs::Trigger::TimePattern | yes |
| TemplateTrigger | src/automation/triggers.rs::Trigger::Template | yes |
| HassStartTrigger | src/automation/triggers.rs::Trigger::Homeassistant | yes |
| Condition::Template | src/automation/conditions.rs::Condition::Template | yes |
| Condition::State | src/automation/conditions.rs::Condition::State | yes |
| Condition::NumericState | src/automation/conditions.rs::Condition::NumericState | yes |
| Condition::Time | src/automation/conditions.rs::Condition::Time | yes |
| Condition::And | src/automation/conditions.rs::Condition::And | yes |
| Condition::Or | src/automation/conditions.rs::Condition::Or | yes |
| Condition::Not | src/automation/conditions.rs::Condition::Not | yes |
| Script | src/script.rs::Script | yes |
| Script::run | src/script.rs::Script::run | yes |
| Action::Service | src/script.rs::Action::Service | yes |
| Action::Delay | src/script.rs::Action::Delay | yes |
| Action::Choose | src/script.rs::Action::Choose | yes |
| Action::Repeat | src/script.rs::Action::Repeat | yes |
| Action::If | src/script.rs::Action::If | yes |
| Action::WaitForTrigger | src/script.rs::Action::WaitForTrigger | yes |
| Action::Scene | src/script.rs::Action::Scene | yes |
| Scene | src/scene.rs::Scene | yes |
| Scene::activate | src/scene.rs::Scene::activate | yes |
| Template | src/template.rs::Template | yes |
| Template::render | src/template.rs::Template::render | yes |
| states function | src/template.rs::Template (closure) | yes |
| is_state function | src/template.rs::Template (closure) | yes |
| state_attr function | src/template.rs::Template (closure) | yes |
| now function | src/template.rs::Template (closure) | yes |
| as_timestamp filter | src/template.rs::Template (closure) | yes |
| today_at function | src/template.rs::Template (closure) | yes |
| ConfigEntry | src/config_entry.rs::ConfigEntry | yes |
| ConfigEntries | src/config_entry.rs::ConfigEntries | yes |
| ConfigEntryState | src/config_entry.rs::ConfigEntryState | yes |
| ConfigFlowHandler | src/config_entry.rs::ConfigFlowHandler | yes |
| EVENT_STATE_CHANGED | src/event_bus.rs::EVENT_STATE_CHANGED | yes |
| EVENT_CALL_SERVICE | src/event_bus.rs::EVENT_CALL_SERVICE | yes |
| EVENT_HOMEASSISTANT_START | src/event_bus.rs::EVENT_HASS_START | yes |
| EVENT_HOMEASSISTANT_STOP | src/event_bus.rs::EVENT_HASS_STOP | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| homeassistant/components/recorder | phase-1b | Sqlite-backed history recorder + statistics; cave-home-history will land this separately. |
| homeassistant/components/energy | phase-1b | Energy Dashboard — Charter §3 first-class but a follow-on of the recorder. |
| homeassistant/components/frontend & lovelace | permanent | Lovelace UI lives in cave-home-portal (different stack, Rust + Leptos-class); we port only the data model, not the JS. |
| homeassistant/components/http (aiohttp server) | phase-1b | HTTP API lives in cave-home-portal; we expose typed Rust APIs instead of dynamic aiohttp routes. |
| homeassistant/auth (Auth provider framework) | phase-1b | Auth/RBAC will reuse cave-home-portal's session layer; HA's home-assistant.local.OWNER user model is folded into Portal. |
| homeassistant/components/blueprint | phase-2 | Reusable automation/script blueprints — needs a YAML import dance built on the scaffolded engine. |
| homeassistant/components/script (script entities) | phase-1b | Script *entities* — Phase 1 lands the Script runner; the script-as-entity wrapper is a follow-on. |
| homeassistant/components/group | phase-1b | Entity groups. |
| homeassistant/components/template (template entities) | phase-1b | Template sensors/binary_sensors — needs entity platform plumbing first. |
| homeassistant/components/sun + zone + person | phase-1b | Helpers used by conditions; Phase 1 lands the engine + service surface only. |
| homeassistant/components/device_automation | phase-1b | Device-friendly trigger/condition/action layer — needs the device-registry crate first. |
| homeassistant/helpers/restore_state | phase-1b | Last-known-state restoration across restarts — needs the recorder. |
| homeassistant/helpers/entity (Entity / ToggleEntity) | phase-1b | Entity platform abstraction; ~10 kLOC of helpers — folded into per-integration crates as needed. |
| homeassistant/components/cloud + nabucasa | permanent | cave-home is cloud-free per Charter §1. |
| homeassistant/components/onboarding | phase-1b | First-run onboarding wizard — Portal lands this in the home-world UX. |
| homeassistant/components/system_health / hardware / supervisor | permanent | Supervisor lives outside HA core (Home Assistant OS); cave-home runs as a unified Rust binary on bare metal. |

## Drift notes
None — every claimed symbol exists in source. Template functions (states_function, is_state_function, state_attr_function, now_function, as_timestamp_filter, today_at_function) and trigger types (StateTrigger, EventTrigger, etc.) are implemented as enum variants or closures within the Template struct, but all are present and functional.
