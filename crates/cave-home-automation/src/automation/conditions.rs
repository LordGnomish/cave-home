// SPDX-License-Identifier: Apache-2.0
//! Condition AST — port of `homeassistant/helpers/condition.py`.
//!
//! # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/condition.py`

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::error::{HassError, HassResult};
use crate::num::{is_above, is_below};
use crate::state::StateMachine;
use crate::template::Template;

/// AST of a condition expression.
///
/// # Upstream: `home-assistant/core@456202325ac4:homeassistant/helpers/condition.py`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "snake_case")]
pub enum Condition {
    /// Entity is in one of the given states.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_state`
    State {
        entity_id: String,
        /// State value (or list of accepted values).
        state: ConditionStateValue,
    },

    /// Entity numeric state is within bounds.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_numeric_state`
    NumericState {
        entity_id: String,
        #[serde(default)]
        above: Option<f64>,
        #[serde(default)]
        below: Option<f64>,
    },

    /// Template renders truthy.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_template`
    Template { value_template: String },

    /// Now is between two clock times "HH:MM[:SS]".
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::time`
    Time {
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        before: Option<String>,
    },

    /// All children must evaluate true.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_and_from_config`
    And { conditions: Vec<Self> },

    /// Any child evaluating true is enough.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_or_from_config`
    Or { conditions: Vec<Self> },

    /// Negate the inner condition.
    ///
    /// # Upstream: `homeassistant/helpers/condition.py::async_not_from_config`
    Not { conditions: Vec<Self> },
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
            Self::State { entity_id, state } => {
                Ok(sm.get(entity_id).is_some_and(|s| state.matches(&s.state)))
            }
            Self::NumericState {
                entity_id,
                above,
                below,
            } => {
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
                    if !is_above(v, *a) {
                        return Ok(false);
                    }
                }
                if let Some(b) = below {
                    if !is_below(v, *b) {
                        return Ok(false);
                    }
                }
                Ok(above.is_some() || below.is_some())
            }
            Self::Template { value_template } => {
                let tmpl = Template::new(value_template, sm.clone());
                tmpl.render_bool(&Value::Null)
            }
            Self::Time { after, before } => time_window_contains(
                seconds_of_day(OffsetDateTime::now_utc()),
                after.as_deref(),
                before.as_deref(),
            ),
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

/// Seconds since midnight on `t`'s own clock (its offset is kept as is).
fn seconds_of_day(t: OffsetDateTime) -> u32 {
    u32::from(t.hour()) * 3600 + u32::from(t.minute()) * 60 + u32::from(t.second())
}

