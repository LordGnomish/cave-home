// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The overall energy-site status.

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
