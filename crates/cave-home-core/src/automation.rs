//! Port of the automation core: `homeassistant.components.automation` plus the
//! `helpers.trigger` / `helpers.condition` / `helpers.script` machinery it
//! drives, reduced to its synchronous decision logic.
//!
//! An [`AutomationRule`] is the familiar trigger → condition → action chain.
//! [`AutomationEngine::handle_event`] takes an [`Event`] from the bus (today
//! the `state_changed` events the [`StateMachine`](crate::state_machine) fires,
//! or any custom event), evaluates every rule, and returns the
//! [`ServiceCall`]s the matching rules' actions produce — threading a child
//! [`Context`] so the resulting calls trace back to the triggering event. The
//! actual *execution* of those calls is the deferred service-execution layer
//! (see [`crate::service`]); the engine's job is to decide *what* to call.

use crate::context::Context;
use crate::event::Event;
use crate::service::ServiceCall;
use crate::state::EntityId;
use crate::state_machine::EVENT_STATE_CHANGED;
use crate::state_machine::StateMachine;
use crate::template::TemplateEngine;

/// `automation` run mode (`SCRIPT_MODE_*`). Retained for fidelity; the
/// synchronous core evaluates rules independently so the mode only matters
/// once an async runtime schedules overlapping runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutomationMode {
    #[default]
    Single,
    Restart,
    Queued,
    Parallel,
}

/// Port of the trigger platforms used by the chain (`helpers.trigger`).
#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    /// `platform: event` — fires when an event of `event_type` is fired.
    Event { event_type: String },
    /// `platform: state` — fires on a `state_changed` for `entity_id` whose
    /// old/new values match the optional `from`/`to` filters. With neither
    /// filter set, any real change of the entity fires it.
    State {
        entity_id: EntityId,
        from: Option<String>,
        to: Option<String>,
    },
    /// `platform: numeric_state` — fires when the entity's numeric value
    /// *crosses into* the `above`/`below` band (was outside, now inside).
    NumericState {
        entity_id: EntityId,
        above: Option<f64>,
        below: Option<f64>,
    },
}

/// Port of the condition platforms (`helpers.condition`).
#[derive(Clone, Debug)]
pub enum Condition {
    /// `condition: state` — entity currently equals `state`.
    State { entity_id: EntityId, state: String },
    /// `condition: numeric_state` — entity's value is within the band.
    NumericState {
        entity_id: EntityId,
        above: Option<f64>,
        below: Option<f64>,
    },
    /// `condition: template` — the template renders truthy.
    Template { template: String },
    /// `condition: and`.
    And(Vec<Self>),
    /// `condition: or`.
    Or(Vec<Self>),
    /// `condition: not`.
    Not(Box<Self>),
}

/// Port of the action steps the script engine runs. Only the service-call step
/// is modelled here (the engine emits calls; it does not run them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `service: domain.service` with `data`, optionally targeting an entity.
    CallService {
        domain: String,
        service: String,
        data: serde_json::Value,
        target: Option<EntityId>,
    },
}

/// Port of an `automation:` config entry.
#[derive(Clone, Debug)]
pub struct AutomationRule {
    pub id: String,
    pub alias: Option<String>,
    pub triggers: Vec<Trigger>,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    pub mode: AutomationMode,
}

impl AutomationRule {
    /// A bare rule with one trigger and one action, no conditions.
    #[must_use]
    pub fn new(id: impl Into<String>, trigger: Trigger, action: Action) -> Self {
        Self {
            id: id.into(),
            alias: None,
            triggers: vec![trigger],
            conditions: Vec::new(),
            actions: vec![action],
            mode: AutomationMode::Single,
        }
    }
}

impl Trigger {
    /// Whether `event` fires this trigger. State/numeric triggers read the
    /// old and new values straight from the `state_changed` payload, so no
    /// state-machine lookup is needed.
    #[must_use]
    pub fn matches(&self, event: &Event) -> bool {
        match self {
            Self::Event { event_type } => &event.event_type == event_type,
            Self::State { entity_id, from, to } => {
                let Some(change) = StateChangePayload::parse(event) else {
                    return false;
                };
                if change.entity_id != entity_id.to_string() {
                    return false;
                }
                // Filter checks; an absent filter matches anything.
                if let Some(want_from) = from {
                    if change.old.as_deref() != Some(want_from.as_str()) {
                        return false;
                    }
                }
                if let Some(want_to) = to {
                    if change.new.as_deref() != Some(want_to.as_str()) {
                        return false;
                    }
                }
                // With no `to`/`from`, require a genuine value change.
                if from.is_none() && to.is_none() {
                    return change.old != change.new;
                }
                true
            }
            Self::NumericState { entity_id, above, below } => {
                let Some(change) = StateChangePayload::parse(event) else {
                    return false;
                };
                if change.entity_id != entity_id.to_string() {
                    return false;
                }
                let new_val = change.new.as_deref().and_then(|s| s.parse::<f64>().ok());
                let old_val = change.old.as_deref().and_then(|s| s.parse::<f64>().ok());
                let Some(new_val) = new_val else { return false };
                // Crossing: new value inside the band, old value outside (or
                // unparseable / absent — i.e. it was not already inside).
                let now_inside = within_band(new_val, *above, *below);
                let was_inside = old_val.is_some_and(|v| within_band(v, *above, *below));
                now_inside && !was_inside
            }
        }
    }
}

