// SPDX-License-Identifier: Apache-2.0
//! Auto-dashboard generation.
//!
//! Given a [`Home`] (rooms + entities), build a sensible default [`Dashboard`]
//! with zero configuration — the experience the headline persona gets the
//! moment they finish onboarding: one tab per room, each device rendered with
//! the right card kind for its domain, scenes surfaced as quick actions, and a
//! catch-all "Other" tab for anything not yet assigned to a room.
//!
//! Developer-only views (cluster topology, logs) are NOT auto-generated here —
//! they are added by the power-user surface and gated by
//! [`crate::dashboard::Dashboard::for_mode`].

use crate::area::{Domain, Home};
use crate::card::Card;
use crate::dashboard::{Dashboard, View};
use crate::label::{Lang, Phrase};

/// Build the default dashboard for a home.
///
/// Rules:
/// - One [`View`] per room that has at least one entity, in the home's room
///   order. The tab title is the room name; its icon is the room icon.
/// - Within a room, scenes are collected into a single Scene-strip ordering but
///   each device still gets the card kind [`Card::default_for`] picks for its
///   domain.
/// - Entities with no room land in a final "Other" view (localised) so nothing
///   is ever silently dropped.
/// - Every scene anywhere in the home is surfaced in the quick-actions strip.
/// - The first light/switch in each room is auto-pinned as a favourite, capped
///   so the strip stays glanceable.
#[must_use]
pub fn auto_dashboard(home: &Home, lang: Lang) -> Dashboard {
    /// Cap on auto-pinned favourites so the strip stays glanceable.
    const MAX_AUTO_FAVORITES: usize = 6;

    let mut dash = Dashboard::new();

    for area in home.areas() {
        let members = home.entities_in(&area.id);
        if members.is_empty() {
            continue;
        }
        let cards = members
            .iter()
            .map(|e| Card::default_for(e.domain, e.id.clone()))
            .collect();
        dash.push_view(View::new(area.name.clone(), area.icon.clone(), cards));
    }

    // Catch-all for unassigned devices.
    let orphans = home.unassigned();
    if !orphans.is_empty() {
        let cards = orphans
            .iter()
            .map(|e| Card::default_for(e.domain, e.id.clone()))
            .collect();
        // "Other" — localised, jargon-free.
        dash.push_view(View::new(other_title(lang), "dots", cards));
    }

    // Quick-action scenes: every scene in the home, in insertion order.
    for e in home.entities() {
        if e.domain == Domain::Scene {
            dash.scenes.push(e.id.clone());
        }
    }

    // Auto-favourite the first controllable light/switch per room (capped).
    for area in home.areas() {
        if dash.favorites.len() >= MAX_AUTO_FAVORITES {
            break;
        }
        if let Some(e) = home
            .entities_in(&area.id)
            .into_iter()
            .find(|e| matches!(e.domain, Domain::Light | Domain::Switch))
        {
            dash.add_favorite(e.id.clone());
        }
    }

    dash
}

/// The localised title for the catch-all view.
fn other_title(lang: Lang) -> String {
    match lang {
        Lang::En => "Other",
        Lang::De => "Sonstiges",
        Lang::Tr => "Diğer",
    }
    .to_string()
}

/// The phrase shown above the scenes strip, for the caller's header.
#[must_use]
pub const fn scenes_heading(lang: Lang) -> &'static str {
    Phrase::Scenes.text(lang)
}

