// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The overall energy-site status.

use super::OpMode;
use crate::fleet_api::types::{LiveStatus, SiteInfo};

/// The site's overall state — the merge of a live-status sample with the site's
/// configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteStatus {
    /// The household-chosen site name (from `site_info`).
    pub name: Option<String>,
    /// Whether the site is grid-connected (grid status `Active`).
    pub grid_connected: bool,
    /// The microgrid island status, if reported.
    pub island_status: Option<String>,
    /// Whether storm watch is currently active.
    pub storm_mode_active: bool,
    /// The configured operation mode (from `site_info`).
    pub op_mode: Option<OpMode>,
    /// The configured backup reserve percent (from `site_info`).
    pub backup_reserve_percent: Option<u8>,
}

impl SiteStatus {
    /// Build from a live-status sample and optional site configuration.
    #[must_use]
    pub fn from_parts(live: &LiveStatus, info: Option<&SiteInfo>) -> Self {
        Self {
            name: info.and_then(|i| i.site_name.clone()),
            grid_connected: live.grid_status.eq_ignore_ascii_case("Active"),
            island_status: live.island_status.clone(),
            storm_mode_active: live.storm_mode_active.unwrap_or(false),
            op_mode: info.and_then(|i| OpMode::from_wire(&i.default_real_mode)),
            backup_reserve_percent: info.map(|i| i.backup_reserve_percent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::OpMode;
    use super::*;
    use crate::fleet_api::types::{Envelope, LiveStatus, SiteInfo};

    fn live(grid_status: &str, island: &str, storm: bool) -> LiveStatus {
        let json = format!(
            r#"{{"response":{{"solar_power":0,"battery_power":0,"load_power":0,"grid_power":0,
            "percentage_charged":50,"energy_left":0,"total_pack_energy":0,
            "grid_status":"{grid_status}","island_status":"{island}","storm_mode_active":{storm}}}}}"#
        );
        serde_json::from_str::<Envelope<LiveStatus>>(&json).unwrap().response
    }

    fn info() -> SiteInfo {
        let json = r#"{"response":{"id":"STE9","site_name":"Cave","backup_reserve_percent":20,
            "default_real_mode":"backup","battery_count":1,"nameplate_power":5000,
            "nameplate_energy":13500}}"#;
        serde_json::from_str::<Envelope<SiteInfo>>(json).unwrap().response
    }

    #[test]
    fn grid_connected_from_active_status() {
        let s = SiteStatus::from_parts(&live("Active", "on_grid", false), None);
        assert!(s.grid_connected);
        let off = SiteStatus::from_parts(&live("Inactive", "off_grid", false), None);
        assert!(!off.grid_connected);
    }

    #[test]
    fn storm_mode_flag_carried() {
        let s = SiteStatus::from_parts(&live("Active", "on_grid", true), None);
        assert!(s.storm_mode_active);
    }

    #[test]
    fn site_info_merges_name_mode_and_reserve() {
        let s = SiteStatus::from_parts(&live("Active", "on_grid", false), Some(&info()));
        assert_eq!(s.name.as_deref(), Some("Cave"));
        assert_eq!(s.op_mode, Some(OpMode::Backup));
        assert_eq!(s.backup_reserve_percent, Some(20));
    }

    #[test]
    fn without_site_info_optionals_are_none() {
        let s = SiteStatus::from_parts(&live("Active", "on_grid", false), None);
        assert!(s.name.is_none());
        assert!(s.op_mode.is_none());
        assert!(s.backup_reserve_percent.is_none());
    }
}
