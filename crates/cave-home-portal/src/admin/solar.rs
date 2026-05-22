// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Portal hooks for the Solar Tier 1 admin surface.
//!
//! Three pages, all served under `/admin/solar/*`:
//!
//! * `/admin/solar/evcc/*`     — EVCC loadpoints, planner, surplus loop
//! * `/admin/solar/sunspec/*`  — SunSpec inverter & battery readings
//! * `/admin/solar/forecast/*` — Forecast.Solar + PVGIS forecasts
//!
//! Phase 2b mounts real Lovelace-class panels here; for Phase 2 this
//! module exposes the route table that the Portal router consumes
//! (every page also has a paired CLI sub-command in
//! `cave-home-cli::commands::solar`).
//!
//! Charter §6.3 grandma-friendly UX is enforced by the
//! [`HOME_WORD_LABELS`] table: each route slot publishes a
//! home-world label that the dashboard renders. Raw kW / Modbus
//! registers / SunSpec model IDs are surfaced only under the
//! Developer-view sub-routes (`*/raw`).

/// One entry in the admin route table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolarAdminRoute {
    pub path: &'static str,
    /// Grandma-friendly label rendered in the navigation drawer.
    pub home_word_label: &'static str,
    /// `true` if this route is only visible with Developer view ON
    /// (Settings → "Developer view" toggle, off by default per
    /// ADR-007).
    pub developer_view_only: bool,
}

/// All routes the solar admin surface exposes.
pub const SOLAR_ADMIN_ROUTES: &[SolarAdminRoute] = &[
    // EVCC track ---------------------------------------------------------
    SolarAdminRoute {
        path: "/admin/solar/evcc",
        home_word_label: "EV charging & solar surplus",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/evcc/loadpoints",
        home_word_label: "EV chargers & heat-pump",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/evcc/planner",
        home_word_label: "Cheap-electricity plan",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/evcc/raw",
        home_word_label: "Surplus loop (developer)",
        developer_view_only: true,
    },
    // SunSpec track ------------------------------------------------------
    SolarAdminRoute {
        path: "/admin/solar/sunspec",
        home_word_label: "Solar inverter & battery",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/sunspec/inverter",
        home_word_label: "Solar inverter",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/sunspec/battery",
        home_word_label: "Home battery",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/sunspec/raw",
        home_word_label: "Modbus registers (developer)",
        developer_view_only: true,
    },
    // Forecast track -----------------------------------------------------
    SolarAdminRoute {
        path: "/admin/solar/forecast",
        home_word_label: "Solar forecast",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/forecast/today",
        home_word_label: "Today's solar production",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/forecast/tomorrow",
        home_word_label: "Tomorrow's solar production",
        developer_view_only: false,
    },
    SolarAdminRoute {
        path: "/admin/solar/forecast/raw",
        home_word_label: "Forecast API responses (developer)",
        developer_view_only: true,
    },
];

/// Returns the route table — useful for the Portal router and for
/// CI checks that the home-word labels are aligned with
/// `docs/ui-language.md`.
#[must_use]
pub const fn routes() -> &'static [SolarAdminRoute] {
    SOLAR_ADMIN_ROUTES
}

/// Returns only the routes visible to a user with Developer view
/// off (the default — Charter §6.3 / ADR-007).
#[must_use]
pub fn routes_for_normal_user() -> Vec<SolarAdminRoute> {
    SOLAR_ADMIN_ROUTES
        .iter()
        .copied()
        .filter(|r| !r.developer_view_only)
        .collect()
}

/// Returns the home-word vocabulary list used by the navigation
/// drawer. Used by the dashboard renderer.
#[must_use]
pub fn home_word_labels() -> Vec<&'static str> {
    SOLAR_ADMIN_ROUTES
        .iter()
        .map(|r| r.home_word_label)
        .collect()
}

pub fn placeholder() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_covers_three_tracks() {
        assert!(SOLAR_ADMIN_ROUTES.iter().any(|r| r.path == "/admin/solar/evcc"));
        assert!(SOLAR_ADMIN_ROUTES.iter().any(|r| r.path == "/admin/solar/sunspec"));
        assert!(SOLAR_ADMIN_ROUTES.iter().any(|r| r.path == "/admin/solar/forecast"));
    }

    #[test]
    fn developer_only_routes_are_marked() {
        let dev = SOLAR_ADMIN_ROUTES
            .iter()
            .filter(|r| r.developer_view_only)
            .count();
        // Three: evcc/raw, sunspec/raw, forecast/raw
        assert_eq!(dev, 3);
    }

    #[test]
    fn home_word_labels_never_leak_implementation() {
        // ADR-007: UI vocabulary must not contain technical leakage.
        let forbidden = [
            "Modbus",
            "register",
            "K3s",
            "Kubernetes",
            "MQTT",
            "Watt",
            "EVCC",
            "SunSpec",
        ];
        for r in routes_for_normal_user() {
            for needle in forbidden {
                assert!(
                    !r.home_word_label.contains(needle),
                    "route {} leaks `{}` to the headline persona",
                    r.path,
                    needle
                );
            }
        }
    }

    #[test]
    fn raw_routes_carry_developer_label() {
        for r in SOLAR_ADMIN_ROUTES {
            if r.developer_view_only {
                assert!(
                    r.home_word_label.contains("developer"),
                    "developer-only route {} should mention `developer`",
                    r.path
                );
            }
        }
    }

    #[test]
    fn route_paths_are_distinct() {
        let mut paths: Vec<&str> = SOLAR_ADMIN_ROUTES.iter().map(|r| r.path).collect();
        paths.sort_unstable();
        let len_before = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), len_before);
    }

    #[test]
    fn normal_user_routes_exclude_raw() {
        for r in routes_for_normal_user() {
            assert!(!r.path.ends_with("/raw"));
        }
    }

    #[test]
    fn home_word_labels_returns_one_per_route() {
        assert_eq!(home_word_labels().len(), SOLAR_ADMIN_ROUTES.len());
    }
}