/// `true` when `now_sec` (seconds since midnight) is inside the window of
/// the `time` condition: at or after `after` and before `before`.
fn time_window_contains(
    now_sec: u32,
    after: Option<&str>,
    before: Option<&str>,
) -> HassResult<bool> {
    if let Some(a) = after {
        let a_sec = hms_to_seconds(a)
            .ok_or_else(|| HassError::ConditionError(format!("bad after: {a}")))?;
        if now_sec < a_sec {
            return Ok(false);
        }
    }
    if let Some(b) = before {
        let b_sec = hms_to_seconds(b)
            .ok_or_else(|| HassError::ConditionError(format!("bad before: {b}")))?;
        if now_sec >= b_sec {
            return Ok(false);
        }
    }
    Ok(true)
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

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn machine_with(entity_id: &str, state: &str) -> HassResult<Arc<StateMachine>> {
        let sm = Arc::new(StateMachine::new(Arc::new(InMemoryEventBus::new())));
        sm.async_set(entity_id, state, BTreeMap::new(), false, None)?;
        Ok(sm)
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_state_raises`
    #[test]
    fn state_condition_matches() -> TestResult {
        let sm = machine_with("light.kitchen", "on")?;
        let c = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Single("on".into()),
        };
        assert!(c.evaluate(&sm)?);

        let c_off = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Single("off".into()),
        };
        assert!(!c_off.evaluate(&sm)?);

        let c_many = Condition::State {
            entity_id: "light.kitchen".into(),
            state: ConditionStateValue::Many(vec!["off".into(), "on".into()]),
        };
        assert!(c_many.evaluate(&sm)?);
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_and_condition`
    #[test]
    fn and_condition_short_circuits() -> TestResult {
        let sm = machine_with("light.kitchen", "on")?;
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
        assert!(c.evaluate(&sm)?);

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
        assert!(!c_no.evaluate(&sm)?);
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_or_condition`
    #[test]
    fn or_condition_short_circuits() -> TestResult {
        let sm = machine_with("light.kitchen", "off")?;
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
        assert!(c.evaluate(&sm)?);
        Ok(())
    }

    /// Upstream-test: `tests/helpers/test_condition.py::test_not_condition`
    #[test]
    fn not_condition_negates() -> TestResult {
        let sm = machine_with("light.kitchen", "on")?;
        let c = Condition::Not {
            conditions: vec![Condition::State {
                entity_id: "light.kitchen".into(),
                state: ConditionStateValue::Single("off".into()),
            }],
        };
        assert!(c.evaluate(&sm)?);
        Ok(())
    }

    #[test]
    fn numeric_state_condition_evaluates() -> TestResult {
        let sm = machine_with("sensor.temp", "22")?;
        let c = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(20.0),
            below: Some(30.0),
        };
        assert!(c.evaluate(&sm)?);
        Ok(())
    }

    #[test]
    fn numeric_state_condition_bounds_are_strict() -> TestResult {
        let sm = machine_with("sensor.temp", "20")?;
        let above = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(20.0),
            below: None,
        };
        assert!(!above.evaluate(&sm)?);
        let below = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: None,
            below: Some(20.0),
        };
        assert!(!below.evaluate(&sm)?);
        let window = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(19.5),
            below: Some(20.5),
        };
        assert!(window.evaluate(&sm)?);
        Ok(())
    }

    #[test]
    fn numeric_state_condition_never_matches_nan() -> TestResult {
        // "NaN" parses as a number but is neither above nor below anything.
        let sm = machine_with("sensor.temp", "NaN")?;
        let above = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: Some(20.0),
            below: None,
        };
        assert!(!above.evaluate(&sm)?);
        let below = Condition::NumericState {
            entity_id: "sensor.temp".into(),
            above: None,
            below: Some(20.0),
        };
        assert!(!below.evaluate(&sm)?);
        Ok(())
    }

    fn empty_machine() -> Arc<StateMachine> {
        Arc::new(StateMachine::new(Arc::new(InMemoryEventBus::new())))
    }

    fn time_condition(after: Option<&str>, before: Option<&str>) -> Condition {
        Condition::Time {
            after: after.map(str::to_owned),
            before: before.map(str::to_owned),
        }
    }

    /// The message of a `ConditionError`, if `result` is one.
    fn condition_error(result: HassResult<bool>) -> Option<String> {
        match result {
            Err(HassError::ConditionError(msg)) => Some(msg),
            _ => None,
        }
    }

    /// Check the window at each `(hour, minute, second, expected)` row.
    #[track_caller]
    fn assert_window(
        after: Option<&str>,
        before: Option<&str>,
        rows: &[(u32, u32, u32, bool)],
    ) -> TestResult {
        for &(h, m, s, expected) in rows {
            let got = time_window_contains(h * 3600 + m * 60 + s, after, before)?;
            assert_eq!(
                got, expected,
                "after={after:?} before={before:?} at {h:02}:{m:02}:{s:02}"
            );
        }
        Ok(())
    }

    #[test]
    fn seconds_of_day_counts_from_midnight_on_the_given_clock() {
        use time::macros::datetime;

        assert_eq!(seconds_of_day(datetime!(2026-11-23 00:00:00 UTC)), 0);
        assert_eq!(seconds_of_day(datetime!(2026-11-23 06:00:00 UTC)), 21_600);
        assert_eq!(seconds_of_day(datetime!(2026-11-23 23:59:59 UTC)), 86_399);
        // Sub-second precision is dropped, not rounded.
        assert_eq!(
            seconds_of_day(datetime!(2026-11-23 21:59:59.999 UTC)),
            79_199
        );
        // The value's own offset is used; nothing is converted to UTC.
        assert_eq!(seconds_of_day(datetime!(2026-11-23 22:30:15 +1)), 81_015);
    }

    #[test]
    fn time_window_same_day_includes_after_and_excludes_before() -> TestResult {
        assert_window(
            Some("08:00"),
            Some("17:00"),
            &[
                (0, 0, 0, false),
                (7, 59, 59, false),
                (8, 0, 0, true),
                (12, 0, 0, true),
                (16, 59, 59, true),
                (17, 0, 0, false),
                (23, 59, 59, false),
            ],
        )
    }

    #[test]
    fn time_window_crossing_midnight_wraps_to_the_next_day() -> TestResult {
        assert_window(
            Some("22:00"),
            Some("06:00"),
            &[
                (21, 59, 59, false),
                (22, 0, 0, true),
                (23, 59, 59, true),
                (0, 0, 0, true),
                (5, 59, 59, true),
                (6, 0, 0, false),
                (12, 0, 0, false),
            ],
        )
    }

    #[test]
    fn time_window_with_only_after_runs_until_midnight() -> TestResult {
        assert_window(
            Some("22:00"),
            None,
            &[
                (0, 0, 0, false),
                (21, 59, 59, false),
                (22, 0, 0, true),
                (23, 59, 59, true),
            ],
        )
    }

    #[test]
    fn time_window_with_only_before_runs_from_midnight() -> TestResult {
        assert_window(
            None,
            Some("06:00"),
            &[
                (0, 0, 0, true),
                (5, 59, 59, true),
                (6, 0, 0, false),
                (23, 59, 59, false),
            ],
        )
    }

    #[test]
    fn time_window_without_bounds_always_matches() -> TestResult {
        assert_window(
            None,
            None,
            &[(0, 0, 0, true), (12, 0, 0, true), (23, 59, 59, true)],
        )
    }

    #[test]
    fn time_window_with_equal_bounds_never_matches() -> TestResult {
        let never = [
            (0, 0, 0, false),
            (11, 59, 59, false),
            (12, 0, 0, false),
            (12, 0, 1, false),
            (23, 59, 59, false),
        ];
        assert_window(Some("12:00"), Some("12:00"), &never)?;
        // The same instant written with and without seconds.
        assert_window(Some("12:00"), Some("12:00:00"), &never)?;
        assert_window(Some("00:00"), Some("00:00"), &never)
    }

    #[test]
    fn time_window_same_day_accepts_times_with_and_without_seconds() -> TestResult {
        assert_window(
            Some("08:00"),
            Some("08:00:01"),
            &[(7, 59, 59, false), (8, 0, 0, true), (8, 0, 1, false)],
        )?;
        assert_window(
            Some("08:00:30"),
            Some("17:30"),
            &[
                (8, 0, 29, false),
                (8, 0, 30, true),
                (17, 29, 59, true),
                (17, 30, 0, false),
            ],
        )
    }

    #[test]
    fn time_window_crossing_midnight_accepts_times_with_and_without_seconds() -> TestResult {
        assert_window(
            Some("22:00:30"),
            Some("06:00"),
            &[
                (22, 0, 29, false),
                (22, 0, 30, true),
                (5, 59, 59, true),
                (6, 0, 0, false),
            ],
        )?;
        assert_window(
            Some("22:00"),
            Some("06:00:15"),
            &[
                (21, 59, 59, false),
                (22, 0, 0, true),
                (6, 0, 14, true),
                (6, 0, 15, false),
            ],
        )
    }

    #[test]
    fn time_window_rejects_a_bad_after() {
        for bad in ["nope", "", "22", "aa:bb", "22:xx", "-1:00"] {
            for now_sec in [0, 43_200, 86_399] {
                assert_eq!(
                    condition_error(time_window_contains(now_sec, Some(bad), None)),
                    Some(format!("bad after: {bad}")),
                    "after={bad:?} at second {now_sec}"
                );
            }
        }
        // `after` is checked first, so it wins when both bounds are bad.
        assert_eq!(
            condition_error(time_window_contains(0, Some("nope"), Some("nah"))),
            Some("bad after: nope".to_owned())
        );
    }

    #[test]
    fn time_window_rejects_a_bad_before() {
        for bad in ["nope", "", "06", "aa:bb", "06:xx", "-1:00"] {
            for now_sec in [0, 43_200, 86_399] {
                assert_eq!(
                    condition_error(time_window_contains(now_sec, None, Some(bad))),
                    Some(format!("bad before: {bad}")),
                    "before={bad:?} at second {now_sec}"
                );
            }
        }
    }

    /// Whether the window crosses midnight is only known once both bounds
    /// are read, so a bad `before` is an error at any time of day — also
    /// while `now` is still ahead of `after`.
    #[test]
    fn time_window_rejects_a_bad_before_at_any_time_of_day() {
        for now_sec in [0, 36_000, 79_199, 79_200, 86_399] {
            assert_eq!(
                condition_error(time_window_contains(now_sec, Some("22:00"), Some("nope"))),
                Some("bad before: nope".to_owned()),
                "at second {now_sec}"
            );
        }
    }

    /// Both windows cross midnight and together they cover the whole day
    /// (18:00–12:00 and 10:00–02:00), so one of them holds whenever this
    /// runs.
    #[test]
    fn time_condition_crossing_midnight_matches_through_evaluate() -> TestResult {
        let sm = empty_machine();
        let c = Condition::Or {
            conditions: vec![
                time_condition(Some("18:00"), Some("12:00")),
                time_condition(Some("10:00"), Some("02:00")),
            ],
        };
        assert!(c.evaluate(&sm)?);
        Ok(())
    }

    /// Results that do not depend on when the test runs.
    #[test]
    fn time_condition_fixed_results_through_evaluate() -> TestResult {
        let sm = empty_machine();
        assert!(time_condition(None, None).evaluate(&sm)?);
        assert!(time_condition(Some("00:00"), None).evaluate(&sm)?);
        assert!(!time_condition(None, Some("00:00")).evaluate(&sm)?);
        assert!(!time_condition(Some("12:00"), Some("12:00")).evaluate(&sm)?);
        // Two same-day windows that never overlap.
        let disjoint = Condition::And {
            conditions: vec![
                time_condition(Some("02:00"), Some("10:00")),
                time_condition(Some("12:00"), Some("18:00")),
            ],
        };
        assert!(!disjoint.evaluate(&sm)?);
        Ok(())
    }

    #[test]
    fn time_condition_reports_bad_bounds_through_evaluate() {
        let sm = empty_machine();
        assert_eq!(
            condition_error(time_condition(Some("nope"), None).evaluate(&sm)),
            Some("bad after: nope".to_owned())
        );
        assert_eq!(
            condition_error(time_condition(None, Some("nope")).evaluate(&sm)),
            Some("bad before: nope".to_owned())
        );
        assert_eq!(
            condition_error(time_condition(Some("00:00"), Some("nope")).evaluate(&sm)),
            Some("bad before: nope".to_owned())
        );
        assert_eq!(
            condition_error(time_condition(Some("nope"), Some("nah")).evaluate(&sm)),
            Some("bad after: nope".to_owned())
        );
    }

    #[test]
    fn time_condition_serialized_form_is_stable() -> TestResult {
        let night = time_condition(Some("22:00"), Some("06:00"));
        assert_eq!(
            serde_json::to_value(&night)?,
            serde_json::json!({"condition": "time", "after": "22:00", "before": "06:00"})
        );
        assert_eq!(
            serde_json::to_value(time_condition(None, None))?,
            serde_json::json!({"condition": "time", "after": null, "before": null})
        );
        let parsed: Condition =
            serde_json::from_value(serde_json::json!({"condition": "time", "after": "22:00"}))?;
        assert!(matches!(
            &parsed,
            Condition::Time { after: Some(a), before: None } if a == "22:00"
        ));
        Ok(())
    }
}
