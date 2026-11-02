// SPDX-License-Identifier: Apache-2.0
//! The dashboard structure: [`Dashboard`] → [`View`] (tabs) → [`Card`]s, plus
//! the favourites and scenes/quick-action surfaces.
//!
//! The key behaviour here is **developer-gating**: [`Dashboard::for_mode`]
//! returns a copy with every developer-only card and every developer-only view
//! structurally removed when the [`ViewMode`] is Resident or the surface is
//! mobile (Charter §6.3). Resident output never *contains* power-user content —
//! it is not merely hidden.

use crate::card::Card;
use crate::view_mode::ViewMode;

/// One tab of the dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Friendly tab title ("Living room", "All", "Cluster").
    pub title: String,
    /// Logical icon name for the tab.
    pub icon: String,
    /// The cards on this tab, in order.
    pub cards: Vec<Card>,
    /// Whether the whole tab is a power-user page (Charter §6.3).
    pub developer_only: bool,
}

impl View {
    /// A resident-facing view.
    #[must_use]
    pub fn new(title: impl Into<String>, icon: impl Into<String>, cards: Vec<Card>) -> Self {
        Self {
            title: title.into(),
            icon: icon.into(),
            cards,
            developer_only: false,
        }
    }

    /// A power-user-only view (cluster topology, logs, …).
    #[must_use]
    pub fn developer(title: impl Into<String>, icon: impl Into<String>, cards: Vec<Card>) -> Self {
        Self {
            title: title.into(),
            icon: icon.into(),
            cards,
            developer_only: true,
        }
    }
}

/// A favourite: a pinned entity the resident reaches in one tap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    /// The entity id (plumbing; the tile supplies the friendly name).
    pub entity_id: String,
}

/// The whole dashboard: ordered tabs, a favourites strip, and a scenes strip.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dashboard {
    /// The tabs, in order.
    pub views: Vec<View>,
    /// Pinned favourites, in display order.
    pub favorites: Vec<Favorite>,
    /// Scene entity ids surfaced as quick actions, in display order.
    pub scenes: Vec<String>,
}

impl Dashboard {
    /// An empty dashboard.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tab (builder style).
    pub fn push_view(&mut self, view: View) -> &mut Self {
        self.views.push(view);
        self
    }

    /// Pin a favourite if not already pinned (idempotent).
    pub fn add_favorite(&mut self, entity_id: impl Into<String>) -> &mut Self {
        let id = entity_id.into();
        if !self.favorites.iter().any(|f| f.entity_id == id) {
            self.favorites.push(Favorite { entity_id: id });
        }
        self
    }

    /// Remove a favourite.
    pub fn remove_favorite(&mut self, entity_id: &str) -> &mut Self {
        self.favorites.retain(|f| f.entity_id != entity_id);
        self
    }

    /// Total card count across every view (including developer cards).
    #[must_use]
    pub fn card_count(&self) -> usize {
        self.views.iter().map(|v| v.cards.len()).sum()
    }

    /// Produce the dashboard as it should be rendered for `mode`.
    ///
    /// In Resident mode (or on any mobile surface) this drops every
    /// developer-only view and every developer-only card from the views that
    /// remain. The favourites and scenes strips are resident surfaces and are
    /// preserved unchanged. This is the Charter §6.3 gate.
    #[must_use]
    pub fn for_mode(&self, mode: ViewMode) -> Self {
        if mode.shows_developer_content() {
            return self.clone();
        }
        let views = self
            .views
            .iter()
            .filter(|v| !v.developer_only)
            .map(|v| View {
                title: v.title.clone(),
                icon: v.icon.clone(),
                cards: v
                    .cards
                    .iter()
                    .filter(|c| !c.is_developer_only())
                    .cloned()
                    .collect(),
                developer_only: false,
            })
            .collect();
        Self {
            views,
            favorites: self.favorites.clone(),
            scenes: self.scenes.clone(),
        }
    }

    /// `true` if any view or card in this dashboard is developer-only. Used by
    /// the gating test to prove resident output is clean.
    #[must_use]
    pub fn has_developer_content(&self) -> bool {
        self.views
            .iter()
            .any(|v| v.developer_only || v.cards.iter().any(Card::is_developer_only))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_mode::{Surface, ViewMode};

    fn dash_with_dev() -> Dashboard {
        let mut d = Dashboard::new();
        d.push_view(View::new(
            "Living room",
            "sofa",
            vec![
                Card::Light {
                    entity_id: "l1".into(),
                },
                Card::RawEntity {
                    entity_id: "l1".into(),
                }, // dev card mixed in
            ],
        ));
        d.push_view(View::developer(
            "Cluster",
            "server",
            vec![Card::ClusterTopology],
        ));
        d.add_favorite("l1");
        d.scenes.push("scene.evening".into());
        d
    }

    #[test]
    fn resident_mode_strips_all_developer_content() {
        let d = dash_with_dev();
        assert!(d.has_developer_content());

        let resident = d.for_mode(ViewMode::resident(Surface::Portal));
        assert!(
            !resident.has_developer_content(),
            "resident dashboard must contain NO developer content"
        );
        // The developer view is gone entirely.
        assert_eq!(resident.views.len(), 1);
        assert_eq!(resident.views[0].title, "Living room");
        // The developer card inside the kept view is gone too.
        assert_eq!(resident.views[0].cards.len(), 1);
        assert!(matches!(resident.views[0].cards[0], Card::Light { .. }));
    }

    #[test]
    fn mobile_strips_developer_content_even_when_flag_set() {
        let d = dash_with_dev();
        let mobile_dev = d.for_mode(ViewMode::developer(Surface::Mobile));
        assert!(
            !mobile_dev.has_developer_content(),
            "mobile must never show developer content"
        );
        assert_eq!(mobile_dev.views.len(), 1);
    }

    #[test]
    fn developer_on_portal_keeps_everything() {
        let d = dash_with_dev();
        let dev = d.for_mode(ViewMode::developer(Surface::Portal));
        assert!(dev.has_developer_content());
        assert_eq!(dev.views.len(), 2);
        assert_eq!(dev, d, "developer view is the unmodified dashboard");
    }

    #[test]
    fn favorites_and_scenes_survive_gating() {
        let d = dash_with_dev();
        let resident = d.for_mode(ViewMode::resident(Surface::Portal));
        assert_eq!(resident.favorites.len(), 1);
        assert_eq!(resident.scenes, vec!["scene.evening".to_string()]);
    }

    #[test]
    fn favorites_are_idempotent_and_removable() {
        let mut d = Dashboard::new();
        d.add_favorite("a").add_favorite("a").add_favorite("b");
        assert_eq!(d.favorites.len(), 2);
        d.remove_favorite("a");
        assert_eq!(d.favorites.len(), 1);
        assert_eq!(d.favorites[0].entity_id, "b");
    }

    #[test]
    fn card_count_sums_views() {
        let d = dash_with_dev();
        assert_eq!(d.card_count(), 3); // 2 in living + 1 cluster
    }
}
