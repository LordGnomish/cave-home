// SPDX-License-Identifier: Apache-2.0
//! Trigger definitions — port of
//! `homeassistant/components/homeassistant/triggers/*.py`.
//!
//! Phase 1 covers: state, event, `numeric_state`, template, time,
//! `time_pattern`, homeassistant (start/stop).
//!
//! # Upstream: `home-assistant/core@456202325ac4:homeassistant/components/homeassistant/triggers/`

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{HassError, HassResult};
use crate::event_bus::Event;
use crate::num::{is_above, is_below};
use crate::state::State;

/// Variants of a trigger declaration — the YAML the user writes maps
/// 1:1 to these variants.
///
/// Phase 1 covers the seven mandated platforms from `parity.manifest.toml`.
///
/// # Upstream: `home-assistant/core@456202325ac4:homeassistant/components/homeassistant/triggers/`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum Trigger {
    /// Fire when the state of an entity transitions.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/state.py`
    State {
        entity_id: String,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
    },

    /// Fire on an event-bus event of the given type.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/event.py`
    Event {
        event_type: String,
        #[serde(default)]
        event_data: Option<Value>,
    },

    /// Fire when a numeric state crosses a threshold.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/numeric_state.py`
    NumericState {
        entity_id: String,
        #[serde(default)]
        above: Option<f64>,
        #[serde(default)]
        below: Option<f64>,
    },

    /// Fire when a template evaluates truthy.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/template.py`
    Template { value_template: String },

    /// Fire at an absolute clock time "HH:MM[:SS]".
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/time.py`
    Time { at: String },

    /// Cron-style minute/hour/seconds match.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/time_pattern.py`
    TimePattern {
        #[serde(default)]
        hours: Option<String>,
        #[serde(default)]
        minutes: Option<String>,
        #[serde(default)]
        seconds: Option<String>,
    },

    /// Fire on Home Assistant start or shutdown.
    ///
    /// # Upstream: `homeassistant/components/homeassistant/triggers/homeassistant.py`
    Homeassistant {
        /// `"start"` or `"shutdown"`.
        event: String,
    },
}

impl Trigger {
    /// Subscribed event type for triggers that wire directly to the bus
    /// (state / event / `numeric_state` / template / homeassistant). The
    /// time-based triggers are driven by the scheduler tick — they
    /// return `None` here.
    #[must_use]
    pub fn subscribed_event_type(&self) -> Option<&'static str> {
        match self {
            Self::State { .. } | Self::NumericState { .. } | Self::Template { .. } => {
                Some(crate::event_bus::EVENT_STATE_CHANGED)
            }
            Self::Homeassistant { event } => match event.as_str() {
                "shutdown" => Some(crate::event_bus::EVENT_HASS_STOP),
                // "start", and any other value, subscribes to start.
                _ => Some(crate::event_bus::EVENT_HASS_START),
            },
            // `Event`: event_type is dynamic; engine matches manually.
            // `Time` / `TimePattern`: driven by the scheduler tick.
            Self::Event { .. } | Self::Time { .. } | Self::TimePattern { .. } => None,
        }
    }

    /// Evaluate against an event — returns true when the trigger should
    /// fire. The state machine is needed for `numeric_state` and
    /// `template` evaluation; the time-based variants are not called
    /// here (the engine wires them through a scheduler tick instead).
    pub fn matches(
        &self,
        event: &Event,
        sm: Option<&std::sync::Arc<crate::state::StateMachine>>,
    ) -> HassResult<bool> {
        match self {
            Self::State {
                entity_id,
                from,
                to,
            } => Ok(state_trigger_matches(
                event,
                entity_id,
                from.as_deref(),
                to.as_deref(),
            )),
            Self::Event {
                event_type,
                event_data,
            } => {
                if &event.event_type != event_type {
                    return Ok(false);
                }
                if let Some(expected) = event_data {
                    return Ok(json_subset(expected, &event.data));
                }
                Ok(true)
            }
            Self::NumericState {
                entity_id,
                above,
                below,
            } => numeric_state_trigger_matches(event, sm, entity_id, *above, *below),
            Self::Template { value_template } => {
                let sm = sm.ok_or_else(|| {
                    HassError::TemplateError("template trigger needs state machine".into())
                })?;
                let tmpl = crate::template::Template::new(value_template, sm.clone());
                tmpl.render_bool(&serde_json::to_value(event).unwrap_or(Value::Null))
            }
            Self::Homeassistant { event: which } => Ok(event.event_type
                == match which.as_str() {
                    "shutdown" => crate::event_bus::EVENT_HASS_STOP,
                    _ => crate::event_bus::EVENT_HASS_START,
                }),
            Self::Time { .. } | Self::TimePattern { .. } => Ok(false),
        }
    }

    /// Check whether the trigger's "tick" condition fires at `tick`.
    #[must_use]
    pub fn matches_tick(&self, tick: time::OffsetDateTime) -> bool {
        match self {
            Self::Time { at } => parse_hms(at).is_some_and(|(h, m, s)| {
                tick.hour() == h && tick.minute() == m && tick.second() == s
            }),
            Self::TimePattern {
                hours,
                minutes,
                seconds,
            } => {
                let h_ok = hours
                    .as_deref()
                    .is_none_or(|h| cron_match(h, u32::from(tick.hour()), 23));
                let m_ok = minutes
                    .as_deref()
                    .is_none_or(|m| cron_match(m, u32::from(tick.minute()), 59));
                let s_ok = seconds
                    .as_deref()
                    .is_none_or(|s| cron_match(s, u32::from(tick.second()), 59));
                h_ok && m_ok && s_ok
            }
            _ => false,
        }
    }
}