/// The phrase shown above the favourites strip.
#[must_use]
pub const fn favorites_heading(lang: Lang) -> &'static str {
    Phrase::Favorites.text(lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::area::{Area, Entity};
    use crate::view_mode::{Surface, ViewMode};

    fn furnished_home() -> Home {
        let mut h = Home::new();
        h.add_area(Area::new("living", "Living room", "sofa"));
        h.add_area(Area::new("bed", "Bedroom", "bed"));
        h.add_area(Area::new("empty", "Spare room", "door")); // no devices
        h.add_entity(Entity::new("l1", "Ceiling light", Domain::Light, Some("living")));
        h.add_entity(Entity::new("cam", "Front cam", Domain::Camera, Some("living")));
        h.add_entity(Entity::new("th", "Thermostat", Domain::Climate, Some("living")));
        h.add_entity(Entity::new("cv", "Blinds", Domain::Cover, Some("living")));
        h.add_entity(Entity::new("temp", "Temperature", Domain::Sensor, Some("bed")));
        h.add_entity(Entity::new("bl", "Reading light", Domain::Light, Some("bed")));
        h.add_entity(Entity::new("ev", "Evening", Domain::Scene, Some("living")));
        h.add_entity(Entity::new("plug", "Garage plug", Domain::Switch, None::<String>));
        h
    }

    #[test]
    fn empty_home_yields_empty_dashboard() {
        let d = auto_dashboard(&Home::new(), Lang::En);
        assert!(d.views.is_empty());
        assert!(d.scenes.is_empty());
        assert!(d.favorites.is_empty());
    }

    #[test]
    fn one_view_per_non_empty_room_plus_other() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        // living + bed + Other (spare room is empty → skipped)
        let titles: Vec<_> = d.views.iter().map(|v| v.title.as_str()).collect();
        assert_eq!(titles, ["Living room", "Bedroom", "Other"]);
    }

    #[test]
    fn empty_room_is_skipped() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        assert!(!d.views.iter().any(|v| v.title == "Spare room"));
    }

    #[test]
    fn cards_match_domain() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        let living = d.views.iter().find(|v| v.title == "Living room").expect("living");
        // light → Light, camera → Camera, climate → Thermostat, cover → Cover, scene → Scene
        assert!(living.cards.iter().any(|c| matches!(c, Card::Light { .. })));
        assert!(living.cards.iter().any(|c| matches!(c, Card::Camera { .. })));
        assert!(living.cards.iter().any(|c| matches!(c, Card::Thermostat { .. })));
        assert!(living.cards.iter().any(|c| matches!(c, Card::Cover { .. })));
        assert!(living.cards.iter().any(|c| matches!(c, Card::Scene { .. })));

        let bed = d.views.iter().find(|v| v.title == "Bedroom").expect("bed");
        assert!(bed.cards.iter().any(|c| matches!(c, Card::SensorGraph { .. })));
    }

    #[test]
    fn unassigned_lands_in_other_view() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        let other = d.views.iter().find(|v| v.title == "Other").expect("other");
        assert_eq!(other.cards.len(), 1);
        assert!(matches!(other.cards[0], Card::Entity { .. })); // a switch
    }

    #[test]
    fn other_view_title_localised() {
        assert_eq!(other_title(Lang::De), "Sonstiges");
        assert_eq!(other_title(Lang::Tr), "Diğer");
        let d = auto_dashboard(&furnished_home(), Lang::De);
        assert!(d.views.iter().any(|v| v.title == "Sonstiges"));
    }

    #[test]
    fn scenes_are_surfaced_as_quick_actions() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        assert_eq!(d.scenes, vec!["ev".to_string()]);
    }

    #[test]
    fn first_light_or_switch_per_room_auto_favorited() {
        let d = auto_dashboard(&furnished_home(), Lang::En);
        // l1 (living's first light), bl (bedroom's light). The unassigned plug
        // is not in a room, so it is not auto-favourited.
        assert!(d.favorites.iter().any(|f| f.entity_id == "l1"));
        assert!(d.favorites.iter().any(|f| f.entity_id == "bl"));
        assert!(!d.favorites.iter().any(|f| f.entity_id == "plug"));
    }

    #[test]
    fn auto_dashboard_has_no_developer_content() {
        // The generator must never emit a power-user card/view.
        let d = auto_dashboard(&furnished_home(), Lang::En);
        assert!(!d.has_developer_content());
        // And it is unchanged by resident gating (proof there's nothing to strip).
        let resident = d.for_mode(ViewMode::resident(Surface::Portal));
        assert_eq!(resident.card_count(), d.card_count());
        assert_eq!(resident.views.len(), d.views.len());
    }

    #[test]
    fn headings_localised() {
        assert_eq!(scenes_heading(Lang::De), "Szenen");
        assert_eq!(favorites_heading(Lang::Tr), "Sık kullanılanlar");
    }
}
