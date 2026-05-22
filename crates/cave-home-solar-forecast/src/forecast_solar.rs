// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! [Forecast.Solar](https://doc.forecast.solar/api) HTTP client.
//!
//! The endpoint cave-home consumes is `/estimate/{lat}/{lon}/{dec}/{az}/{kwp}`
//! (public tier, no API key required).
//!
//! Response (public tier, JSON, abridged):
//! ```json
//! {
//!   "result": {
//!     "watts":         { "2026-05-17 06:00:00": 0,    "2026-05-17 07:00:00": 800, ... },
//!     "watt_hours":    { ... },
//!     "watt_hours_day":{ "2026-05-17": 41230, "2026-05-18": 39120 }
//!   },
//!   "message": { "code": 0 }
//! }
//! ```

use crate::error::{Error, Result};
use crate::http::{HttpClient, HttpRequest};
use crate::site::PvString;
use crate::summary::{Forecast, ForecastSlot};
use crate::FORECAST_SOLAR_BASE_URL;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Forecast.Solar account tier — controls quota.
/// Source: <https://doc.forecast.solar/account>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSolarTier {
    Public,
    Personal,
    Professional,
}

impl ForecastSolarTier {
    /// Daily request quota documented at <https://doc.forecast.solar/account>.
    #[must_use]
    pub const fn daily_quota(self) -> u32 {
        match self {
            Self::Public => 12,
            Self::Personal => 600,
            Self::Professional => 5000,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    result: ApiResult,
    #[serde(default)]
    message: Option<ApiMessage>,
}

#[derive(Debug, Deserialize)]
struct ApiResult {
    #[serde(default)]
    watts: std::collections::BTreeMap<String, f64>,
    #[serde(default, rename = "watt_hours_day")]
    watt_hours_day: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    #[serde(default)]
    code: i32,
}

/// Parsed estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastSolarEstimate {
    pub forecast: Forecast,
}

/// Forecast.Solar client.
#[derive(Debug)]
pub struct ForecastSolarClient<T: HttpClient> {
    pub transport: T,
    pub base_url: String,
    pub tier: ForecastSolarTier,
    pub api_key: Option<String>,
}

impl<T: HttpClient> ForecastSolarClient<T> {
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            base_url: FORECAST_SOLAR_BASE_URL.to_string(),
            tier: ForecastSolarTier::Public,
            api_key: None,
        }
    }

    /// Build the public-tier URL for a single string. Pattern:
    /// `{base}/estimate/{lat}/{lon}/{tilt}/{azimuth}/{kwp}`.
    /// For paid tiers the URL is prefixed with `/{api_key}`.
    ///
    /// Source: <https://doc.forecast.solar/api>.
    #[must_use]
    pub fn build_url(&self, lat: f64, lon: f64, s: &PvString) -> String {
        let key_segment = self.api_key.as_deref().map_or(String::new(), |k| format!("/{k}"));
        format!(
            "{}{}/estimate/{:.4}/{:.4}/{}/{}/{}",
            self.base_url, key_segment, lat, lon, s.tilt_deg as i32, s.azimuth_deg as i32, s.peak_kwp
        )
    }

    /// Fetch and parse an estimate for one PV string.
    ///
    /// # Errors
    ///
    /// Returns [`Error::QuotaExhausted`] on HTTP 429, [`Error::HttpStatus`]
    /// on any other non-2xx, or [`Error::Malformed`] when the JSON
    /// envelope can't be decoded.
    pub async fn estimate(&self, lat: f64, lon: f64, s: &PvString) -> Result<ForecastSolarEstimate> {
        s.validate()?;
        let url = self.build_url(lat, lon, s);
        let resp = self.transport.fetch(HttpRequest::get(url)).await?;
        match resp.status {
            200 => self.parse_envelope(&resp.body),
            429 => Err(Error::QuotaExhausted),
            other => Err(Error::HttpStatus {
                status: other,
                body: resp.body,
            }),
        }
    }

    fn parse_envelope(&self, body: &str) -> Result<ForecastSolarEstimate> {
        let env: ApiEnvelope =
            serde_json::from_str(body).map_err(|e| Error::Malformed(e.to_string()))?;
        if let Some(msg) = env.message {
            if msg.code != 0 {
                return Err(Error::Malformed(format!(
                    "forecast.solar message.code={}",
                    msg.code
                )));
            }
        }

        let mut forecast = Forecast::new("forecast.solar");
        for (ts_str, watts) in env.result.watts {
            // Forecast.Solar timestamps are local "YYYY-MM-DD HH:MM:SS" without
            // timezone info. We parse them as if they were UTC, which is wrong
            // for sub-hour ordering but harmless for the sums we expose. The
            // Portal renders them in the user's local time via the wall clock,
            // so consumers MUST treat `start` as "approximate hour bucket".
            let Some(ts) = parse_local_timestamp(&ts_str) else {
                continue;
            };
            let kwh = watts / 1000.0;
            forecast.hourly.push(ForecastSlot { start: ts, kwh });
        }
        forecast.hourly.sort_by_key(|s| s.start);

        if !env.result.watt_hours_day.is_empty() {
            // Sum first two days into today/tomorrow if present.
            let mut days: Vec<(String, f64)> = env.result.watt_hours_day.into_iter().collect();
            days.sort_by(|a, b| a.0.cmp(&b.0));
            forecast.kwh_today = days.first().map_or(0.0, |(_, wh)| wh / 1000.0);
            forecast.kwh_tomorrow = days.get(1).map_or(0.0, |(_, wh)| wh / 1000.0);
        }
        forecast.peak_kw = forecast
            .hourly
            .iter()
            .map(|s| s.kwh)
            .fold(0.0f64, f64::max);

        Ok(ForecastSolarEstimate { forecast })
    }
}

