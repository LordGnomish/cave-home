// SPDX-License-Identifier: Apache-2.0
//! Raw entity state → grandma-friendly tile view-model.
//!
//! The rest of cave-home reports a device as a raw `state` string plus a handful
//! of attributes (e.g. a light is `"on"` with `brightness=200`, a thermostat is
//! `"heat"` with `current_temperature=21.0`). The resident must never see any of
//! that. This module maps a [`crate::area::Entity`] + an [`EntityState`] to a
//! [`Tile`]: a friendly name, one short human status line ("On", "21°",
//! "Locked"), an icon, and the action(s) the resident can take — localised
//! EN / DE / TR.

use crate::area::{Domain, Entity};
use crate::label::{Lang, Phrase};

/// A raw device reading as the rest of cave-home reports it. Deliberately
/// loose (a string state + named numeric/string attributes) so any backend can
/// produce one without depending on the Portal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityState {
    /// The raw state token, e.g. `"on"`, `"off"`, `"locked"`, `"heat"`,
    /// `"open"`, or empty/`"unavailable"` when the device has not reported.
    pub state: String,
    /// Named numeric attributes (brightness 0..=255, temperature, position %, …).
    pub numbers: Vec<(String, f64)>,
    /// Named string attributes (unit of measurement, hvac action, …).
    pub strings: Vec<(String, String)>,
    /// Whether the device is currently reachable.
    pub available: bool,
}

impl EntityState {
    /// A reachable device with the given raw state and no attributes.
    #[must_use]
    pub fn on_off(state: &str) -> Self {
        Self {
            state: state.to_string(),
            numbers: Vec::new(),
            strings: Vec::new(),
            available: true,
        }
    }

    /// An unreachable device (renders the "Not responding" status).
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            state: String::new(),
            numbers: Vec::new(),
            strings: Vec::new(),
            available: false,
        }
    }

    /// Attach a numeric attribute (builder style).
    #[must_use]
    pub fn with_number(mut self, key: &str, value: f64) -> Self {
        self.numbers.push((key.to_string(), value));
        self
    }

    /// Attach a string attribute (builder style).
    #[must_use]
    pub fn with_string(mut self, key: &str, value: &str) -> Self {
        self.strings.push((key.to_string(), value.to_string()));
        self
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.numbers.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.strings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn is_truthy(&self) -> bool {
        matches!(
            self.state.to_ascii_lowercase().as_str(),
            "on" | "open" | "unlocked" | "playing" | "home" | "detected" | "true"
        )
    }
}

/// An action the resident can trigger from a tile. The string captions are
/// localised; the `kind` is what the (deferred) backend dispatches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// What the action does.
    pub kind: ActionKind,
}

/// The closed set of resident-facing actions a tile can offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Turn a light / switch / media player on.
    TurnOn,
    /// Turn it off.
    TurnOff,
    /// Open a cover.
    Open,
    /// Close a cover.
    Close,
    /// Lock a lock.
    Lock,
    /// Unlock a lock.
    Unlock,
    /// Run a scene.
    Run,
}

impl ActionKind {
    /// The localised button caption.
    #[must_use]
    pub const fn caption(self, lang: Lang) -> &'static str {
        match self {
            Self::TurnOn => Phrase::ActionTurnOn.text(lang),
            Self::TurnOff => Phrase::ActionTurnOff.text(lang),
            Self::Open => Phrase::ActionOpen.text(lang),
            Self::Close => Phrase::ActionClose.text(lang),
            Self::Lock => Phrase::ActionLock.text(lang),
            Self::Unlock => Phrase::ActionUnlock.text(lang),
            Self::Run => Phrase::ActionRun.text(lang),
        }
    }
}

/// The grandma-friendly tile: everything needed to render one device, with no
/// implementation detail. Built by [`tile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    /// Friendly device name ("Ceiling light").
    pub name: String,
    /// One short human status line ("On", "21°", "Locked", "Not responding").
    pub status: String,
    /// Logical icon name the frontend maps to a glyph.
    pub icon: String,
    /// Whether the tile should render as "active" (light on, door open, …).
    pub active: bool,
    /// The actions the resident may take, in display order.
    pub actions: Vec<Action>,
}

/// Map a raw `(entity, state)` pair to a localised [`Tile`].
///
/// This is the function the (deferred) live-state stream feeds, once per update.
/// It is total: an unknown state or a missing attribute degrades to a safe,
/// jargon-free status rather than erroring.
#[must_use]
pub fn tile(entity: &Entity, state: &EntityState, lang: Lang) -> Tile {
    if !state.available {
        return Tile {
            name: entity.name.clone(),
            status: Phrase::Unavailable.text(lang).to_string(),
            icon: icon_for(entity.domain),
            active: false,
            actions: Vec::new(),
        };
    }

    let (status, active, actions) = render_domain(entity.domain, state, lang);
    Tile {
        name: entity.name.clone(),
        status,
        icon: icon_for(entity.domain),
        active,
        actions,
    }
}

