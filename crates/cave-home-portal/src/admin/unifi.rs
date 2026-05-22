// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Portal admin sub-tree for the UniFi ecosystem (ADR-009).
//!
//! Mounts under `/admin/unifi/*`:
//!   - `/admin/unifi/network/*`  — switches, APs, port telemetry, client tracking
//!   - `/admin/unifi/protect/*`  — cameras + doorbell events (merges with
//!                                  cave-home-camera/Frigate via `FrigateSeam`)
//!   - `/admin/unifi/access/*`   — door state + access events + emergency status
//!   - `/admin/unifi/talk/*`     — TalkPhone roster + incoming calls + control
//!
//! Charter v6 §6.3 / ADR-007: every label rendered to grandma uses the
//! home-world vocabulary helpers from the underlying crates
//! (`friendly_device_label`, `friendly_camera_label`, `friendly_door_label`,
//! `friendly_phone_label`). MAC addresses, GUIDs, port indexes appear
//! only behind the `?verbose=1` query — never in the default view.
//!
//! Phase 1 portal hook surfaces the route table + the per-pillar mount
//! function the Lovelace frontend calls during boot. Real router wiring
//! lands when the Phase 2 portal HTTP frame ships (M2).

/// Top-level UniFi admin route prefix.
pub const ROUTE_PREFIX: &str = "/admin/unifi";

/// All four sub-pillar route prefixes.
#[must_use]
pub fn subtree_routes() -> [&'static str; 4] {
    [
        "/admin/unifi/network",
        "/admin/unifi/protect",
        "/admin/unifi/access",
        "/admin/unifi/talk",
    ]
}

/// Per-pillar route catalogue for the Phase 1 portal. Each entry is
/// the home-world label the grandma-friendly nav shows alongside the
/// admin route.
#[must_use]
pub fn nav_entries() -> Vec<NavEntry> {
    vec![
        NavEntry {
            route: "/admin/unifi/network",
            grandma_label: "Wi-Fi & ev ağı",
            developer_label: "UniFi Network",
        },
        NavEntry {
            route: "/admin/unifi/protect",
            grandma_label: "Kameralar",
            developer_label: "UniFi Protect",
        },
        NavEntry {
            route: "/admin/unifi/access",
            grandma_label: "Kapılar",
            developer_label: "UniFi Access",
        },
        NavEntry {
            route: "/admin/unifi/talk",
            grandma_label: "İnterkomlar",
            developer_label: "UniFi Talk",
        },
    ]
}

/// A single nav entry (grandma label + developer label + route).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavEntry {
    /// Admin route mounted by this pillar.
    pub route: &'static str,
    /// Charter §6.3 home-world label (default view).
    pub grandma_label: &'static str,
    /// Verbose / developer-view label.
    pub developer_label: &'static str,
}

/// True when the verbose toggle is on. Used by every sub-pillar tile
/// to decide whether to render the raw MAC / GUID / port index.
#[must_use]
pub fn is_verbose_view(query_verbose: bool) -> bool {
    query_verbose
}

/// Sub-tree mount points — each emits a route string. The Phase 2
/// HTTP frame iterates this list during boot and registers each route
/// against an `axum::Router`. Phase 1 just enumerates them so the
/// observability dashboard can show "X routes mounted".
#[must_use]
pub fn mounted_routes_summary() -> String {
    let routes = subtree_routes();
    format!("/admin/unifi mounts {} sub-routes", routes.len())
}

/// Network pillar — switch + AP tiles, client list, port stat tiles.
pub mod network {
    /// Route path (HA-style: per-site).
    pub const ROUTE: &str = "/admin/unifi/network";
    /// Tile kinds the network pillar renders.
    #[must_use]
    pub fn tiles() -> &'static [&'static str] {
        &["devices", "clients", "ports", "wifi"]
    }
}

/// Protect pillar — camera tiles + event timeline + Frigate-seam editor.
pub mod protect {
    /// Route path.
    pub const ROUTE: &str = "/admin/unifi/protect";
    /// Tile kinds the protect pillar renders.
    #[must_use]
    pub fn tiles() -> &'static [&'static str] {
        &["cameras", "events", "doorbells", "frigate-seam"]
    }
}

/// Access pillar — door tiles + access-event timeline + emergency status.
pub mod access {
    /// Route path.
    pub const ROUTE: &str = "/admin/unifi/access";
    /// Tile kinds the access pillar renders.
    #[must_use]
    pub fn tiles() -> &'static [&'static str] {
        &["doors", "events", "emergency", "lock-rules"]
    }
}

/// Talk pillar — phone tiles + incoming-call tiles + call control.
pub mod talk {
    /// Route path.
    pub const ROUTE: &str = "/admin/unifi/talk";
    /// Tile kinds the talk pillar renders.
    #[must_use]
    pub fn tiles() -> &'static [&'static str] {
        &["phones", "incoming", "history"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_prefix_is_admin_unifi() {
        assert_eq!(ROUTE_PREFIX, "/admin/unifi");
    }

    #[test]
    fn subtree_has_four_pillars() {
        assert_eq!(subtree_routes().len(), 4);
    }

    #[test]
    fn nav_entries_match_pillars() {
        let entries = nav_entries();
        assert_eq!(entries.len(), 4);
        // Every nav entry's route must appear in subtree_routes.
        let routes: Vec<_> = subtree_routes().to_vec();
        for entry in entries {
            assert!(
                routes.contains(&entry.route),
                "nav entry route '{}' not in subtree routes",
                entry.route
            );
        }
    }

    #[test]
    fn grandma_labels_use_turkish_home_world() {
        let entries = nav_entries();
        let labels: Vec<_> = entries.iter().map(|e| e.grandma_label).collect();
        // ADR-007 — none of the grandma labels contain "UniFi", "MAC",
        // "VLAN", "RTSP" or other API jargon.
        for label in &labels {
            for jargon in &["UniFi", "MAC", "VLAN", "RTSP", "GUID", "API", "controller"] {
                assert!(
                    !label.contains(jargon),
                    "grandma label '{label}' must not contain jargon '{jargon}'"
                );
            }
        }
    }

    #[test]
    fn developer_labels_keep_unifi_name() {
        let entries = nav_entries();
        for e in entries {
            assert!(e.developer_label.contains("UniFi"));
        }
    }

    #[test]
    fn verbose_toggle_is_pass_through() {
        assert!(!is_verbose_view(false));
        assert!(is_verbose_view(true));
    }

    #[test]
    fn mounted_routes_summary_mentions_count() {
        assert!(mounted_routes_summary().contains("4"));
    }

    #[test]
    fn network_pillar_tiles() {
        assert!(network::tiles().contains(&"devices"));
        assert!(network::tiles().contains(&"clients"));
    }

    #[test]
    fn protect_pillar_has_frigate_seam() {
        assert!(protect::tiles().contains(&"frigate-seam"));
    }

    #[test]
    fn access_pillar_has_emergency() {
        assert!(access::tiles().contains(&"emergency"));
    }

    #[test]
    fn talk_pillar_has_phones() {
        assert!(talk::tiles().contains(&"phones"));
    }
}
