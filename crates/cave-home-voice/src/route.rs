//! Intent routing — turn a matched intent into a typed action other crates run.
//!
//! The matcher tells us *which* intent fired and *what slots* it captured. The
//! router turns that into a [`IntentAction`] — a small, typed command the rest
//! of cave-home (lights, climate, covers, scenes) executes. This crate does not
//! execute anything itself (that wiring is Phase-1b, see the parity manifest);
//! it produces the typed intent so the boundary stays clean and testable.

use crate::matcher::IntentMatch;
use crate::slot::SlotValue;

/// A typed command produced from a recognised voice intent. Other cave-home
/// crates pattern-match on this to act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentAction {
    /// Switch a light (or a room's lights) on or off.
    SetLight {
        /// Canonical device or room name (e.g. `"living room"`).
        target: String,
        /// `true` = on, `false` = off.
        on: bool,
    },
    /// Set a light's brightness as a percentage `0..=100`.
    SetBrightness {
        /// Canonical device or room name.
        target: String,
        /// Brightness percent.
        percent: u32,
    },
    /// Set a room's target temperature in whole degrees Celsius.
    SetTemperature {
        /// Canonical room name.
        target: String,
        /// Target temperature, °C.
        celsius: u32,
    },
    /// Open or close a cover (blind / curtain / garage).
    SetCover {
        /// Canonical cover name.
        target: String,
        /// `true` = open, `false` = close.
        open: bool,
    },
    /// Activate a named scene (e.g. `"movie night"`).
    ActivateScene {
        /// Scene name.
        name: String,
    },
    /// Read back the current state of something the household asked about.
    QueryState {
        /// Canonical name of what was asked about.
        target: String,
        /// What aspect was asked about.
        what: QueryKind,
    },
}

/// What a [`IntentAction::QueryState`] is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    /// "What's the … temperature?"
    Temperature,
    /// "Is the … on?"
    OnState,
}

/// Why a matched intent could not be routed to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// The intent id is not one this router knows how to action.
    UnknownIntent(String),
    /// A slot the action needs was missing or the wrong shape.
    MissingSlot {
        /// The intent id being routed.
        intent: String,
        /// The slot that was missing or malformed.
        slot: &'static str,
    },
}

/// A routed action plus the confidence carried over from the match.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedAction {
    /// The typed command to execute.
    pub action: IntentAction,
    /// Confidence carried from the [`IntentMatch`].
    pub confidence: f32,
}

/// Route a successful [`IntentMatch`] to a typed [`RoutedAction`].
///
/// The intent ids are the built-in set defined in [`crate::intents`]
/// (`light.on`, `light.off`, `light.brightness`, `climate.set`, `cover.open`,
/// `cover.close`, `scene.activate`, `query.temperature`, `query.on_state`).
///
/// # Errors
///
/// Returns [`RouteError`] when the intent id is unknown to the router or a
/// required slot is absent / has the wrong type.
pub fn route(m: &IntentMatch) -> Result<RoutedAction, RouteError> {
    let action = match m.intent.as_str() {
        "light.on" => IntentAction::SetLight {
            target: text(m, "name")?,
            on: true,
        },
        "light.off" => IntentAction::SetLight {
            target: text(m, "name")?,
            on: false,
        },
        "light.brightness" => IntentAction::SetBrightness {
            target: text(m, "name")?,
            percent: number(m, "level")?,
        },
        "climate.set" => IntentAction::SetTemperature {
            target: text(m, "name")?,
            celsius: number(m, "level")?,
        },
        "cover.open" => IntentAction::SetCover {
            target: text(m, "name")?,
            open: true,
        },
        "cover.close" => IntentAction::SetCover {
            target: text(m, "name")?,
            open: false,
        },
        "scene.activate" => IntentAction::ActivateScene {
            name: text(m, "name")?,
        },
        "query.temperature" => IntentAction::QueryState {
            target: text(m, "name")?,
            what: QueryKind::Temperature,
        },
        "query.on_state" => IntentAction::QueryState {
            target: text(m, "name")?,
            what: QueryKind::OnState,
        },
        other => return Err(RouteError::UnknownIntent(other.to_string())),
    };
    Ok(RoutedAction {
        action,
        confidence: m.confidence,
    })
}