/// Compute the `(status, active, actions)` triple for a reachable device of a
/// given domain. Split out of [`tile`] to keep each piece readable.
fn render_domain(domain: Domain, state: &EntityState, lang: Lang) -> (String, bool, Vec<Action>) {
    match domain {
        Domain::Light | Domain::Switch | Domain::MediaPlayer => {
            let on = state.is_truthy();
            let status = if on {
                Phrase::On.text(lang).to_string()
            } else {
                Phrase::Off.text(lang).to_string()
            };
            let action = if on {
                ActionKind::TurnOff
            } else {
                ActionKind::TurnOn
            };
            (status, on, vec![Action { kind: action }])
        }
        Domain::Lock => {
            let locked = state.state.eq_ignore_ascii_case("locked");
            let status = if locked {
                Phrase::Locked.text(lang).to_string()
            } else {
                Phrase::Unlocked.text(lang).to_string()
            };
            let action = if locked {
                ActionKind::Unlock
            } else {
                ActionKind::Lock
            };
            // A lock counts as "active" (drawing attention) when it is *open*.
            (status, !locked, vec![Action { kind: action }])
        }
        Domain::Cover => {
            // Prefer an explicit position attribute (0 closed .. 100 open).
            let open = state
                .number("position")
                .map_or_else(|| state.is_truthy(), |p| p > 0.0);
            let status = if open {
                Phrase::Open.text(lang).to_string()
            } else {
                Phrase::Closed.text(lang).to_string()
            };
            let action = if open {
                ActionKind::Close
            } else {
                ActionKind::Open
            };
            (status, open, vec![Action { kind: action }])
        }
        Domain::Climate => {
            // Show the current room temperature, not the mode token.
            let status = state.number("current_temperature").map_or_else(
                || Phrase::Unavailable.text(lang).to_string(),
                format_temperature,
            );
            let heating = !state.state.eq_ignore_ascii_case("off") && !state.state.is_empty();
            (status, heating, Vec::new())
        }
        Domain::Sensor => {
            // A sensor shows its value + unit, e.g. "62 %", "415 W".
            let status = match state.number("value") {
                Some(v) => state
                    .text("unit")
                    .map_or_else(|| trim_float(v), |u| format!("{} {u}", trim_float(v))),
                None if !state.state.is_empty() => state.state.clone(),
                None => Phrase::Unavailable.text(lang).to_string(),
            };
            (status, false, Vec::new())
        }
        Domain::BinarySensor => {
            let detected = state.is_truthy();
            let status = if detected {
                Phrase::Open.text(lang).to_string()
            } else {
                Phrase::Closed.text(lang).to_string()
            };
            (status, detected, Vec::new())
        }
        Domain::Camera => {
            // Cameras render a live thumbnail; the status line is just "On".
            (Phrase::On.text(lang).to_string(), true, Vec::new())
        }
        Domain::Scene => (
            String::new(),
            false,
            vec![Action {
                kind: ActionKind::Run,
            }],
        ),
    }
}

/// Format a temperature for a tile: rounded to the nearest degree with the
/// degree sign, e.g. `21.4 -> "21°"`. No unit jargon, no decimal noise.
fn format_temperature(celsius: f64) -> String {
    if !celsius.is_finite() {
        return String::new();
    }
    // `{:.0}` rounds to the nearest integer without a fragile float→int cast.
    format!("{celsius:.0}°")
}

