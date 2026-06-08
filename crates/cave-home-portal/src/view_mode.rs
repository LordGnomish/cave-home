// SPDX-License-Identifier: Apache-2.0
//! The Developer-view toggle (Charter §6.3, ADR-007, `docs/ui-language.md`).
//!
//! The headline persona never sees power-user surfaces. Power users get an
//! escape hatch via a Settings toggle that is **off by default** and **never
//! exposed on the mobile app**. Developer mode *adds* pages (cluster topology,
//! logs, raw device inspector); it never relabels the home-world UI.
//!
//! This module is the gate: given a [`ViewMode`] and a [`Surface`], it decides
//! whether a developer-only card or page is allowed to render. The
//! [`crate::dashboard`] layout engine consults it so developer content is
//! structurally absent — not merely hidden with CSS — from resident output.

/// Who is looking, and from where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewMode {
    /// Whether the power-user surface is enabled.
    pub developer: bool,
    /// The client surface. The mobile app never shows the toggle nor any
    /// developer content, regardless of the stored preference.
    pub surface: Surface,
}

/// The client a dashboard is being rendered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The Portal web app (the only place Developer view can be enabled).
    Portal,
    /// The mobile companion app (Developer view is unavailable here).
    Mobile,
}

impl Default for ViewMode {
    /// The mandated default: Resident, on the Portal. Charter §6.3.
    fn default() -> Self {
        Self {
            developer: false,
            surface: Surface::Portal,
        }
    }
}

impl ViewMode {
    /// A resident (non-developer) on the given surface.
    #[must_use]
    pub const fn resident(surface: Surface) -> Self {
        Self {
            developer: false,
            surface,
        }
    }

    /// A developer on the Portal. (Construct-able only with `Surface::Portal`
    /// to keep the invariant clear; passing Mobile here still gates everything
    /// off — see [`ViewMode::shows_developer_content`].)
    #[must_use]
    pub const fn developer(surface: Surface) -> Self {
        Self {
            developer: true,
            surface,
        }
    }

    /// Whether developer-only cards and pages may render *at all*.
    ///
    /// This is the single source of truth. It is `true` only when the toggle is
    /// on **and** we are on the Portal — the mobile app can never show developer
    /// content even if the stored preference says developer.
    #[must_use]
    pub const fn shows_developer_content(self) -> bool {
        self.developer && matches!(self.surface, Surface::Portal)
    }

    /// Whether the Settings screen should even render the Developer-view toggle.
    /// Per `docs/ui-language.md`, the mobile app does not expose it.
    #[must_use]
    pub const fn shows_developer_toggle(self) -> bool {
        matches!(self.surface, Surface::Portal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_resident_on_portal() {
        let vm = ViewMode::default();
        assert!(!vm.developer);
        assert_eq!(vm.surface, Surface::Portal);
        assert!(!vm.shows_developer_content());
    }

    #[test]
    fn resident_never_sees_developer_content() {
        assert!(!ViewMode::resident(Surface::Portal).shows_developer_content());
        assert!(!ViewMode::resident(Surface::Mobile).shows_developer_content());
    }

    #[test]
    fn developer_content_only_on_portal() {
        assert!(ViewMode::developer(Surface::Portal).shows_developer_content());
        // Even with the developer flag set, mobile gates it off entirely.
        assert!(!ViewMode::developer(Surface::Mobile).shows_developer_content());
    }

    #[test]
    fn mobile_never_shows_the_toggle() {
        assert!(ViewMode::resident(Surface::Portal).shows_developer_toggle());
        assert!(ViewMode::developer(Surface::Portal).shows_developer_toggle());
        assert!(!ViewMode::resident(Surface::Mobile).shows_developer_toggle());
        assert!(!ViewMode::developer(Surface::Mobile).shows_developer_toggle());
    }
}
