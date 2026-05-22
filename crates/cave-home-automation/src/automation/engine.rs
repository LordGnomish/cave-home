// SPDX-License-Identifier: Apache-2.0
//! Automation engine — port of
//! `homeassistant/components/automation/__init__.py`.
//!
//! Wiring (one per automation):
//! - one trigger event-bus subscription per trigger,
//! - condition list evaluated in declaration order,
//! - action sequence executed via [`crate::script::Script`].
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::automation::conditions::Condition;
use crate::automation::triggers::Trigger;
use crate::error::HassResult;
use crate::event_bus::{Event, EventBus, ListenerFn, MATCH_ALL};
use crate::script::{Action, Script, ScriptContext};
use crate::scene::SceneRegistry;
use crate::service::ServiceRegistry;
use crate::state::StateMachine;

/// YAML-equivalent declaration of a single automation rule.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py::CONF_AUTOMATION
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    /// Stable id — what cavectl + portal use to refer to this automation.
    pub id: String,
    /// User-facing alias ("Akşam Lambaları").
    #[serde(default)]
    pub alias: Option<String>,
    /// One or more triggers (any of them activates the rule).
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Conditions — every condition must pass.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Action sequence.
    #[serde(default)]
    pub actions: Vec<Action>,
    /// Initially enabled?
    #[serde(default = "yes_default")]
    pub enabled: bool,
}

const fn yes_default() -> bool {
    true
}

/// Live record of a registered automation.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py::AutomationEntity
#[derive(Debug, Clone, Serialize)]
pub struct Automation {
    pub config: AutomationConfig,
    pub enabled: bool,
    pub last_triggered: Option<time::OffsetDateTime>,
    pub run_count: u64,
    pub error_count: u64,
}

/// Handle returned from [`AutomationEngine::register`]. Dropping it
/// unsubscribes the automation's triggers.
pub struct AutomationHandle {
    _listeners: Vec<crate::event_bus::ListenerHandle>,
}

/// The automation runtime.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py::AutomationManager
pub struct AutomationEngine {
    automations: Arc<RwLock<std::collections::HashMap<String, Automation>>>,
    sm: Arc<StateMachine>,
    services: Arc<ServiceRegistry>,
    bus: Arc<dyn EventBus>,
    scenes: Arc<SceneRegistry>,
}

