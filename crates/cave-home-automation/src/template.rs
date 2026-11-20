// SPDX-License-Identifier: Apache-2.0
//! Template environment — port of `homeassistant/helpers/template/__init__.py`.
//!
//! Uses [`minijinja`] (Rust crate) as the Jinja2-class engine and
//! registers the HA-flavoured globals/filters listed in
//! [`Template::register_extensions`].
//!
//! # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/template/__init__.py`

use std::sync::Arc;

use minijinja::value::Value as JinjaValue;
use minijinja::{Environment, Error as JinjaError};
use parking_lot::RwLock;
use serde_json::Value;
use time::OffsetDateTime;

use crate::error::{HassError, HassResult};
use crate::num::i64_to_f64;
use crate::state::StateMachine;

/// Snapshot of a single state for the template environment.
fn state_to_jinja(state: &crate::state::State) -> JinjaValue {
    JinjaValue::from_serialize(serde_json::json!({
        "entity_id": state.entity_id,
        "domain": state.domain,
        "object_id": state.object_id,
        "state": state.state,
        "attributes": state.attributes,
        "last_changed": state.last_changed.unix_timestamp(),
        "last_updated": state.last_updated.unix_timestamp(),
    }))
}

/// Lookup helper around a (typically shared) [`StateMachine`].
fn lookup(sm: &Arc<StateMachine>, entity_id: &str) -> Option<crate::state::State> {
    sm.get(entity_id)
}

/// Wrapper around a `minijinja` template string, paired with a state
/// machine for global lookups.
///
/// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/template/__init__.py::Template`
pub struct Template {
    src: String,
    env: Arc<RwLock<Environment<'static>>>,
    /// Held for callers that need to inspect the bound state machine
    /// (e.g. the engine when re-binding the template at run time).
    state_machine: Arc<StateMachine>,
}

impl Template {
    /// Reference to the bound state machine.
    #[must_use]
    pub const fn state_machine(&self) -> &Arc<StateMachine> {
        &self.state_machine
    }
}

impl std::fmt::Debug for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Template")
            .field("src", &self.src)
            .finish_non_exhaustive()
    }
}

impl Template {
    /// Build a new template from `src` against the given state machine.
    ///
    /// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/template/__init__.py::Template.__init__`
    pub fn new(src: impl Into<String>, state_machine: Arc<StateMachine>) -> Self {
        let mut env = Environment::new();
        Self::register_extensions(&mut env, state_machine.clone());
        Self {
            src: src.into(),
            env: Arc::new(RwLock::new(env)),
            state_machine,
        }
    }

    /// Register HA-flavoured globals/filters on a fresh `minijinja::Environment`.
    ///
    /// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/template/__init__.py`
    ///   (`TemplateEnvironment.__init__` — `globals` / `filters` registration)
    fn register_extensions(env: &mut Environment<'static>, sm: Arc<StateMachine>) {
        // states(entity_id) — fetch the bare state value, or all states if no arg.
        let sm_states = sm.clone();
        env.add_function(
            "states",
            move |args: &[JinjaValue]| -> Result<JinjaValue, JinjaError> {
                if args.is_empty() {
                    let snapshot: Vec<JinjaValue> =
                        sm_states.all(None).iter().map(state_to_jinja).collect();
                    return Ok(JinjaValue::from(snapshot));
                }
                let key = args[0].as_str().ok_or_else(|| {
                    JinjaError::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "states() expects an entity_id string",
                    )
                })?;
                Ok(match lookup(&sm_states, key) {
                    Some(s) => JinjaValue::from(s.state),
                    None => JinjaValue::from("unknown"),
                })
            },
        );

        // is_state(entity_id, state) -> bool
        let sm_is_state = sm.clone();
        env.add_function(
            "is_state",
            move |entity_id: &str, state: &str| -> Result<bool, JinjaError> {
                Ok(sm_is_state.is_state(entity_id, state))
            },
        );

        // state_attr(entity_id, attribute) -> attribute value or None
        let sm_attr = sm;
        env.add_function(
            "state_attr",
            move |entity_id: &str, attr: &str| -> Result<JinjaValue, JinjaError> {
                Ok(lookup(&sm_attr, entity_id)
                    .and_then(|s| s.attributes.get(attr).map(JinjaValue::from_serialize))
                    .unwrap_or_else(|| JinjaValue::from(())))
            },
        );

        // now() -> ISO-8601 UTC timestamp string
        env.add_function("now", || -> Result<JinjaValue, JinjaError> {
            Ok(JinjaValue::from(iso_now()))
        });

        // today_at(time_str) -> ISO-8601 datetime at today's date + given HH:MM:SS.
        env.add_function(
            "today_at",
            |time_str: &str| -> Result<JinjaValue, JinjaError> {
                let parts: Vec<&str> = time_str.split(':').collect();
                let hh = parts
                    .first()
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(0);
                let mm = parts.get(1).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
                let ss = parts.get(2).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
                let now = OffsetDateTime::now_utc();
                let target = now
                    .replace_hour(hh)
                    .and_then(|d| d.replace_minute(mm))
                    .and_then(|d| d.replace_second(ss))
                    .map_err(|e| {
                        JinjaError::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                    })?;
                Ok(JinjaValue::from(iso(target)))
            },
        );

