// SPDX-License-Identifier: Apache-2.0
//! `cave-home-portal` — the Portal dashboard view-model + layout engine
//! (Charter §3 Lovelace-class dashboard; §6.3 grandma-friendly UX).
//!
//! This crate is the **pure UI model** behind the household dashboard: the
//! navigation tree, the typed card model, the auto-dashboard generator, the
//! raw-state → friendly-tile mapping, the Developer-view gate, and the theme /
//! responsive model. It is std-only and depends on no other cave-home crate or
//! network primitive, so it can be unit-tested in isolation and reused by both
//! the Portal web app and the mobile companion.
//!
//! # What lives here (Phase 1 MVP, real + tested)
//! - [`area`] — Home → Areas (rooms) → Entities, the grandma-friendly
//!   navigation tree ("rooms over hierarchies", `docs/ui-language.md`).
//! - [`card`] — the typed Lovelace-class card model, including the
//!   developer-only card kinds.
//! - [`dashboard`] — [`dashboard::Dashboard`] / [`dashboard::View`], favourites
//!   and scenes, and the Developer-view gate ([`dashboard::Dashboard::for_mode`]).
//! - [`autogen`] — zero-config default dashboard generation from a [`area::Home`].
//! - [`viewmodel`] — raw entity state → a friendly, localised [`viewmodel::Tile`].
//! - [`view_mode`] — the Resident vs Developer toggle (default Resident; hidden
//!   on mobile), Charter §6.3.
//! - [`theme`] — theme/branding + responsive breakpoint → column hints.
//! - [`label`] — the EN/DE/TR UI vocabulary, verified jargon-free.
//!
//! # What is deferred (see `parity.manifest.toml` `[[unmapped]]`)
//! The HTTP/WebSocket server, the REST API, the web frontend assets (HTML / JS /
//! WASM), live state streaming, auth/session, the drag-and-drop editor's
//! persistence, and the `cave-home-core` entity-state subscription are all
//! network- or frontend-bound and land in Phase 1b. This crate is the model they
//! all sit on top of.
//!
//! # Example: a zero-config dashboard, gated for a resident
//!
//! ```
//! use cave_home_portal::area::{Area, Domain, Entity, Home};
//! use cave_home_portal::autogen::auto_dashboard;
//! use cave_home_portal::label::Lang;
//! use cave_home_portal::view_mode::{Surface, ViewMode};
//!
//! let mut home = Home::new();
//! home.add_area(Area::new("living", "Living room", "sofa"));
//! home.add_entity(Entity::new("l1", "Ceiling light", Domain::Light, Some("living")));
//! home.add_entity(Entity::new("ev", "Evening", Domain::Scene, Some("living")));
//!
//! let dashboard = auto_dashboard(&home, Lang::En);
//! assert_eq!(dashboard.views.len(), 1);
//! assert_eq!(dashboard.views[0].title, "Living room");
//! assert_eq!(dashboard.scenes, vec!["ev".to_string()]);
//!
//! // Residents (the default) never see developer content.
//! let resident = dashboard.for_mode(ViewMode::resident(Surface::Portal));
//! assert!(!resident.has_developer_content());
//! ```

#![forbid(unsafe_code)]

pub mod area;
pub mod autogen;
pub mod card;
pub mod cluster;
pub mod dashboard;
pub mod energy;
pub mod jarvis;
pub mod label;
pub mod theme;
pub mod view_mode;
pub mod viewmodel;

pub use area::{Area, Domain, Entity, Home};
pub use autogen::auto_dashboard;
pub use card::Card;
pub use dashboard::{Dashboard, Favorite, View};
pub use energy::{
    BackupToggle, EnergyFlowView, EnergyMode, EnergyPage, FlowEdge, FlowNode, HistoryGraph,
    ModeOption, SocBar,
};
pub use jarvis::{Interaction, JarvisPage, MicStatus, UnderstoodBy};
pub use label::{Lang, Phrase};
pub use theme::{Breakpoint, Mode, Theme, ThemeError};
pub use view_mode::{Surface, ViewMode};
pub use viewmodel::{tile, Action, ActionKind, EntityState, Tile};