/// `true` when `event` is a `state_changed` event about `entity_id`.
fn is_state_change_of(event: &Event, entity_id: &str) -> bool {
    event.event_type == crate::event_bus::EVENT_STATE_CHANGED
        && event
            .data
            .get("entity_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            == entity_id
}

/// The `state` string of the `old_state` / `new_state` object (`key`) in
/// a `state_changed` payload, if there is one.
fn payload_state<'a>(event: &'a Event, key: &str) -> Option<&'a str> {
    event
        .data
        .get(key)
        .and_then(|v| v.get("state"))
        .and_then(Value::as_str)
}

/// `state` platform: `event` is a state change of `entity_id` whose old
/// and new state strings satisfy the optional `from` / `to` filters.
fn state_trigger_matches(
    event: &Event,
    entity_id: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> bool {
    if !is_state_change_of(event, entity_id) {
        return false;
    }
    let old = payload_state(event, "old_state");
    let new = payload_state(event, "new_state");
    from.is_none_or(|want| old == Some(want)) && to.is_none_or(|want| new == Some(want))
}

/// `numeric_state` platform: `event` is a state change of `entity_id`
/// and the entity's numeric state lies strictly inside the configured
/// bounds. With neither bound set the trigger never fires.
///
/// The value comes from the state machine when one is given and knows
/// the entity, otherwise from the event's `new_state`; a value that is
/// not numeric is a [`HassError::ConditionError`].
fn numeric_state_trigger_matches(
    event: &Event,
    sm: Option<&std::sync::Arc<crate::state::StateMachine>>,
    entity_id: &str,
    above: Option<f64>,
    below: Option<f64>,
) -> HassResult<bool> {
    if !is_state_change_of(event, entity_id) {
        return Ok(false);
    }
    let state_value = if let Some(s) = sm.and_then(|sm| sm.get(entity_id)) {
        parse_number(&s.state).ok_or_else(|| {
            HassError::ConditionError(format!(
                "entity {entity_id} state '{}' is not numeric",
                s.state
            ))
        })?
    } else {
        payload_state(event, "new_state")
            .and_then(parse_number)
            .ok_or_else(|| {
                HassError::ConditionError(format!("entity {entity_id} new state is not numeric"))
            })?
    };
    if let Some(a) = above {
        if !is_above(state_value, a) {
            return Ok(false);
        }
    }
    if let Some(b) = below {
        if !is_below(state_value, b) {
            return Ok(false);
        }
    }
    Ok(above.is_some() || below.is_some())
}

fn parse_number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

fn parse_hms(s: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = s.split(':').collect();
    let h = parts.first()?.parse::<u8>().ok()?;
    let m = parts.get(1)?.parse::<u8>().ok()?;
    let s = parts.get(2).and_then(|x| x.parse::<u8>().ok()).unwrap_or(0);
    Some((h, m, s))
}

/// Minimal cron-style pattern matcher (supports `*`, `*/N`, integer
/// literal). HA's full cron grammar is larger; this is the MVP slice.
fn cron_match(pattern: &str, value: u32, max: u32) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(stripped) = pattern.strip_prefix("*/") {
        if let Ok(step) = stripped.parse::<u32>() {
            return step != 0 && value % step == 0 && value <= max;
        }
    }
    pattern.parse::<u32>().is_ok_and(|p| p == value)
}

