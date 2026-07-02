// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The `/api/1/energy_sites/*` request surface.
//!
//! Each [`EnergyEndpoint`] lowers to an [`ApiRequest`] (method + path + body).
//! Building the request is pure; the transport ([`super::client`]) prefixes the
//! regional [`super::Region::base_url`] and performs the call.

use serde_json::json;

use super::Region;

/// The HTTP verbs the energy surface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// A read.
    Get,
    /// A command.
    Post,
}

impl HttpMethod {
    /// The uppercase wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// A lowered, transport-ready request: a method, a path (relative to the
/// regional base) and an optional JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The path + query, relative to the regional base host.
    pub path: String,
    /// The JSON request body, for commands.
    pub body: Option<String>,
}

impl ApiRequest {
    /// The absolute URL for this request against `region`.
    #[must_use]
    pub fn full_url(&self, region: Region) -> String {
        format!("{}{}", region.base_url(), self.path)
    }
}

/// The kind of calendar-history series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// Instantaneous power samples (watts).
    Power,
    /// Aggregated energy buckets (watt-hours).
    Energy,
}

impl HistoryKind {
    /// The wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Energy => "energy",
        }
    }
}

/// The aggregation period for calendar history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPeriod {
    /// One day.
    Day,
    /// One week.
    Week,
    /// One month.
    Month,
    /// One year.
    Year,
}

impl HistoryPeriod {
    /// The wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

/// One energy-site API operation, before it is lowered to an [`ApiRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnergyEndpoint<'a> {
    /// List the account's products (vehicles + energy sites).
    Products,
    /// Live power flows + state of charge for a site.
    LiveStatus(u64),
    /// Site configuration.
    SiteInfo(u64),
    /// Coarse site status.
    SiteStatus(u64),
    /// A calendar-history time series.
    History {
        /// The energy site id.
        site_id: u64,
        /// Power vs energy.
        kind: HistoryKind,
        /// Aggregation period.
        period: HistoryPeriod,
    },
    /// Set the backup reserve percent.
    SetBackupReserve {
        /// The energy site id.
        site_id: u64,
        /// Reserve, 0..=100.
        percent: u8,
    },
    /// Set the operation mode (`self_consumption` / `backup` / `autonomous`).
    SetOperationMode {
        /// The energy site id.
        site_id: u64,
        /// The Tesla wire mode string.
        mode: &'a str,
    },
    /// Enable/disable storm watch.
    SetStormMode {
        /// The energy site id.
        site_id: u64,
        /// Whether storm watch is enabled.
        enabled: bool,
    },
}

impl EnergyEndpoint<'_> {
    /// Lower this operation to a method/path/body.
    #[must_use]
    pub fn request(&self) -> ApiRequest {
        match self {
            Self::Products => get("/api/1/products".to_string()),
            Self::LiveStatus(id) => get(format!("/api/1/energy_sites/{id}/live_status")),
            Self::SiteInfo(id) => get(format!("/api/1/energy_sites/{id}/site_info")),
            Self::SiteStatus(id) => get(format!("/api/1/energy_sites/{id}/site_status")),
            Self::History {
                site_id,
                kind,
                period,
            } => get(format!(
                "/api/1/energy_sites/{site_id}/calendar_history?kind={}&period={}",
                kind.as_str(),
                period.as_str()
            )),
            Self::SetBackupReserve { site_id, percent } => post(
                format!("/api/1/energy_sites/{site_id}/backup"),
                &json!({ "backup_reserve_percent": percent }),
            ),
            Self::SetOperationMode { site_id, mode } => post(
                format!("/api/1/energy_sites/{site_id}/operation"),
                &json!({ "default_real_mode": mode }),
            ),
            Self::SetStormMode { site_id, enabled } => post(
                format!("/api/1/energy_sites/{site_id}/storm_mode"),
                &json!({ "enabled": enabled }),
            ),
        }
    }
}

const fn get(path: String) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Get,
        path,
        body: None,
    }
}

fn post(path: String, body: &serde_json::Value) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Post,
        path,
        body: Some(body.to_string()),
    }
}

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