/// Render a float without a trailing `.0` ("62" not "62.0"), keeping one
/// decimal otherwise ("4.2").
fn trim_float(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if (v.fract()).abs() < f64::EPSILON {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// The logical icon name for a domain, as an owned string (frontend-agnostic).
#[must_use]
pub fn icon_for(domain: Domain) -> String {
    icon_name(domain).to_string()
}

/// The logical icon name for a domain. Frontend maps this to a glyph.
#[must_use]
pub const fn icon_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Light => "lightbulb",
        Domain::Switch => "power-plug",
        Domain::Lock => "lock",
        Domain::Cover => "blinds",
        Domain::Climate => "thermostat",
        Domain::Camera => "camera",
        Domain::Sensor => "gauge",
        Domain::BinarySensor => "motion-sensor",
        Domain::Scene => "palette",
        Domain::MediaPlayer => "speaker",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label::BANNED_JARGON;

    fn ent(domain: Domain) -> Entity {
        Entity::new("e", "Device", domain, Some("living"))
    }

    #[test]
    fn light_on_off_maps_to_human_status_and_toggle_action() {
        let e = Entity::new("l", "Ceiling light", Domain::Light, Some("living"));
        let on = tile(&e, &EntityState::on_off("on"), Lang::En);
        assert_eq!(on.name, "Ceiling light");
        assert_eq!(on.status, "On");
        assert!(on.active);
        assert_eq!(
            on.actions,
            vec![Action {
                kind: ActionKind::TurnOff
            }]
        );

        let off = tile(&e, &EntityState::on_off("off"), Lang::En);
        assert_eq!(off.status, "Off");
        assert!(!off.active);
        assert_eq!(
            off.actions,
            vec![Action {
                kind: ActionKind::TurnOn
            }]
        );
    }

    #[test]
    fn lock_state_text_localised() {
        let e = ent(Domain::Lock);
        assert_eq!(
            tile(&e, &EntityState::on_off("locked"), Lang::En).status,
            "Locked"
        );
        assert_eq!(
            tile(&e, &EntityState::on_off("locked"), Lang::De).status,
            "Verriegelt"
        );
        assert_eq!(
            tile(&e, &EntityState::on_off("locked"), Lang::Tr).status,
            "Kilitli"
        );
        let unlocked = tile(&e, &EntityState::on_off("unlocked"), Lang::En);
        assert_eq!(unlocked.status, "Unlocked");
        assert_eq!(unlocked.actions[0].kind, ActionKind::Lock);
    }

    #[test]
    fn climate_shows_rounded_temperature_not_mode() {
        let e = ent(Domain::Climate);
        let s = EntityState::on_off("heat").with_number("current_temperature", 21.4);
        let t = tile(&e, &s, Lang::En);
        assert_eq!(t.status, "21°");
        assert!(t.active, "heating mode marks the tile active");
        // The raw mode token never leaks into the status line.
        assert!(!t.status.contains("heat"));
    }

    #[test]
    fn cover_uses_position_attribute() {
        let e = ent(Domain::Cover);
        let open = tile(
            &e,
            &EntityState::on_off("open").with_number("position", 80.0),
            Lang::En,
        );
        assert_eq!(open.status, "Open");
        assert!(open.active);
        assert_eq!(open.actions[0].kind, ActionKind::Close);

        let closed = tile(
            &e,
            &EntityState::on_off("open").with_number("position", 0.0),
            Lang::En,
        );
        assert_eq!(closed.status, "Closed");
        assert_eq!(closed.actions[0].kind, ActionKind::Open);
    }

    #[test]
    fn cover_falls_back_to_state_when_no_position() {
        let e = ent(Domain::Cover);
        assert_eq!(
            tile(&e, &EntityState::on_off("open"), Lang::En).status,
            "Open"
        );
        assert_eq!(
            tile(&e, &EntityState::on_off("closed"), Lang::En).status,
            "Closed"
        );
    }

    #[test]
    fn sensor_shows_value_with_unit() {
        let e = ent(Domain::Sensor);
        let humidity = EntityState::on_off("")
            .with_number("value", 62.0)
            .with_string("unit", "%");
        assert_eq!(tile(&e, &humidity, Lang::En).status, "62 %");
        let power = EntityState::on_off("")
            .with_number("value", 4.2)
            .with_string("unit", "W");
        assert_eq!(tile(&e, &power, Lang::En).status, "4.2 W");
    }

    #[test]
    fn unavailable_device_renders_friendly_status_no_actions() {
        let e = ent(Domain::Light);
        let t = tile(&e, &EntityState::unavailable(), Lang::En);
        assert_eq!(t.status, "Not responding");
        assert!(t.actions.is_empty());
        assert_eq!(
            tile(&e, &EntityState::unavailable(), Lang::De).status,
            "Keine Antwort"
        );
        assert_eq!(
            tile(&e, &EntityState::unavailable(), Lang::Tr).status,
            "Yanıt vermiyor"
        );
    }

    #[test]
    fn scene_offers_only_a_run_action() {
        let e = ent(Domain::Scene);
        let t = tile(&e, &EntityState::on_off("idle"), Lang::Tr);
        assert_eq!(
            t.actions,
            vec![Action {
                kind: ActionKind::Run
            }]
        );
        assert_eq!(t.actions[0].kind.caption(Lang::Tr), "Başlat");
    }

    #[test]
    fn action_captions_localised() {
        assert_eq!(ActionKind::TurnOn.caption(Lang::De), "Einschalten");
        assert_eq!(ActionKind::Lock.caption(Lang::Tr), "Kilitle");
        assert_eq!(ActionKind::Open.caption(Lang::En), "Open");
    }

    #[test]
    fn each_domain_has_a_non_empty_icon() {
        for d in Domain::ALL {
            assert!(!icon_for(d).is_empty(), "{d:?} has no icon");
        }
    }

    #[test]
    fn no_tile_status_or_icon_leaks_jargon() {
        // Exercise every domain across every state/language; assert the
        // resident-facing strings are jargon-free (Charter §6.3).
        let states = [
            EntityState::on_off("on"),
            EntityState::on_off("off"),
            EntityState::on_off("locked"),
            EntityState::on_off("open"),
            EntityState::unavailable(),
            EntityState::on_off("heat").with_number("current_temperature", 19.0),
            EntityState::on_off("")
                .with_number("value", 7.0)
                .with_string("unit", "W"),
        ];
        for d in Domain::ALL {
            let e = ent(d);
            for lang in Lang::ALL {
                for s in &states {
                    let t = tile(&e, s, lang);
                    let bag = format!("{} {} {}", t.name, t.status, t.icon);
                    for banned in BANNED_JARGON {
                        assert!(
                            !bag.contains(banned),
                            "{d:?}/{lang:?} leaks {banned:?}: {bag}"
                        );
                    }
                }
            }
        }
    }
}