fn text(m: &IntentMatch, slot: &'static str) -> Result<String, RouteError> {
    match m.slots.get(slot) {
        Some(SlotValue::Text(t)) => Ok(t.clone()),
        Some(SlotValue::Number(n)) => Ok(n.to_string()),
        None => Err(RouteError::MissingSlot {
            intent: m.intent.clone(),
            slot,
        }),
    }
}

fn number(m: &IntentMatch, slot: &'static str) -> Result<u32, RouteError> {
    match m.slots.get(slot) {
        Some(SlotValue::Number(n)) => Ok(*n),
        _ => Err(RouteError::MissingSlot {
            intent: m.intent.clone(),
            slot,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label::Lang;
    use std::collections::BTreeMap;

    fn m(intent: &str, slots: &[(&str, SlotValue)]) -> IntentMatch {
        IntentMatch {
            intent: intent.to_string(),
            lang: Lang::En,
            slots: slots
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            confidence: 0.9,
        }
    }

    #[test]
    fn routes_light_on_off() {
        let r = route(&m("light.on", &[("name", SlotValue::Text("kitchen".into()))])).expect("r");
        assert_eq!(
            r.action,
            IntentAction::SetLight {
                target: "kitchen".into(),
                on: true
            }
        );
        let r = route(&m("light.off", &[("name", SlotValue::Text("kitchen".into()))])).expect("r");
        assert!(matches!(r.action, IntentAction::SetLight { on: false, .. }));
    }

    #[test]
    fn routes_brightness_and_temperature() {
        let r = route(&m(
            "light.brightness",
            &[
                ("name", SlotValue::Text("bedroom".into())),
                ("level", SlotValue::Number(40)),
            ],
        ))
        .expect("r");
        assert_eq!(
            r.action,
            IntentAction::SetBrightness {
                target: "bedroom".into(),
                percent: 40
            }
        );
        let r = route(&m(
            "climate.set",
            &[
                ("name", SlotValue::Text("living room".into())),
                ("level", SlotValue::Number(21)),
            ],
        ))
        .expect("r");
        assert_eq!(
            r.action,
            IntentAction::SetTemperature {
                target: "living room".into(),
                celsius: 21
            }
        );
    }

    #[test]
    fn routes_cover_and_scene_and_queries() {
        assert!(matches!(
            route(&m("cover.open", &[("name", SlotValue::Text("blinds".into()))]))
                .expect("r")
                .action,
            IntentAction::SetCover { open: true, .. }
        ));
        assert!(matches!(
            route(&m("scene.activate", &[("name", SlotValue::Text("movie night".into()))]))
                .expect("r")
                .action,
            IntentAction::ActivateScene { .. }
        ));
        assert_eq!(
            route(&m("query.temperature", &[("name", SlotValue::Text("bedroom".into()))]))
                .expect("r")
                .action,
            IntentAction::QueryState {
                target: "bedroom".into(),
                what: QueryKind::Temperature
            }
        );
    }

    #[test]
    fn unknown_intent_errors() {
        assert_eq!(
            route(&m("does.not.exist", &[])),
            Err(RouteError::UnknownIntent("does.not.exist".into()))
        );
    }

    #[test]
    fn missing_slot_errors() {
        let bad = IntentMatch {
            intent: "light.on".into(),
            lang: Lang::En,
            slots: BTreeMap::new(),
            confidence: 0.9,
        };
        assert_eq!(
            route(&bad),
            Err(RouteError::MissingSlot {
                intent: "light.on".into(),
                slot: "name"
            })
        );
    }

    #[test]
    fn brightness_requires_numeric_level() {
        let bad = m(
            "light.brightness",
            &[
                ("name", SlotValue::Text("bedroom".into())),
                ("level", SlotValue::Text("loud".into())),
            ],
        );
        assert!(matches!(route(&bad), Err(RouteError::MissingSlot { .. })));
    }

    #[test]
    fn confidence_is_carried_through() {
        let r = route(&m("light.on", &[("name", SlotValue::Text("kitchen".into()))])).expect("r");
        assert!((r.confidence - 0.9).abs() < f32::EPSILON);
    }
}