impl Condition {
    /// Evaluate against current `states`, rendering template conditions with
    /// `template`.
    #[must_use]
    pub fn evaluate(&self, states: &StateMachine, template: &TemplateEngine) -> bool {
        match self {
            Self::State { entity_id, state } => {
                states.get(entity_id).is_some_and(|s| &s.state == state)
            }
            Self::NumericState { entity_id, above, below } => {
                numeric_value(states, entity_id).is_some_and(|v| within_band(v, *above, *below))
            }
            Self::Template { template: tmpl } => template
                .render(tmpl)
                .is_ok_and(|rendered| render_truthy(&rendered)),
            Self::And(parts) => parts.iter().all(|c| c.evaluate(states, template)),
            Self::Or(parts) => parts.iter().any(|c| c.evaluate(states, template)),
            Self::Not(inner) => !inner.evaluate(states, template),
        }
    }
}

/// Port of the `AutomationComponent` runtime, reduced to rule evaluation.
pub struct AutomationEngine {
    states: StateMachine,
    template: TemplateEngine,
    rules: Vec<AutomationRule>,
}

impl AutomationEngine {
    /// Build an engine reading `states`. A [`TemplateEngine`] over the same
    /// state machine is created for template conditions.
    #[must_use]
    pub fn new(states: StateMachine) -> Self {
        let template = TemplateEngine::new(states.clone());
        Self {
            states,
            template,
            rules: Vec::new(),
        }
    }

    /// Register a rule.
    pub fn add_rule(&mut self, rule: AutomationRule) {
        self.rules.push(rule);
    }

    /// Number of registered rules.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Evaluate every rule against `event`. For each rule with at least one
    /// matching trigger and all conditions satisfied, its actions are turned
    /// into [`ServiceCall`]s (each carrying a [`Context`] that is a child of
    /// the event's context). Returns the calls in rule, then action, order.
    #[must_use]
    pub fn handle_event(&self, event: &Event) -> Vec<ServiceCall> {
        let mut calls = Vec::new();
        for rule in &self.rules {
            let triggered = rule.triggers.iter().any(|t| t.matches(event));
            if !triggered {
                continue;
            }
            let pass = rule
                .conditions
                .iter()
                .all(|c| c.evaluate(&self.states, &self.template));
            if !pass {
                continue;
            }
            for action in &rule.actions {
                if let Some(call) = action.to_service_call(&event.context) {
                    calls.push(call);
                }
            }
        }
        calls
    }
}

impl Action {
    /// Turn an action into the [`ServiceCall`] it represents, threading a
    /// [`Context`] that descends from `parent` (the triggering event's
    /// context). A target entity is folded into the call data as `entity_id`,
    /// matching HA's target→data expansion. Returns `None` only if the
    /// domain/service names are malformed.
    fn to_service_call(&self, parent: &Context) -> Option<ServiceCall> {
        match self {
            Self::CallService { domain, service, data, target } => {
                let mut data = data.clone();
                if let Some(entity_id) = target {
                    if let Some(map) = data.as_object_mut() {
                        map.insert(
                            "entity_id".to_owned(),
                            serde_json::Value::String(entity_id.to_string()),
                        );
                    }
                }
                ServiceCall::new(domain, service, data, Context::child_of(parent)).ok()
            }
        }
    }
}

/// The fields a `state_changed` event carries that the trigger logic needs:
/// the entity id and the old/new state *values* (attributes are ignored here).
struct StateChangePayload {
    entity_id: String,
    old: Option<String>,
    new: Option<String>,
}

