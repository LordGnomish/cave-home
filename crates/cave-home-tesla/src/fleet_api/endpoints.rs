// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `/api/1/energy_sites/*` request surface.
//!
//! Each [`EnergyEndpoint`] lowers to an [`ApiRequest`] (method + path + body).
//! Building the request is pure; the transport ([`super::client`]) prefixes the
//! regional [`super::Region::base_url`] and performs the call.

#[cfg(test)]
mod tests {
    use super::super::Region;
    use super::*;

    #[test]
    fn region_base_urls() {
        assert_eq!(
            Region::NorthAmericaAsiaPacific.base_url(),
            "https://fleet-api.prd.na.vn.cloud.tesla.com"
        );
        assert_eq!(
            Region::Europe.base_url(),
            "https://fleet-api.prd.eu.vn.cloud.tesla.com"
        );
        assert_eq!(Region::China.base_url(), "https://fleet-api.prd.cn.vn.cloud.tesla.cn");
    }

    #[test]
    fn region_parses_from_config_key() {
        assert_eq!(Region::from_key("na"), Some(Region::NorthAmericaAsiaPacific));
        assert_eq!(Region::from_key("eu"), Some(Region::Europe));
        assert_eq!(Region::from_key("cn"), Some(Region::China));
        assert_eq!(Region::from_key("mars"), None);
    }

    #[test]
    fn products_is_a_get() {
        let r = EnergyEndpoint::Products.request();
        assert_eq!(r.method, HttpMethod::Get);
        assert_eq!(r.path, "/api/1/products");
        assert!(r.body.is_none());
    }

    #[test]
    fn live_status_path_includes_site() {
        let r = EnergyEndpoint::LiveStatus(99).request();
        assert_eq!(r.method, HttpMethod::Get);
        assert_eq!(r.path, "/api/1/energy_sites/99/live_status");
    }

    #[test]
    fn site_info_and_status_paths() {
        assert_eq!(
            EnergyEndpoint::SiteInfo(7).request().path,
            "/api/1/energy_sites/7/site_info"
        );
        assert_eq!(
            EnergyEndpoint::SiteStatus(7).request().path,
            "/api/1/energy_sites/7/site_status"
        );
    }

    #[test]
    fn history_encodes_kind_and_period() {
        let r = EnergyEndpoint::History {
            site_id: 5,
            kind: HistoryKind::Power,
            period: HistoryPeriod::Day,
        }
        .request();
        assert_eq!(r.method, HttpMethod::Get);
        assert_eq!(
            r.path,
            "/api/1/energy_sites/5/calendar_history?kind=power&period=day"
        );
    }

    #[test]
    fn backup_reserve_is_a_post_with_body() {
        let r = EnergyEndpoint::SetBackupReserve { site_id: 3, percent: 25 }.request();
        assert_eq!(r.method, HttpMethod::Post);
        assert_eq!(r.path, "/api/1/energy_sites/3/backup");
        let body = r.body.unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["backup_reserve_percent"], 25);
    }

    #[test]
    fn operation_mode_post_body() {
        let r = EnergyEndpoint::SetOperationMode {
            site_id: 3,
            mode: "self_consumption",
        }
        .request();
        assert_eq!(r.method, HttpMethod::Post);
        assert_eq!(r.path, "/api/1/energy_sites/3/operation");
        let v: serde_json::Value = serde_json::from_str(&r.body.unwrap()).unwrap();
        assert_eq!(v["default_real_mode"], "self_consumption");
    }

    #[test]
    fn storm_mode_post_body() {
        let r = EnergyEndpoint::SetStormMode { site_id: 3, enabled: true }.request();
        let v: serde_json::Value = serde_json::from_str(&r.body.unwrap()).unwrap();
        assert_eq!(v["enabled"], true);
        assert_eq!(r.path, "/api/1/energy_sites/3/storm_mode");
    }

    #[test]
    fn full_url_prefixes_region_base() {
        let r = EnergyEndpoint::LiveStatus(1).request();
        assert_eq!(
            r.full_url(Region::Europe),
            "https://fleet-api.prd.eu.vn.cloud.tesla.com/api/1/energy_sites/1/live_status"
        );
    }
}
