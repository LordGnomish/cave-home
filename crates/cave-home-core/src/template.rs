//! Port of the state-access surface of `homeassistant.helpers.template`.
//!
//! HA renders templates with Jinja2 and injects a handful of globals that read
//! the live [`StateMachine`](crate::state_machine::StateMachine): `states(id)`,
//! `is_state(id, value)`, `state_attr(id, attr)` and `is_state_attr(id, attr,
//! value)`. We build on `minijinja` — a Jinja2 implementation — and register
//! the same globals, each closing over a clone of the state machine handle so a
//! template always sees current state.

use crate::state::EntityId;
use crate::state_machine::StateMachine;
use minijinja::{Environment, Value};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("template render error: {0}")]
    Render(String),
}

/// Port of `homeassistant.helpers.template.Template` bound to a state machine.
pub struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Build an engine whose `states`/`is_state`/`state_attr`/`is_state_attr`
    /// globals read `states`. The handle is cloned (it is `Arc`-backed), so the
    /// engine reflects live state for the lifetime of the template.
    #[must_use]
    pub fn new(states: StateMachine) -> Self {
        let mut env = Environment::new();

        // states('light.kitchen') -> the state string, or "unknown" if the
        // entity is absent or its id is malformed (HA's STATE_UNKNOWN).
        let sm = states.clone();
        env.add_function("states", move |entity_id: &str| -> String {
            lookup_state(&sm, entity_id)
                .map_or_else(|| crate::entity::STATE_UNKNOWN.to_owned(), |s| s.state)
        });

        // is_state('light.kitchen', 'on') -> bool.
        let sm = states.clone();
        env.add_function("is_state", move |entity_id: &str, value: &str| -> bool {
            lookup_state(&sm, entity_id).is_some_and(|s| s.state == value)
        });

        // state_attr('light.kitchen', 'brightness') -> the attribute value, or
        // none when the entity or attribute is missing.
        let sm = states.clone();
        env.add_function("state_attr", move |entity_id: &str, attr: &str| -> Value {
            lookup_state(&sm, entity_id)
                .and_then(|s| s.attributes.get(attr).cloned())
                .map_or_else(|| Value::from(()), |v| Value::from_serialize(&v))
        });

        // is_state_attr('climate.x', 'hvac_action', 'heating') -> bool.
        // The last closure consumes `states` directly (no further clone).
        let sm = states;
        env.add_function(
            "is_state_attr",
            move |entity_id: &str, attr: &str, value: Value| -> bool {
                lookup_state(&sm, entity_id)
                    .and_then(|s| s.attributes.get(attr).cloned())
                    .is_some_and(|v| Value::from_serialize(&v) == value)
            },
        );

        Self { env }
    }

    /// Render `template` to a string. With no template tags the input is
    /// returned verbatim (HA short-circuits non-templates, but rendering a
    /// literal yields the same result).
    ///
    /// # Errors
    /// [`TemplateError::Render`] if the template is malformed or a global
    /// raises during rendering.
    pub fn render(&self, template: &str) -> Result<String, TemplateError> {
        self.env
            .render_str(template, minijinja::context! {})
            .map_err(|e| TemplateError::Render(e.to_string()))
    }
}

/// Look up a state by its string id, tolerating a malformed id (returns
/// `None`, never errors — matching HA's lenient template lookups).
fn lookup_state(states: &StateMachine, entity_id: &str) -> Option<crate::state::State> {
    let id = EntityId::from_str(entity_id).ok()?;
    states.get(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::state::StateAttributes;
    use serde_json::json;

    fn engine_with_kitchen() -> TemplateEngine {
        let sm = StateMachine::new(crate::event_bus::EventBus::new());
        let id = EntityId::new("light", "kitchen").expect("id");
        let mut attrs = StateAttributes::new();
        attrs.insert("brightness".into(), json!(200));
        attrs.insert("friendly_name".into(), json!("Kitchen"));
        sm.set(id, "on", attrs, Context::new());
        TemplateEngine::new(sm)
    }

    #[test]
    fn literal_passthrough() {
        let eng = engine_with_kitchen();
        assert_eq!(eng.render("hello world").expect("render"), "hello world");
    }

    #[test]
    fn states_function_reads_value() {
        let eng = engine_with_kitchen();
        assert_eq!(
            eng.render("{{ states('light.kitchen') }}").expect("render"),
            "on"
        );
        // missing entity -> unknown
        assert_eq!(
            eng.render("{{ states('light.nope') }}").expect("render"),
            "unknown"
        );
        // malformed id -> unknown (no error)
        assert_eq!(eng.render("{{ states('garbage') }}").expect("render"), "unknown");
    }

    #[test]
    fn is_state_branches_in_if() {
        let eng = engine_with_kitchen();
        let out = eng
            .render("{% if is_state('light.kitchen', 'on') %}lit{% else %}dark{% endif %}")
            .expect("render");
        assert_eq!(out, "lit");
        let out2 = eng
            .render("{% if is_state('light.kitchen', 'off') %}lit{% else %}dark{% endif %}")
            .expect("render");
        assert_eq!(out2, "dark");
    }

    #[test]
    fn state_attr_reads_attribute_and_supports_arithmetic() {
        let eng = engine_with_kitchen();
        assert_eq!(
            eng.render("{{ state_attr('light.kitchen', 'brightness') }}").expect("r"),
            "200"
        );
        // the value is a real number, usable in arithmetic
        assert_eq!(
            eng.render("{{ state_attr('light.kitchen', 'brightness') // 2 }}").expect("r"),
            "100"
        );
        // missing attribute renders as the none token
        assert_eq!(
            eng.render("{{ state_attr('light.kitchen', 'nope') }}").expect("r"),
            "none"
        );
    }

    #[test]
    fn is_state_attr_compares_value() {
        let eng = engine_with_kitchen();
        assert_eq!(
            eng.render("{{ is_state_attr('light.kitchen', 'brightness', 200) }}").expect("r"),
            "true"
        );
        assert_eq!(
            eng.render("{{ is_state_attr('light.kitchen', 'brightness', 5) }}").expect("r"),
            "false"
        );
    }

    #[test]
    fn malformed_template_errors() {
        let eng = engine_with_kitchen();
        assert!(eng.render("{{ unclosed ").is_err());
    }
}
