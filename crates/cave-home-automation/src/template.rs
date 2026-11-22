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

    /// Thermostat fixture for the numeric-filter tests: float, integer and
    /// non-numeric attributes, the way a JSON integration reports them.
    fn climate_machine() -> HassResult<Arc<StateMachine>> {
        machine_with(
            "climate.living_room",
            "heat",
            vec![
                ("temperature", serde_json::json!(21.5)),
                ("target_temp_low", serde_json::json!(-3.25)),
                ("fan_speed", serde_json::json!(3)),
                ("preset_mode", serde_json::json!("eco")),
                ("last_seen", serde_json::json!(1_700_000_000.5)),
            ],
        )
    }

    /// Variables available to every numeric-filter test template.
    fn numeric_variables() -> Value {
        serde_json::json!({
            "negative": -12.75,
            "stamp": 1_710_510_600.5,
            "beyond_i64": 1e19,
            "huge_integer": u64::MAX,
            "near_max": 1e308,
        })
    }

    /// Render every `(template, expected)` pair against the thermostat
    /// fixture and report all mismatches together.
    fn assert_renders(cases: &[(&str, &str)]) -> TestResult {
        let sm = climate_machine()?;
        let variables = numeric_variables();
        let mut mismatches = Vec::new();
        for &(src, expected) in cases {
            match Template::new(src, sm.clone()).render(&variables) {
                Ok(rendered) if rendered == expected => {}
                Ok(rendered) => mismatches.push(format!(
                    "`{src}` rendered {rendered:?}, expected {expected:?}"
                )),
                Err(err) => mismatches.push(format!("`{src}` failed: {err}")),
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        Ok(())
    }

    /// What `float` already returned for integers, booleans and numeric
    /// strings; accepting floats must not move any of these.
    #[test]
    fn template_float_filter_keeps_integer_and_string_results() -> TestResult {
        assert_renders(&[
            ("{{ 3 | float }}", "3.0"),
            ("{{ 0 | float }}", "0.0"),
            ("{{ (-7) | float }}", "-7.0"),
            ("{{ 3.0 | float }}", "3.0"),
            ("{{ true | float }}", "1.0"),
            ("{{ '3.7' | float }}", "3.7"),
            ("{{ '42' | float }}", "42.0"),
            ("{{ '-0.5' | float }}", "-0.5"),
            ("{{ '1e3' | float }}", "1000.0"),
            (
                "{{ state_attr('climate.living_room', 'fan_speed') | float }}",
                "3.0",
            ),
        ])
    }

    /// A value that already is a float converts to itself.
    #[test]
    fn template_float_filter_accepts_float_values() -> TestResult {
        assert_renders(&[
            ("{{ 3.7 | float }}", "3.7"),
            ("{{ 0.1 | float }}", "0.1"),
            ("{{ (-2.5) | float }}", "-2.5"),
            ("{{ negative | float }}", "-12.75"),
            ("{{ 3.7 | float(9.5) }}", "3.7"),
            ("{{ (3.7 | float) > 3 }}", "True"),
        ])
    }

    /// A JSON float attribute read through `state_attr` is the case that
    /// silently became `0.0`.
    #[test]
    fn template_float_filter_reads_float_state_attributes() -> TestResult {
        assert_renders(&[
            (
                "{{ state_attr('climate.living_room', 'temperature') | float }}",
                "21.5",
            ),
            (
                "{{ state_attr('climate.living_room', 'target_temp_low') | float }}",
                "-3.25",
            ),
            (
                "{{ state_attr('climate.living_room', 'temperature') | float(0.0) }}",
                "21.5",
            ),
            (
                "{{ state_attr('climate.living_room', 'temperature') | float > 21 }}",
                "True",
            ),
        ])
    }

    /// Numbers that do not fit an `i64` (a float beyond its range, an
    /// integer above `i64::MAX`) are numbers all the same.
    #[test]
    fn template_float_filter_converts_numbers_outside_the_i64_range() -> TestResult {
        assert_renders(&[
            ("{{ beyond_i64 | float }}", "10000000000000000000.0"),
            ("{{ huge_integer | float }}", "18446744073709552000.0"),
        ])
    }

    /// Input that is not a number yields the default argument, or `0.0`
    /// when none is given. (Upstream documents an error for the
    /// no-default case; this crate has always returned `0.0`.)
    #[test]
    fn template_float_filter_falls_back_for_non_numeric_input() -> TestResult {
        assert_renders(&[
            ("{{ 'abc' | float }}", "0.0"),
            ("{{ 'abc' | float(9.5) }}", "9.5"),
            ("{{ 'abc' | float(2) }}", "2.0"),
            ("{{ '' | float(1.5) }}", "1.5"),
            ("{{ none | float }}", "0.0"),
            ("{{ none | float(2.5) }}", "2.5"),
            ("{{ [1.5] | float(4.0) }}", "4.0"),
            ("{{ states('sensor.ghost') | float(7.0) }}", "7.0"),
            (
                "{{ state_attr('climate.living_room', 'preset_mode') | float(18.5) }}",
                "18.5",
            ),
            (
                "{{ state_attr('climate.living_room', 'missing') | float(18.5) }}",
                "18.5",
            ),
        ])
    }

    /// `nan` and `inf` are valid float spellings (in Rust as in Python's
    /// `float()`), so the string forms have always converted; a non-finite
    /// float value is passed through the same way instead of being
    /// replaced by the default.
    #[test]
    fn template_numeric_filters_pass_non_finite_values_through() -> TestResult {
        assert_renders(&[
            ("{{ 'nan' | float }}", "NaN"),
            ("{{ 'NaN' | float(1.0) }}", "NaN"),
            ("{{ 'inf' | float }}", "inf"),
            ("{{ '-inf' | float(1.0) }}", "-inf"),
            ("{{ 'Infinity' | float }}", "inf"),
            ("{{ 'nan' | as_timestamp }}", "NaN"),
            ("{{ 'inf' | as_timestamp }}", "inf"),
            ("{{ (near_max * 10) | float(1.0) }}", "inf"),
            ("{{ (near_max * 10) | as_timestamp }}", "inf"),
        ])
    }

    /// Every input form `as_timestamp` already converted keeps its result,
    /// and input that is neither a date string nor a number stays undefined.
    #[test]
    fn template_as_timestamp_keeps_existing_conversions() -> TestResult {
        assert_renders(&[
            ("{{ 1700000000 | as_timestamp }}", "1700000000.0"),
            ("{{ 0 | as_timestamp }}", "0.0"),
            ("{{ (-60) | as_timestamp }}", "-60.0"),
            ("{{ '1970-01-01T00:01:00Z' | as_timestamp }}", "60.0"),
            (
                "{{ '2024-03-15T14:50:00+01:00' | as_timestamp }}",
                "1710510600.0",
            ),
            ("{{ '1700000000' | as_timestamp }}", "1700000000.0"),
            ("{{ '1.5' | as_timestamp }}", "1.5"),
            ("{{ 'not a date' | as_timestamp }}", ""),
            ("{{ ('not a date' | as_timestamp) is undefined }}", "True"),
            ("{{ none | as_timestamp }}", ""),
            ("{{ [1.5] | as_timestamp }}", ""),
        ])
    }

    /// A float is taken as seconds since the epoch, exactly like an
    /// integer, keeping its fractional part.
    #[test]
    fn template_as_timestamp_accepts_float_values() -> TestResult {
        assert_renders(&[
            ("{{ 1.5 | as_timestamp }}", "1.5"),
            ("{{ 1700000000.25 | as_timestamp }}", "1700000000.25"),
            ("{{ (-0.5) | as_timestamp }}", "-0.5"),
            ("{{ stamp | as_timestamp }}", "1710510600.5"),
            (
                "{{ state_attr('climate.living_room', 'last_seen') | as_timestamp }}",
                "1700000000.5",
            ),
            ("{{ (1.5 | as_timestamp) is defined }}", "True"),
        ])
    }

    /// `int`, `round`, `abs` and the comparison operators are the engine's
    /// own and already take floats; pinned so that stays visible here.
    #[test]
    fn template_builtin_numeric_filters_accept_floats() -> TestResult {
        assert_renders(&[
            ("{{ 3.7 | int }}", "3"),
            ("{{ (-3.7) | int }}", "-3"),
            ("{{ '3.7' | int }}", "3"),
            ("{{ 3.7 | round }}", "4.0"),
            ("{{ 3.14159 | round(2) }}", "3.14"),
            ("{{ (-3.7) | abs }}", "3.7"),
            ("{{ 3.7 > 3 }}", "True"),
            (
                "{{ state_attr('climate.living_room', 'temperature') > 21 }}",
                "True",
            ),
            (
                "{{ state_attr('climate.living_room', 'temperature') | round }}",
                "22.0",
            ),
            (
                "{{ state_attr('climate.living_room', 'temperature') | int }}",
                "21",
            ),
        ])
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