/// Recursive "expected is a subset of actual" check for `event_data`
/// matching.
fn json_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, ev)| a.get(k).is_some_and(|av| json_subset(ev, av))),
        (Value::Array(e), Value::Array(a)) => e
            .iter()
            .enumerate()
            .all(|(i, ev)| a.get(i).is_some_and(|av| json_subset(ev, av))),
        _ => expected == actual,
    }
}

/// Tiny helper used by the trigger engine to surface the entity state
/// from a `state_changed` event payload.
#[must_use]
pub fn event_new_state(event: &Event) -> Option<State> {
    event
        .data
        .get("new_state")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value::<State>(v.clone()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;
    use crate::event_bus::{EVENT_STATE_CHANGED, EventOrigin};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn state_changed_event(entity_id: &str, old: Option<&str>, new: &str) -> Event {
        let payload = serde_json::json!({
            "entity_id": entity_id,
            "old_state": old.map(|s| serde_json::json!({"state": s})),
            "new_state": {"state": new},
        });
        Event::new(
            EVENT_STATE_CHANGED.into(),
            payload,
            EventOrigin::Local,
            Context::new(),
        )
    }

    /// Upstream-test: `tests/components/homeassistant/triggers/test_state.py::test_if_fires_on_entity_change`
    #[test]
    fn state_trigger_fires_on_change() -> TestResult {
        let t = Trigger::State {
            entity_id: "light.kitchen".into(),
            from: Some("off".into()),
            to: Some("on".into()),
        };
        let evt = state_changed_event("light.kitchen", Some("off"), "on");
        assert!(t.matches(&evt, None)?);
        let evt2 = state_changed_event("light.kitchen", Some("on"), "off");
        assert!(!t.matches(&evt2, None)?);
        Ok(())
    }

    /// Upstream-test: `tests/components/homeassistant/triggers/test_event.py::test_if_fires_on_event`
    #[test]
    fn event_trigger_fires_on_event_type() -> TestResult {
        let t = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: None,
        };
        let evt = Event::new(
            "doorbell_pressed".into(),
            serde_json::json!({"front": true}),
            EventOrigin::Local,
            Context::new(),
        );
        assert!(t.matches(&evt, None)?);

        let t_with_data = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: Some(serde_json::json!({"front": true})),
        };
        assert!(t_with_data.matches(&evt, None)?);

        let t_with_other = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: Some(serde_json::json!({"front": false})),
        };
        assert!(!t_with_other.matches(&evt, None)?);
        Ok(())
    }

    /// Upstream-test: `tests/components/homeassistant/triggers/test_numeric_state.py::test_if_fires_on_entity_change_below`
    #[test]
    fn numeric_state_trigger_fires_when_crosses_threshold() -> TestResult {
        let t = Trigger::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(20.0),
            below: None,
        };
        let evt = serde_json::json!({
            "entity_id": "sensor.temp",
            "old_state": {"state": "10"},
            "new_state": {"state": "25"},
        });
        let event = Event::new(
            crate::event_bus::EVENT_STATE_CHANGED.into(),
            evt,
            crate::event_bus::EventOrigin::Local,
            Context::new(),
        );
        assert!(t.matches(&event, None)?);

        let t_below = Trigger::NumericState {
            entity_id: "sensor.temp".into(),
            above: None,
            below: Some(15.0),
        };
        assert!(!t_below.matches(&event, None)?);
        Ok(())
    }

    #[test]
    fn time_pattern_trigger_matches() -> TestResult {
        let t = Trigger::TimePattern {
            hours: Some("*/2".into()),
            minutes: Some("0".into()),
            seconds: Some("0".into()),
        };
        let at_even = time::OffsetDateTime::from_unix_timestamp(0)? + time::Duration::hours(4);
        assert!(t.matches_tick(at_even));
        let at_odd = time::OffsetDateTime::from_unix_timestamp(0)? + time::Duration::hours(3);
        assert!(!t.matches_tick(at_odd));
        Ok(())
    }

    #[test]
    fn is_state_change_of_checks_event_type_and_entity() {
        let evt = state_changed_event("light.kitchen", Some("off"), "on");
        assert!(is_state_change_of(&evt, "light.kitchen"));
        assert!(!is_state_change_of(&evt, "light.hall"));
        let other = Event::new(
            "doorbell_pressed".into(),
            serde_json::json!({"entity_id": "light.kitchen"}),
            EventOrigin::Local,
            Context::new(),
        );
        assert!(!is_state_change_of(&other, "light.kitchen"));
    }

    #[test]
    fn payload_state_reads_old_and_new_state() {
        let evt = state_changed_event("light.kitchen", Some("off"), "on");
        assert_eq!(payload_state(&evt, "old_state"), Some("off"));
        assert_eq!(payload_state(&evt, "new_state"), Some("on"));
        let first = state_changed_event("light.kitchen", None, "on");
        assert_eq!(payload_state(&first, "old_state"), None);
        assert_eq!(payload_state(&first, "no_such_key"), None);
    }

    #[test]
    fn state_trigger_matches_applies_from_and_to_filters() {
        let evt = state_changed_event("light.kitchen", Some("off"), "on");
        assert!(state_trigger_matches(&evt, "light.kitchen", None, None));
        assert!(state_trigger_matches(
            &evt,
            "light.kitchen",
            Some("off"),
            Some("on")
        ));
        assert!(!state_trigger_matches(
            &evt,
            "light.kitchen",
            Some("on"),
            None
        ));
        assert!(!state_trigger_matches(
            &evt,
            "light.kitchen",
            None,
            Some("off")
        ));
        assert!(!state_trigger_matches(&evt, "light.hall", None, None));
        // A first report has no old state, so a `from` filter cannot match.
        let first = state_changed_event("light.kitchen", None, "on");
        assert!(!state_trigger_matches(
            &first,
            "light.kitchen",
            Some("off"),
            None
        ));
        assert!(state_trigger_matches(
            &first,
            "light.kitchen",
            None,
            Some("on")
        ));
    }

    #[test]
    fn numeric_state_trigger_matches_checks_strict_bounds() -> TestResult {
        let evt = state_changed_event("sensor.temp", Some("10"), "25");
        let check = |entity: &str, above, below| {
            numeric_state_trigger_matches(&evt, None, entity, above, below)
        };
        assert!(check("sensor.temp", Some(20.0), None)?);
        assert!(check("sensor.temp", Some(20.0), Some(30.0))?);
        // Both bounds are strict.
        assert!(!check("sensor.temp", Some(25.0), None)?);
        assert!(!check("sensor.temp", None, Some(25.0))?);
        // Without any bound the trigger never fires.
        assert!(!check("sensor.temp", None, None)?);
        // Another entity's change is not a match (and not an error).
        assert!(!check("sensor.other", Some(20.0), None)?);
        Ok(())
    }

    #[test]
    fn numeric_state_trigger_matches_handles_non_numeric_and_nan() -> TestResult {
        let bad = state_changed_event("sensor.temp", Some("10"), "warm");
        assert!(matches!(
            numeric_state_trigger_matches(&bad, None, "sensor.temp", Some(20.0), None),
            Err(HassError::ConditionError(_))
        ));
        // "NaN" parses as a number but is neither above nor below anything.
        let nan = state_changed_event("sensor.temp", Some("10"), "NaN");
        assert!(!numeric_state_trigger_matches(
            &nan,
            None,
            "sensor.temp",
            Some(20.0),
            None
        )?);
        assert!(!numeric_state_trigger_matches(
            &nan,
            None,
            "sensor.temp",
            None,
            Some(20.0)
        )?);
        Ok(())
    }

    #[test]
    fn numeric_state_trigger_prefers_the_state_machine_value() -> TestResult {
        let bus = std::sync::Arc::new(crate::event_bus::InMemoryEventBus::new());
        let sm = std::sync::Arc::new(crate::state::StateMachine::new(bus));
        sm.async_set(
            "sensor.temp",
            "5",
            std::collections::BTreeMap::new(),
            false,
            None,
        )?;
        // The payload says 25, the state machine says 5: the latter wins.
        let evt = state_changed_event("sensor.temp", Some("10"), "25");
        assert!(!numeric_state_trigger_matches(
            &evt,
            Some(&sm),
            "sensor.temp",
            Some(20.0),
            None
        )?);
        assert!(numeric_state_trigger_matches(
            &evt,
            Some(&sm),
            "sensor.temp",
            None,
            Some(20.0)
        )?);
        Ok(())
    }

    #[test]
    fn homeassistant_trigger_subscribes_to_start_unless_shutdown() {
        let subscribed = |event: &str| {
            Trigger::Homeassistant {
                event: event.into(),
            }
            .subscribed_event_type()
        };
        assert_eq!(
            subscribed("start"),
            Some(crate::event_bus::EVENT_HASS_START)
        );
        assert_eq!(
            subscribed("shutdown"),
            Some(crate::event_bus::EVENT_HASS_STOP)
        );
        assert_eq!(
            subscribed("other"),
            Some(crate::event_bus::EVENT_HASS_START)
        );
        let event = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: None,
        };
        assert_eq!(event.subscribed_event_type(), None);
        let time = Trigger::Time { at: "07:00".into() };
        assert_eq!(time.subscribed_event_type(), None);
    }
}
