// SPDX-License-Identifier: Apache-2.0
//! Trigger definitions — port of
//! `homeassistant/components/homeassistant/triggers/*.py`.
//!
//! Phase 1 covers: state, event, numeric_state, template, time,
//! time_pattern, homeassistant (start/stop).
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/components/homeassistant/triggers/

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{HassError, HassResult};
use crate::event_bus::Event;
use crate::state::State;

/// Variants of a trigger declaration — the YAML the user writes maps
/// 1:1 to these variants.
///
/// Phase 1 covers the seven mandated platforms from `parity.manifest.toml`.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/components/homeassistant/triggers/
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum Trigger {
    /// Fire when the state of an entity transitions.
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/state.py
    State {
        entity_id: String,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
    },

    /// Fire on an event-bus event of the given type.
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/event.py
    Event {
        event_type: String,
        #[serde(default)]
        event_data: Option<Value>,
    },

    /// Fire when a numeric state crosses a threshold.
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/numeric_state.py
    NumericState {
        entity_id: String,
        #[serde(default)]
        above: Option<f64>,
        #[serde(default)]
        below: Option<f64>,
    },

    /// Fire when a template evaluates truthy.
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/template.py
    Template { value_template: String },

    /// Fire at an absolute clock time "HH:MM[:SS]".
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/time.py
    Time { at: String },

    /// Cron-style minute/hour/seconds match.
    ///
    /// # Upstream: homeassistant/components/homeassistant/triggers/time_pattern.py
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
    /// # Upstream: homeassistant/components/homeassistant/triggers/homeassistant.py
    Homeassistant {
        /// `"start"` or `"shutdown"`.
        event: String,
    },
}

impl Trigger {
    /// Subscribed event type for triggers that wire directly to the bus
    /// (state / event / numeric_state / template / homeassistant). The
    /// time-based triggers are driven by the scheduler tick — they
    /// return `None` here.
    #[must_use]
    pub fn subscribed_event_type(&self) -> Option<&'static str> {
        match self {
            Self::State { .. } | Self::NumericState { .. } | Self::Template { .. } => {
                Some(crate::event_bus::EVENT_STATE_CHANGED)
            }
            Self::Event { .. } => None, // event_type is dynamic; engine matches manually
            Self::Homeassistant { event } => match event.as_str() {
                "start" => Some(crate::event_bus::EVENT_HASS_START),
                "shutdown" => Some(crate::event_bus::EVENT_HASS_STOP),
                _ => Some(crate::event_bus::EVENT_HASS_START),
            },
            Self::Time { .. } | Self::TimePattern { .. } => None,
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
            Self::State { entity_id, from, to } => {
                if event.event_type != crate::event_bus::EVENT_STATE_CHANGED {
                    return Ok(false);
                }
                let evt_entity = event
                    .data
                    .get("entity_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if evt_entity != entity_id {
                    return Ok(false);
                }
                let old = event
                    .data
                    .get("old_state")
                    .and_then(|v| v.get("state"))
                    .and_then(Value::as_str);
                let new = event
                    .data
                    .get("new_state")
                    .and_then(|v| v.get("state"))
                    .and_then(Value::as_str);
                if let Some(want) = from.as_deref() {
                    if old != Some(want) {
                        return Ok(false);
                    }
                }
                if let Some(want) = to.as_deref() {
                    if new != Some(want) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Event { event_type, event_data } => {
                if &event.event_type != event_type {
                    return Ok(false);
                }
                if let Some(expected) = event_data {
                    return Ok(json_subset(expected, &event.data));
                }
                Ok(true)
            }
            Self::NumericState { entity_id, above, below } => {
                if event.event_type != crate::event_bus::EVENT_STATE_CHANGED {
                    return Ok(false);
                }
                let evt_entity = event
                    .data
                    .get("entity_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if evt_entity != entity_id {
                    return Ok(false);
                }
                let state_value = if let Some(s) = sm.and_then(|sm| sm.get(entity_id.as_str())) {
                    parse_number(&s.state).ok_or_else(|| {
                        HassError::ConditionError(format!(
                            "entity {entity_id} state '{}' is not numeric",
                            s.state
                        ))
                    })?
                } else {
                    event
                        .data
                        .get("new_state")
                        .and_then(|v| v.get("state"))
                        .and_then(Value::as_str)
                        .and_then(parse_number)
                        .ok_or_else(|| {
                            HassError::ConditionError(format!(
                                "entity {entity_id} new state is not numeric"
                            ))
                        })?
                };
                if let Some(a) = above {
                    if !(state_value > *a) {
                        return Ok(false);
                    }
                }
                if let Some(b) = below {
                    if !(state_value < *b) {
                        return Ok(false);
                    }
                }
                Ok(above.is_some() || below.is_some())
            }
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
    pub fn matches_tick(&self, tick: time::OffsetDateTime) -> bool {
        match self {
            Self::Time { at } => parse_hms(at).is_some_and(|(h, m, s)| {
                tick.hour() == h && tick.minute() == m && tick.second() == s
            }),
            Self::TimePattern { hours, minutes, seconds } => {
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

/// Recursive "expected is a subset of actual" check for event_data
/// matching.
fn json_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, ev)| a.get(k).is_some_and(|av| json_subset(ev, av))),
        (Value::Array(e), Value::Array(a)) => e.iter().enumerate().all(|(i, ev)| {
            a.get(i).is_some_and(|av| json_subset(ev, av))
        }),
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
    fn state_trigger_fires_on_change() {
        let t = Trigger::State {
            entity_id: "light.kitchen".into(),
            from: Some("off".into()),
            to: Some("on".into()),
        };
        let evt = state_changed_event("light.kitchen", Some("off"), "on");
        assert!(t.matches(&evt, None).unwrap());
        let evt2 = state_changed_event("light.kitchen", Some("on"), "off");
        assert!(!t.matches(&evt2, None).unwrap());
    }

    /// Upstream-test: `tests/components/homeassistant/triggers/test_event.py::test_if_fires_on_event`
    #[test]
    fn event_trigger_fires_on_event_type() {
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
        assert!(t.matches(&evt, None).unwrap());

        let t_with_data = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: Some(serde_json::json!({"front": true})),
        };
        assert!(t_with_data.matches(&evt, None).unwrap());

        let t_with_other = Trigger::Event {
            event_type: "doorbell_pressed".into(),
            event_data: Some(serde_json::json!({"front": false})),
        };
        assert!(!t_with_other.matches(&evt, None).unwrap());
    }

    /// Upstream-test: `tests/components/homeassistant/triggers/test_numeric_state.py::test_if_fires_on_entity_change_below`
    #[test]
    fn numeric_state_trigger_fires_when_crosses_threshold() {
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
        assert!(t.matches(&event, None).unwrap());

        let t_below = Trigger::NumericState {
            entity_id: "sensor.temp".into(),
            above: None,
            below: Some(15.0),
        };
        assert!(!t_below.matches(&event, None).unwrap());
    }

    #[test]
    fn time_pattern_trigger_matches() {
        let t = Trigger::TimePattern {
            hours: Some("*/2".into()),
            minutes: Some("0".into()),
            seconds: Some("0".into()),
        };
        let at_even =
            time::OffsetDateTime::from_unix_timestamp(0).unwrap() + time::Duration::hours(4);
        assert!(t.matches_tick(at_even));
        let at_odd =
            time::OffsetDateTime::from_unix_timestamp(0).unwrap() + time::Duration::hours(3);
        assert!(!t.matches_tick(at_odd));
    }
}
