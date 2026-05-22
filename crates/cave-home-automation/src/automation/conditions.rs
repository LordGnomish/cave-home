// SPDX-License-Identifier: Apache-2.0
//! Condition AST — port of `homeassistant/helpers/condition.py`.
//!
//! # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/condition.py

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::error::{HassError, HassResult};
use crate::state::StateMachine;
use crate::template::Template;

/// AST of a condition expression.
///
/// # Upstream: home-assistant/core@456202325ac4:homeassistant/helpers/condition.py
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "snake_case")]
pub enum Condition {
    /// Entity is in one of the given states.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_state
    State {
        entity_id: String,
        /// State value (or list of accepted values).
        state: ConditionStateValue,
    },

    /// Entity numeric state is within bounds.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_numeric_state
    NumericState {
        entity_id: String,
        #[serde(default)]
        above: Option<f64>,
        #[serde(default)]
        below: Option<f64>,
    },

    /// Template renders truthy.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_template
    Template { value_template: String },

    /// Now is between two clock times "HH:MM[:SS]".
    ///
    /// # Upstream: homeassistant/helpers/condition.py::time
    Time {
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        before: Option<String>,
    },

    /// All children must evaluate true.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_and_from_config
    And { conditions: Vec<Condition> },

    /// Any child evaluating true is enough.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_or_from_config
    Or { conditions: Vec<Condition> },

    /// Negate the inner condition.
    ///
    /// # Upstream: homeassistant/helpers/condition.py::async_not_from_config
    Not { conditions: Vec<Condition> },
}

/// "state" condition accepts either a single value or a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConditionStateValue {
    Single(String),
    Many(Vec<String>),
}

impl ConditionStateValue {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Single(s) => s == value,
            Self::Many(v) => v.iter().any(|s| s == value),
        }
    }
}

impl Condition {
    /// Evaluate this condition.
    pub fn evaluate(&self, sm: &Arc<StateMachine>) -> HassResult<bool> {
        match self {
            Self::State { entity_id, state } => Ok(sm
                .get(entity_id)
                .is_some_and(|s| state.matches(&s.state))),
            Self::NumericState { entity_id, above, below } => {
                let Some(s) = sm.get(entity_id) else {
                    return Ok(false);
                };
                let v = s.state.parse::<f64>().map_err(|_| {
                    HassError::ConditionError(format!(
                        "entity {entity_id} state '{}' is not numeric",
                        s.state
                    ))
                })?;
                if let Some(a) = above {
                    if !(v > *a) {
                        return Ok(false);
                    }
                }
                if let Some(b) = below {
                    if !(v < *b) {
                        return Ok(false);
                    }
                }
                Ok(above.is_some() || below.is_some())
            }
            Self::Template { value_template } => {
                let tmpl = Template::new(value_template, sm.clone());
                tmpl.render_bool(&Value::Null)
            }
            Self::Time { after, before } => {
                let now = OffsetDateTime::now_utc();
                let now_sec =
                    u32::from(now.hour()) * 3600 + u32::from(now.minute()) * 60 + u32::from(now.second());
                if let Some(a) = after.as_deref() {
                    let a_sec = hms_to_seconds(a)
                        .ok_or_else(|| HassError::ConditionError(format!("bad after: {a}")))?;
                    if !(now_sec >= a_sec) {
                        return Ok(false);
                    }
                }
                if let Some(b) = before.as_deref() {
                    let b_sec = hms_to_seconds(b)
                        .ok_or_else(|| HassError::ConditionError(format!("bad before: {b}")))?;
                    if !(now_sec < b_sec) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::And { conditions } => {
                for c in conditions {
                    if !c.evaluate(sm)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Or { conditions } => {
                for c in conditions {
                    if c.evaluate(sm)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Not { conditions } => {
                for c in conditions {
                    if c.evaluate(sm)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
}

fn hms_to_seconds(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    let h: u32 = parts.first()?.parse().ok()?;
    let m: u32 = parts.get(1)?.parse().ok()?;
    let sec: u32 = parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(0);
    Some(h * 3600 + m * 60 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryEventBus;
    use std::collections::BTreeMap;

    fn machine_with(entity_id: &str, state: &str) -> Arc<StateMachine> {
        let sm = Arc::new(StateMachine::new(Arc::new(InMemoryEventBus::new())));
        sm.async_set(entity_id, state, BTreeMap::new(), false, None)
            .unwrap();
        sm
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_state_raises`
    #[test]
    fn state_condition_matches() {
        let sm = machine_with("light.kitchen", "on");
        let c = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Single("on".into()),
        };
        assert!(c.evaluate(&sm).unwrap());

        let c_off = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Single("off".into()),
        };
        assert!(!c_off.evaluate(&sm).unwrap());

        let c_many = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Many(vec!["off".into(), "on".into()]),
        };
        assert!(c_many.evaluate(&sm).unwrap());
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_and_condition`
    #[test]
    fn and_condition_short_circuits() {
        let sm = machine_with("light.kitchen", "on");
        let c = Condition::And {
            conditions: vec![
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("on".into()),
                },
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("on".into()),
                },
            ],
        };
        assert!(c.evaluate(&sm).unwrap());

        let c_no = Condition::And {
            conditions: vec![
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("on".into()),
                },
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("off".into()),
                },
            ],
        };
        assert!(!c_no.evaluate(&sm).unwrap());
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_or_condition`
    #[test]
    fn or_condition_short_circuits() {
        let sm = machine_with("light.kitchen", "off");
        let c = Condition::Or {
            conditions: vec![
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("on".into()),
                },
                Condition::State {
                    entity_id: "light.kitchen".into(),
                    state: ConditionStateValue::Single("off".into()),
                },
            ],
        };
        assert!(c.evaluate(&sm).unwrap());
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_not_condition`
    #[test]
    fn not_condition_negates() {
        let sm = machine_with("light.kitchen", "on");
        let c = Condition::Not {
            conditions: vec![Condition::State {
                entity_id: "light.kitchen".into(),
                state: ConditionStateValue::Single("off".into()),
            }],
        };
        assert!(c.evaluate(&sm).unwrap());
    }

    #[test]
    fn numeric_state_condition_evaluates() {
        let sm = machine_with("sensor.temp", "22");
        let c = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(20.0),
            below: Some(30.0),
        };
        assert!(c.evaluate(&sm).unwrap());
    }
}