impl StateChangePayload {
    /// Parse a `state_changed` event's data, or `None` for any other event.
    fn parse(event: &Event) -> Option<Self> {
        if event.event_type != EVENT_STATE_CHANGED {
            return None;
        }
        let entity_id = event.data.get("entity_id")?.as_str()?.to_owned();
        let value = |key: &str| {
            event
                .data
                .get(key)
                .and_then(|s| s.get("state"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        Some(Self {
            entity_id,
            old: value("old_state"),
            new: value("new_state"),
        })
    }
}

/// Read `entity_id`'s numeric value (HA parses the state string as a float).
fn numeric_value(states: &StateMachine, entity_id: &EntityId) -> Option<f64> {
    states.get(entity_id)?.state.parse::<f64>().ok()
}

/// Whether `value` falls inside the optional `above`/`below` band. HA's
/// numeric comparison is strict: `value > above` and `value < below`.
fn within_band(value: f64, above: Option<f64>, below: Option<f64>) -> bool {
    above.is_none_or(|a| value > a) && below.is_none_or(|b| value < b)
}

/// HA's `template.result_as_boolean` for template conditions.
fn render_truthy(rendered: &str) -> bool {
    matches!(
        rendered.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "enable" | "1" | "1.0"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventOrigin;
    use crate::event_bus::EventBus;
    use crate::state::StateAttributes;
    use serde_json::json;

    fn light(object: &str) -> EntityId {
        EntityId::new("light", object).expect("id")
    }

    /// Build a state_changed event the way the StateMachine fires one.
    fn state_changed(entity: &str, old: Option<&str>, new: &str) -> Event {
        let old_state = old.map(|o| json!({"entity_id": entity, "state": o, "attributes": {}}));
        Event::new(
            EVENT_STATE_CHANGED,
            json!({
                "entity_id": entity,
                "old_state": old_state,
                "new_state": {"entity_id": entity, "state": new, "attributes": {}},
            }),
            EventOrigin::Local,
            Context::new(),
        )
    }

    fn notify_action() -> Action {
        Action::CallService {
            domain: "notify".into(),
            service: "notify".into(),
            data: json!({"message": "fired"}),
            target: None,
        }
    }

    #[test]
    fn event_trigger_matches_by_type() {
        let t = Trigger::Event { event_type: "my_event".into() };
        assert!(t.matches(&Event::local("my_event", json!({}))));
        assert!(!t.matches(&Event::local("other", json!({}))));
    }

    #[test]
    fn state_trigger_honours_from_to_filters() {
        let to_on = Trigger::State { entity_id: light("kitchen"), from: None, to: Some("on".into()) };
        assert!(to_on.matches(&state_changed("light.kitchen", Some("off"), "on")));
        // wrong target value
        assert!(!to_on.matches(&state_changed("light.kitchen", Some("off"), "off")));
        // wrong entity
        assert!(!to_on.matches(&state_changed("light.hall", Some("off"), "on")));

        let off_to_on =
            Trigger::State { entity_id: light("kitchen"), from: Some("off".into()), to: Some("on".into()) };
        assert!(off_to_on.matches(&state_changed("light.kitchen", Some("off"), "on")));
        // from filter fails (was unavailable, not off)
        assert!(!off_to_on.matches(&state_changed("light.kitchen", Some("unavailable"), "on")));
    }

    #[test]
    fn state_trigger_no_filters_fires_on_real_change_only() {
        let any = Trigger::State { entity_id: light("kitchen"), from: None, to: None };
        assert!(any.matches(&state_changed("light.kitchen", Some("off"), "on")));
        // same old==new value is not a change
        assert!(!any.matches(&state_changed("light.kitchen", Some("on"), "on")));
        // a non-state_changed event never matches a state trigger
        assert!(!any.matches(&Event::local("other", json!({}))));
    }

    #[test]
    fn numeric_state_trigger_fires_on_crossing() {
        // The trigger reads both old and new values from the event payload.
        let temp = EntityId::new("sensor", "temp").expect("id");
        let t = Trigger::NumericState {
            entity_id: temp,
            above: Some(20.0),
            below: None,
        };
        // crossing 18 -> 21 fires (old outside, new inside)
        assert!(t.matches(&state_changed("sensor.temp", Some("18"), "21")));
        // 21 -> 22 stays inside the band: not a crossing
        assert!(!t.matches(&state_changed("sensor.temp", Some("21"), "22")));
        // 18 -> 19 never enters the band
        assert!(!t.matches(&state_changed("sensor.temp", Some("18"), "19")));
    }

    #[test]
    fn conditions_state_numeric_template_and_bool_ops() {
        let states = StateMachine::new(EventBus::new());
        let tmpl = TemplateEngine::new(states.clone());
        states.set(light("kitchen"), "on", StateAttributes::new(), Context::new());
        let temp = EntityId::new("sensor", "temp").expect("id");
        states.set(temp.clone(), "23.5", StateAttributes::new(), Context::new());

        let c_state = Condition::State { entity_id: light("kitchen"), state: "on".into() };
        assert!(c_state.evaluate(&states, &tmpl));

        let c_num = Condition::NumericState { entity_id: temp.clone(), above: Some(20.0), below: Some(25.0) };
        assert!(c_num.evaluate(&states, &tmpl));
        let c_num_out = Condition::NumericState { entity_id: temp, above: Some(24.0), below: None };
        assert!(!c_num_out.evaluate(&states, &tmpl));

        let c_tmpl = Condition::Template {
            template: "{{ is_state('light.kitchen', 'on') }}".into(),
        };
        assert!(c_tmpl.evaluate(&states, &tmpl));

        // and / or / not composition
        let and = Condition::And(vec![c_state.clone(), c_num.clone()]);
        assert!(and.evaluate(&states, &tmpl));
        let not = Condition::Not(Box::new(Condition::State {
            entity_id: light("kitchen"),
            state: "off".into(),
        }));
        assert!(not.evaluate(&states, &tmpl));
        let or = Condition::Or(vec![
            Condition::State { entity_id: light("kitchen"), state: "off".into() },
            c_state,
        ]);
        assert!(or.evaluate(&states, &tmpl));
    }

    #[test]
    fn basic_if_then_produces_service_call() {
        let states = StateMachine::new(EventBus::new());
        let mut engine = AutomationEngine::new(states.clone());
        // "turn on the fan when the kitchen light comes on"
        engine.add_rule(AutomationRule::new(
            "fan_on_light",
            Trigger::State { entity_id: light("kitchen"), from: None, to: Some("on".into()) },
            Action::CallService {
                domain: "fan".into(),
                service: "turn_on".into(),
                data: json!({}),
                target: Some(EntityId::new("fan", "kitchen").expect("id")),
            },
        ));

        // a non-matching event yields nothing
        let none = engine.handle_event(&state_changed("light.kitchen", Some("on"), "off"));
        assert!(none.is_empty());

        // the matching transition yields exactly the configured call
        let calls = engine.handle_event(&state_changed("light.kitchen", Some("off"), "on"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].domain, "fan");
        assert_eq!(calls[0].service, "turn_on");
        // target entity is folded into the call data as entity_id
        assert_eq!(calls[0].data["entity_id"], "fan.kitchen");
    }

    #[test]
    fn conditions_gate_the_action() {
        let states = StateMachine::new(EventBus::new());
        let mut engine = AutomationEngine::new(states.clone());
        // night-mode flag is off
        states.set(
            EntityId::new("input_boolean", "night").expect("id"),
            "off",
            StateAttributes::new(),
            Context::new(),
        );
        let mut rule = AutomationRule::new(
            "notify_at_night",
            Trigger::State { entity_id: light("kitchen"), from: None, to: Some("on".into()) },
            notify_action(),
        );
        rule.conditions = vec![Condition::State {
            entity_id: EntityId::new("input_boolean", "night").expect("id"),
            state: "on".into(),
        }];
        engine.add_rule(rule);

        // trigger matches but condition (night == on) fails → no call
        let calls = engine.handle_event(&state_changed("light.kitchen", Some("off"), "on"));
        assert!(calls.is_empty());

        // flip night on; same event now passes the condition
        states.set(
            EntityId::new("input_boolean", "night").expect("id"),
            "on",
            StateAttributes::new(),
            Context::new(),
        );
        let calls = engine.handle_event(&state_changed("light.kitchen", Some("off"), "on"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].domain, "notify");
        // the produced call's context descends from the triggering event
        assert!(calls[0].context.parent_id.is_some());
    }

    #[test]
    fn render_truthy_tokens() {
        assert!(render_truthy(" True "));
        assert!(render_truthy("on"));
        assert!(render_truthy("1"));
        assert!(!render_truthy("false"));
        assert!(!render_truthy(""));
        assert!(!render_truthy("off"));
    }

    #[test]
    fn within_band_is_strict() {
        assert!(within_band(21.0, Some(20.0), Some(25.0)));
        assert!(!within_band(20.0, Some(20.0), None)); // strict >
        assert!(!within_band(25.0, None, Some(25.0))); // strict <
        assert!(within_band(5.0, None, None)); // unbounded
    }
}