        // as_timestamp(value) -> f64 unix timestamp.
        env.add_filter(
            "as_timestamp",
            |value: JinjaValue| -> Result<JinjaValue, JinjaError> {
                if let Some(s) = value.as_str() {
                    if let Ok(parsed) = OffsetDateTime::parse(
                        s,
                        &time::format_description::well_known::Iso8601::DEFAULT,
                    ) {
                        return Ok(JinjaValue::from(i64_to_f64(parsed.unix_timestamp())));
                    }
                    if let Ok(n) = s.parse::<f64>() {
                        return Ok(JinjaValue::from(n));
                    }
                }
                if let Ok(n) = i64::try_from(value) {
                    return Ok(JinjaValue::from(i64_to_f64(n)));
                }
                Ok(JinjaValue::UNDEFINED)
            },
        );

        // float | int | bool filters with HA semantics (default value on failure).
        env.add_filter(
            "float",
            |v: JinjaValue, default: Option<f64>| -> JinjaValue {
                if let Some(s) = v.as_str() {
                    if let Ok(parsed) = s.parse::<f64>() {
                        return JinjaValue::from(parsed);
                    }
                }
                if let Ok(n) = i64::try_from(v) {
                    return JinjaValue::from(i64_to_f64(n));
                }
                JinjaValue::from(default.unwrap_or(0.0))
            },
        );
    }

    /// Render the template and return the result string.
    ///
    /// `variables` is merged into the global scope under each key.
    ///
    /// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/template/__init__.py::Template.async_render`
    pub fn render(&self, variables: &Value) -> HassResult<String> {
        self.env
            .read()
            .template_from_str(&self.src)
            .and_then(|tmpl| tmpl.render(JinjaValue::from_serialize(variables)))
            .map_err(|e| HassError::TemplateError(e.to_string()))
    }

    /// Render returning the result coerced to a boolean — used by the
    /// `template` condition.
    ///
    /// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/condition.py::async_template`
    pub fn render_bool(&self, variables: &Value) -> HassResult<bool> {
        let rendered = self.render(variables)?;
        let lower = rendered.trim().to_ascii_lowercase();
        Ok(matches!(lower.as_str(), "true" | "yes" | "on" | "1"))
    }
}

fn iso(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap_or_else(|_| t.unix_timestamp().to_string())
}

fn iso_now() -> String {
    iso(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;
    use std::collections::BTreeMap;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn machine_with(
        entity_id: &str,
        state: &str,
        attrs: Vec<(&str, Value)>,
    ) -> HassResult<Arc<StateMachine>> {
        let bus = Arc::new(InMemoryEventBus::new());
        let sm = Arc::new(StateMachine::new(bus));
        let mut a: BTreeMap<String, Value> = BTreeMap::new();
        for (k, v) in attrs {
            a.insert(k.into(), v);
        }
        sm.async_set(entity_id, state, a, false, None)?;
        Ok(sm)
    }

    /// Upstream-test: `tests/helpers/test_template.py::test_state_function`
    #[test]
    fn template_states_function() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new("{{ states('light.kitchen') }}", sm);
        assert_eq!(tmpl.render(&Value::Null)?, "on");
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_template.py::test_is_state`
    #[test]
    fn template_is_state() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new(
            "{% if is_state('light.kitchen', 'on') %}YES{% else %}NO{% endif %}",
            sm,
        );
        assert_eq!(tmpl.render(&Value::Null)?, "YES");
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_template.py::test_state_attr`
    #[test]
    fn template_state_attr() -> TestResult {
        let sm = machine_with(
            "light.kitchen",
            "on",
            vec![("brightness", Value::from(200))],
        )?;
        let tmpl = Template::new("{{ state_attr('light.kitchen', 'brightness') }}", sm);
        assert_eq!(tmpl.render(&Value::Null)?, "200");
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_template.py::test_now`
    #[test]
    fn template_now_returns_utc() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new("{{ now() }}", sm);
        let rendered = tmpl.render(&Value::Null)?;
        assert!(rendered.contains('T'));
        Ok(())
    }

    #[test]
    fn template_render_bool() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new("{{ is_state('light.kitchen', 'on') }}", sm);
        assert!(tmpl.render_bool(&Value::Null)?);
        Ok(())
    }

    #[test]
    fn template_with_variables() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new("Hello {{ name }}", sm);
        let rendered = tmpl.render(&serde_json::json!({"name": "Grandma"}))?;
        assert_eq!(rendered, "Hello Grandma");
        Ok(())
    }

    #[test]
    fn template_state_attr_missing_renders_none() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let missing_attr = Template::new(
            "{{ state_attr('light.kitchen', 'brightness') }}",
            sm.clone(),
        );
        assert_eq!(missing_attr.render(&Value::Null)?, "None");
        let missing_entity = Template::new("{{ state_attr('light.ghost', 'brightness') }}", sm);
        assert_eq!(missing_entity.render(&Value::Null)?, "None");
        Ok(())
    }

    #[test]
    fn template_numeric_filters_convert_integers_and_strings() -> TestResult {
        let sm = machine_with("sensor.temp", "21.5", vec![])?;
        let render = |src: &str| Template::new(src, sm.clone()).render(&Value::Null);
        assert_eq!(render("{{ 3 | float }}")?, "3.0");
        assert_eq!(render("{{ states('sensor.temp') | float }}")?, "21.5");
        assert_eq!(render("{{ 'x' | float(9.5) }}")?, "9.5");
        assert_eq!(render("{{ 1700000000 | as_timestamp }}")?, "1700000000.0");
        assert_eq!(
            render("{{ '1970-01-01T00:01:00Z' | as_timestamp }}")?,
            "60.0"
        );
        Ok(())
    }

    #[test]
    fn template_syntax_error_is_a_template_error() -> TestResult {
        let sm = machine_with("light.kitchen", "on", vec![])?;
        let tmpl = Template::new("{{ unclosed", sm);
        assert!(matches!(
            tmpl.render(&Value::Null),
            Err(HassError::TemplateError(_))
        ));
        Ok(())
    }
}