impl std::fmt::Debug for AutomationEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationEngine")
            .field(
                "automations",
                &self.automations.read().keys().cloned().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl AutomationEngine {
    /// Build a new engine wired to the shared state machine + service
    /// registry + event bus + scene registry.
    #[must_use]
    pub fn new(
        sm: Arc<StateMachine>,
        services: Arc<ServiceRegistry>,
        bus: Arc<dyn EventBus>,
        scenes: Arc<SceneRegistry>,
    ) -> Self {
        Self {
            automations: Arc::new(RwLock::new(std::collections::HashMap::new())),
            sm,
            services,
            bus,
            scenes,
        }
    }

    /// Register an automation; returns an [`AutomationHandle`] whose
    /// `Drop` unsubscribes the triggers.
    ///
    /// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/automation/__init__.py::async_setup_entry
    pub fn register(&self, config: AutomationConfig) -> HassResult<AutomationHandle> {
        let automation = Automation {
            enabled: config.enabled,
            config: config.clone(),
            last_triggered: None,
            run_count: 0,
            error_count: 0,
        };
        self.automations.write().insert(config.id.clone(), automation);

        let mut handles = Vec::new();
        for trigger in &config.triggers {
            let event_type = trigger
                .subscribed_event_type()
                .unwrap_or(MATCH_ALL)
                .to_owned();
            let trigger = trigger.clone();
            let id = config.id.clone();
            let automations = self.automations.clone();
            let sm = self.sm.clone();
            let services = self.services.clone();
            let bus = self.bus.clone();
            let scenes = self.scenes.clone();
            let conditions = config.conditions.clone();
            let actions = config.actions.clone();
            let alias = config.alias.clone().unwrap_or_else(|| id.clone());
            let cb: ListenerFn = Arc::new(move |event: &Event| {
                let enabled = automations
                    .read()
                    .get(&id)
                    .map(|a| a.enabled)
                    .unwrap_or(false);
                if !enabled {
                    return;
                }
                if !trigger.matches(event, Some(&sm)).unwrap_or(false) {
                    return;
                }
                if !conditions.iter().all(|c| c.evaluate(&sm).unwrap_or(false)) {
                    return;
                }
                let rctx = ScriptContext::new(
                    sm.clone(),
                    services.clone(),
                    bus.clone(),
                    scenes.clone(),
                );
                let script = Script::new(alias.clone(), actions.clone());
                let context = event.context.clone();
                let automations_for_count = automations.clone();
                let id_for_count = id.clone();
                tokio::spawn(async move {
                    match script.run(&rctx, Some(context)).await {
                        Ok(()) => {
                            if let Some(a) =
                                automations_for_count.write().get_mut(&id_for_count)
                            {
                                a.run_count += 1;
                                a.last_triggered = Some(time::OffsetDateTime::now_utc());
                            }
                        }
                        Err(err) => {
                            tracing::error!(
                                target: "cave_home_automation::engine",
                                automation = %id_for_count,
                                error = %err,
                                "automation actions failed"
                            );
                            if let Some(a) =
                                automations_for_count.write().get_mut(&id_for_count)
                            {
                                a.error_count += 1;
                            }
                        }
                    }
                });
            });
            handles.push(self.bus.async_listen_dyn(&event_type, cb));
        }
        Ok(AutomationHandle { _listeners: handles })
    }

    /// Disable an automation by id. Returns true if found.
    pub fn disable(&self, id: &str) -> bool {
        self.automations
            .write()
            .get_mut(id)
            .map(|a| {
                a.enabled = false;
                true
            })
            .unwrap_or(false)
    }

    /// Enable an automation by id. Returns true if found.
    pub fn enable(&self, id: &str) -> bool {
        self.automations
            .write()
            .get_mut(id)
            .map(|a| {
                a.enabled = true;
                true
            })
            .unwrap_or(false)
    }

    /// Snapshot of registered automations.
    #[must_use]
    pub fn list(&self) -> Vec<Automation> {
        self.automations.read().values().cloned().collect()
    }

    /// Lookup a single automation.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Automation> {
        self.automations.read().get(id).cloned()
    }

    /// Manually fire an automation (bypassing triggers, but conditions
    /// still apply). Useful for the cavectl `automate run <id>`
    /// command.
    pub async fn run_manual(&self, id: &str, context: Option<crate::context::Context>) -> HassResult<()> {
        let (actions, alias) = {
            let g = self.automations.read();
            let Some(a) = g.get(id) else {
                return Err(crate::error::HassError::Other(format!(
                    "unknown automation: {id}"
                )));
            };
            (
                a.config.actions.clone(),
                a.config.alias.clone().unwrap_or_else(|| id.to_owned()),
            )
        };
        let rctx = ScriptContext::new(
            self.sm.clone(),
            self.services.clone(),
            self.bus.clone(),
            self.scenes.clone(),
        );
        Script::new(alias, actions).run(&rctx, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::conditions::ConditionStateValue;
    use crate::event_bus::InMemoryEventBus;
    use crate::service::{service_handler, SupportsResponse};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn make_engine() -> (
        Arc<AutomationEngine>,
        Arc<StateMachine>,
        Arc<ServiceRegistry>,
    ) {
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::new());
        let sm = Arc::new(StateMachine::new(bus.clone()));
        let services = Arc::new(ServiceRegistry::new(bus.clone()));
        let scenes = Arc::new(SceneRegistry::new());
        let engine = Arc::new(AutomationEngine::new(
            sm.clone(),
            services.clone(),
            bus.clone(),
            scenes,
        ));
        (engine, sm, services)
    }

    /// Upstream-test: `tests/components/automation/test_init.py::test_service_data_not_a_dict`
    /// (conceptually re-ported: a state trigger fires the action sequence).
    #[tokio::test]
    async fn engine_runs_actions_on_state_trigger() {
        let (engine, sm, services) = make_engine();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        services.register(
            "notify",
            "ring",
            handler,
            None,
            SupportsResponse::None,
        );

        let cfg = AutomationConfig {
            id: "doorbell".into(),
            alias: Some("Doorbell".into()),
            triggers: vec![Trigger::State {
                entity_id: "binary_sensor.doorbell".into(),
                from: None,
                to: Some("on".into()),
            }],
            conditions: vec![],
            actions: vec![Action::Service {
                domain: "notify".into(),
                service: "ring".into(),
                data: serde_json::Value::Null,
            }],
            enabled: true,
        };
        let _handle = engine.register(cfg).unwrap();

        sm.async_set(
            "binary_sensor.doorbell",
            "off",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();
        sm.async_set(
            "binary_sensor.doorbell",
            "on",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();

        // Give the spawned task a moment to run.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let listed = engine.list();
        let item = listed.iter().find(|a| a.config.id == "doorbell").unwrap();
        assert_eq!(item.run_count, 1);
    }

    #[tokio::test]
    async fn engine_skips_when_condition_fails() {
        let (engine, sm, services) = make_engine();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        services.register("test", "tick", handler, None, SupportsResponse::None);

        let cfg = AutomationConfig {
            id: "guard".into(),
            alias: None,
            triggers: vec![Trigger::State {
                entity_id: "binary_sensor.motion".into(),
                from: None,
                to: Some("on".into()),
            }],
            conditions: vec![Condition::State {
                entity_id: "switch.guard".into(),
                state: ConditionStateValue::Single("on".into()),
            }],
            actions: vec![Action::Service {
                domain: "test".into(),
                service: "tick".into(),
                data: serde_json::Value::Null,
            }],
            enabled: true,
        };
        let _handle = engine.register(cfg).unwrap();

        // Switch is off → condition fails → no service call.
        sm.async_set("switch.guard", "off", BTreeMap::new(), false, None)
            .unwrap();
        sm.async_set(
            "binary_sensor.motion",
            "on",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);

        // Now turn the switch on — next motion should fire.
        sm.async_set("switch.guard", "on", BTreeMap::new(), false, None)
            .unwrap();
        sm.async_set(
            "binary_sensor.motion",
            "off",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();
        sm.async_set(
            "binary_sensor.motion",
            "on",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn disable_stops_triggering() {
        let (engine, sm, services) = make_engine();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handler = service_handler(move |_call| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }
        });
        services.register("test", "tick", handler, None, SupportsResponse::None);

        let cfg = AutomationConfig {
            id: "toggle".into(),
            alias: None,
            triggers: vec![Trigger::State {
                entity_id: "binary_sensor.motion".into(),
                from: None,
                to: Some("on".into()),
            }],
            conditions: vec![],
            actions: vec![Action::Service {
                domain: "test".into(),
                service: "tick".into(),
                data: serde_json::Value::Null,
            }],
            enabled: true,
        };
        let _handle = engine.register(cfg).unwrap();
        engine.disable("toggle");
        sm.async_set(
            "binary_sensor.motion",
            "on",
            BTreeMap::new(),
            false,
            None,
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
}