/// Permissive parser for the upstream "YYYY-MM-DD HH:MM:SS" string.
/// Caves out to `None` on malformed inputs. cave-home doesn't pull
/// a `chrono` dependency for this — the resulting `SystemTime` is
/// computed as days-since-epoch × 86400 + seconds. This is good
/// enough for slot ordering and aggregate sums.
fn parse_local_timestamp(s: &str) -> Option<SystemTime> {
    let (date_part, time_part) = s.split_once(' ')?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: u32 = date_iter.next()?.parse().ok()?;
    let day: u32 = date_iter.next()?.parse().ok()?;
    let mut time_iter = time_part.split(':');
    let hour: u64 = time_iter.next()?.parse().ok()?;
    let minute: u64 = time_iter.next()?.parse().ok()?;
    let second: u64 = time_iter.next()?.parse().ok()?;

    // Civil-from-days algorithm (Howard Hinnant, public domain).
    let days = civil_to_days(year, month, day);
    let seconds = days * 86_400 + (hour * 3600 + minute * 60 + second) as i64;
    if seconds < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(seconds as u64))
}

/// Howard Hinnant `days_from_civil` — converts `year/month/day` to
/// days since 1970-01-01. Reference:
/// <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>.
fn civil_to_days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockHttpClient};

    fn sample_envelope() -> &'static str {
        r#"{
          "result": {
            "watts": {
              "2026-05-17 06:00:00": 0,
              "2026-05-17 07:00:00": 800,
              "2026-05-17 12:00:00": 7400,
              "2026-05-17 18:00:00": 1200
            },
            "watt_hours_day": {
              "2026-05-17": 41230,
              "2026-05-18": 39120
            }
          },
          "message": { "code": 0 }
        }"#
    }

    #[test]
    fn url_format_public_tier() {
        let m = MockHttpClient::new();
        let c = ForecastSolarClient::new(m);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let url = c.build_url(48.4321, 11.1234, &s);
        assert_eq!(url, "https://api.forecast.solar/estimate/48.4321/11.1234/30/0/8.2");
    }

    #[test]
    fn url_format_with_api_key() {
        let m = MockHttpClient::new();
        let mut c = ForecastSolarClient::new(m);
        c.api_key = Some("ABCDEF".into());
        let s = PvString::new(30.0, -45.0, 4.0).unwrap();
        let url = c.build_url(48.0, 11.0, &s);
        assert_eq!(url, "https://api.forecast.solar/ABCDEF/estimate/48.0000/11.0000/30/-45/4");
    }

    #[tokio::test]
    async fn estimate_parses_envelope() {
        let mock = MockHttpClient::new();
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let c = ForecastSolarClient::new(mock);
        let url = c.build_url(48.0, 11.0, &s);
        c.transport.insert(
            url,
            HttpResponse {
                status: 200,
                body: sample_envelope().to_string(),
            },
        );
        let e = c.estimate(48.0, 11.0, &s).await.unwrap();
        assert_eq!(e.forecast.source, "forecast.solar");
        assert!((e.forecast.kwh_today - 41.230).abs() < 1e-3);
        assert!((e.forecast.kwh_tomorrow - 39.120).abs() < 1e-3);
        // Peak slot is 7.4 kW (= 7400 W / 1000)
        assert!((e.forecast.peak_kw - 7.4).abs() < 1e-6);
        assert_eq!(e.forecast.hourly.len(), 4);
    }

    #[tokio::test]
    async fn estimate_429_yields_quota_exhausted() {
        let mock = MockHttpClient::new();
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let c = ForecastSolarClient::new(mock);
        c.transport.insert(
            c.build_url(48.0, 11.0, &s),
            HttpResponse {
                status: 429,
                body: "rate limited".into(),
            },
        );
        assert!(matches!(
            c.estimate(48.0, 11.0, &s).await,
            Err(Error::QuotaExhausted)
        ));
    }

    #[tokio::test]
    async fn estimate_500_is_http_status() {
        let mock = MockHttpClient::new();
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let c = ForecastSolarClient::new(mock);
        c.transport.insert(
            c.build_url(48.0, 11.0, &s),
            HttpResponse {
                status: 503,
                body: "upstream down".into(),
            },
        );
        assert!(matches!(
            c.estimate(48.0, 11.0, &s).await,
            Err(Error::HttpStatus { status: 503, .. })
        ));
    }

    #[tokio::test]
    async fn estimate_malformed_json() {
        let mock = MockHttpClient::new();
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let c = ForecastSolarClient::new(mock);
        c.transport.insert(
            c.build_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: "{ bad json".into(),
            },
        );
        assert!(matches!(
            c.estimate(48.0, 11.0, &s).await,
            Err(Error::Malformed(_))
        ));
    }

    #[tokio::test]
    async fn estimate_nonzero_message_code_rejected() {
        let mock = MockHttpClient::new();
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let c = ForecastSolarClient::new(mock);
        c.transport.insert(
            c.build_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: r#"{"result":{"watts":{},"watt_hours_day":{}},"message":{"code":2}}"#.into(),
            },
        );
        assert!(matches!(
            c.estimate(48.0, 11.0, &s).await,
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn quota_table_matches_doc() {
        assert_eq!(ForecastSolarTier::Public.daily_quota(), 12);
        assert_eq!(ForecastSolarTier::Personal.daily_quota(), 600);
        assert_eq!(ForecastSolarTier::Professional.daily_quota(), 5000);
    }

    #[test]
    fn local_timestamp_parser_round_trip() {
        let t = parse_local_timestamp("2026-05-17 12:00:00").unwrap();
        let secs = t.duration_since(UNIX_EPOCH).unwrap().as_secs();
        // 2026-05-17 12:00:00 UTC = 1779019200 (verified via days_from_civil)
        assert_eq!(secs, 1779019200);
    }

    #[test]
    fn local_timestamp_parser_returns_none_on_garbage() {
        assert!(parse_local_timestamp("not a date").is_none());
    }
}
