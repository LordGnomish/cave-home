// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! [PVGIS](https://re.jrc.ec.europa.eu/api/v5_2/) HTTP client — the
//! Joint Research Centre's solar resource service.
//!
//! cave-home consumes the `MRcalc` (monthly radiation) endpoint to
//! produce a baseline monthly yield estimate. Forecast.Solar is the
//! preferred short-term forecaster — PVGIS provides the long-term
//! sizing reference that doesn't depend on a paid tier.
//!
//! Endpoint: `GET /v5_2/MRcalc?lat=…&lon=…&peakpower=…&mountingplace=…
//!                          &loss=14&aspect=…&angle=…&outputformat=json`
//!
//! Response (abridged):
//! ```json
//! {
//!   "outputs": {
//!     "monthly": [
//!       { "month": 1, "E_m": 350.5, "H(i)_m": 45.0 },
//!       { "month": 2, "E_m": 420.0, ... },
//!       ...
//!     ]
//!   }
//! }
//! ```

use crate::error::{Error, Result};
use crate::http::{HttpClient, HttpRequest};
use crate::site::PvString;
use crate::PVGIS_BASE_URL;
use serde::Deserialize;

/// PVGIS radiation database. Default is `PVGIS-SARAH2`. Source:
/// <https://joint-research-centre.ec.europa.eu/photovoltaic-geographical-information-system-pvgis/getting-started-pvgis/pvgis-data-sources-calculation-methods_en>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvGisRadiationDatabase {
    Sarah2,
    Sarah3,
    Era5,
}

impl PvGisRadiationDatabase {
    #[must_use]
    pub const fn as_query_value(self) -> &'static str {
        match self {
            Self::Sarah2 => "PVGIS-SARAH2",
            Self::Sarah3 => "PVGIS-SARAH3",
            Self::Era5 => "PVGIS-ERA5",
        }
    }
}

/// PVGIS mounting place — feeds the `mountingplace` query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvGisMounting {
    Free,
    Building,
}

impl PvGisMounting {
    #[must_use]
    pub const fn as_query_value(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Building => "building",
        }
    }
}

/// One row from the `outputs.monthly` array.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PvGisMonthly {
    pub month: u8,
    /// Monthly energy production in kWh.
    #[serde(rename = "E_m")]
    pub energy_kwh: f64,
    /// Monthly in-plane irradiation in kWh/m² (optional).
    #[serde(rename = "H(i)_m", default)]
    pub irradiation_kwh_m2: f64,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    outputs: ApiOutputs,
}

#[derive(Debug, Deserialize)]
struct ApiOutputs {
    #[serde(default)]
    monthly: Vec<PvGisMonthly>,
}

/// PVGIS client.
#[derive(Debug)]
pub struct PvGisClient<T: HttpClient> {
    pub transport: T,
    pub base_url: String,
    pub radiation_database: PvGisRadiationDatabase,
    pub mounting: PvGisMounting,
    pub loss_percent: f64,
}

impl<T: HttpClient> PvGisClient<T> {
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            base_url: PVGIS_BASE_URL.to_string(),
            radiation_database: PvGisRadiationDatabase::Sarah2,
            mounting: PvGisMounting::Free,
            loss_percent: 14.0,
        }
    }

    /// Build the `/MRcalc` URL. Source: PVGIS user manual v5.2
    /// (<https://re.jrc.ec.europa.eu/api/v5_2/MRcalc?>...).
    #[must_use]
    pub fn build_mrcalc_url(&self, lat: f64, lon: f64, s: &PvString) -> String {
        format!(
            "{}/MRcalc?lat={:.4}&lon={:.4}&peakpower={}&loss={}&angle={}&aspect={}&mountingplace={}&pvtechchoice=crystSi&raddatabase={}&outputformat=json",
            self.base_url,
            lat,
            lon,
            s.peak_kwp,
            self.loss_percent,
            s.tilt_deg as i32,
            s.azimuth_deg as i32,
            self.mounting.as_query_value(),
            self.radiation_database.as_query_value(),
        )
    }

    /// Fetch + parse the monthly radiation series.
    ///
    /// # Errors
    ///
    /// Returns [`Error::HttpStatus`] on any non-2xx response;
    /// [`Error::Malformed`] when the JSON cannot be decoded;
    /// [`Error::PvGisEmpty`] when the monthly array is empty.
    pub async fn monthly(&self, lat: f64, lon: f64, s: &PvString) -> Result<Vec<PvGisMonthly>> {
        s.validate()?;
        let url = self.build_mrcalc_url(lat, lon, s);
        let resp = self.transport.fetch(HttpRequest::get(url)).await?;
        if resp.status != 200 {
            return Err(Error::HttpStatus {
                status: resp.status,
                body: resp.body,
            });
        }
        let env: ApiEnvelope =
            serde_json::from_str(&resp.body).map_err(|e| Error::Malformed(e.to_string()))?;
        if env.outputs.monthly.is_empty() {
            return Err(Error::PvGisEmpty);
        }
        Ok(env.outputs.monthly)
    }

    /// Convenience: annual yield in kWh = sum of monthly E_m.
    ///
    /// # Errors
    /// Same as [`Self::monthly`].
    pub async fn annual_yield_kwh(&self, lat: f64, lon: f64, s: &PvString) -> Result<f64> {
        let m = self.monthly(lat, lon, s).await?;
        Ok(m.iter().map(|r| r.energy_kwh).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockHttpClient};

    fn sample_response() -> &'static str {
        r#"{
          "outputs": {
            "monthly": [
              { "month": 1,  "E_m": 350.5,  "H(i)_m": 45.0 },
              { "month": 2,  "E_m": 420.0,  "H(i)_m": 55.0 },
              { "month": 3,  "E_m": 620.0,  "H(i)_m": 80.0 },
              { "month": 4,  "E_m": 780.0,  "H(i)_m": 100.0 },
              { "month": 5,  "E_m": 920.0,  "H(i)_m": 120.0 },
              { "month": 6,  "E_m": 990.0,  "H(i)_m": 130.0 },
              { "month": 7,  "E_m": 1010.0, "H(i)_m": 132.0 },
              { "month": 8,  "E_m": 950.0,  "H(i)_m": 124.0 },
              { "month": 9,  "E_m": 760.0,  "H(i)_m": 98.0 },
              { "month": 10, "E_m": 510.0,  "H(i)_m": 66.0 },
              { "month": 11, "E_m": 360.0,  "H(i)_m": 46.0 },
              { "month": 12, "E_m": 320.0,  "H(i)_m": 40.0 }
            ]
          }
        }"#
    }

    #[test]
    fn url_format_default_options() {
        let m = MockHttpClient::new();
        let c = PvGisClient::new(m);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let url = c.build_mrcalc_url(48.4321, 11.1234, &s);
        assert!(url.starts_with("https://re.jrc.ec.europa.eu/api/v5_2/MRcalc?"));
        assert!(url.contains("lat=48.4321"));
        assert!(url.contains("lon=11.1234"));
        assert!(url.contains("peakpower=8.2"));
        assert!(url.contains("angle=30"));
        assert!(url.contains("mountingplace=free"));
        assert!(url.contains("raddatabase=PVGIS-SARAH2"));
        assert!(url.contains("outputformat=json"));
    }

    #[test]
    fn url_format_custom_database() {
        let m = MockHttpClient::new();
        let mut c = PvGisClient::new(m);
        c.radiation_database = PvGisRadiationDatabase::Era5;
        c.mounting = PvGisMounting::Building;
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        let url = c.build_mrcalc_url(48.0, 11.0, &s);
        assert!(url.contains("raddatabase=PVGIS-ERA5"));
        assert!(url.contains("mountingplace=building"));
    }

    #[tokio::test]
    async fn monthly_parses_full_year() {
        let mock = MockHttpClient::new();
        let c = PvGisClient::new(mock);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        c.transport.insert(
            c.build_mrcalc_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: sample_response().into(),
            },
        );
        let monthly = c.monthly(48.0, 11.0, &s).await.unwrap();
        assert_eq!(monthly.len(), 12);
        assert_eq!(monthly[6].month, 7);
        assert!((monthly[6].energy_kwh - 1010.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn annual_yield_sums_monthly() {
        let mock = MockHttpClient::new();
        let c = PvGisClient::new(mock);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        c.transport.insert(
            c.build_mrcalc_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: sample_response().into(),
            },
        );
        let total = c.annual_yield_kwh(48.0, 11.0, &s).await.unwrap();
        let expected: f64 = [350.5, 420.0, 620.0, 780.0, 920.0, 990.0, 1010.0, 950.0, 760.0, 510.0, 360.0, 320.0]
            .iter()
            .sum();
        assert!((total - expected).abs() < 1e-6);
    }

    #[tokio::test]
    async fn monthly_500_is_http_status() {
        let mock = MockHttpClient::new();
        let c = PvGisClient::new(mock);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        c.transport.insert(
            c.build_mrcalc_url(48.0, 11.0, &s),
            HttpResponse {
                status: 500,
                body: "down".into(),
            },
        );
        assert!(matches!(
            c.monthly(48.0, 11.0, &s).await,
            Err(Error::HttpStatus { status: 500, .. })
        ));
    }

    #[tokio::test]
    async fn monthly_empty_is_pvgis_empty() {
        let mock = MockHttpClient::new();
        let c = PvGisClient::new(mock);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        c.transport.insert(
            c.build_mrcalc_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: r#"{"outputs":{"monthly":[]}}"#.into(),
            },
        );
        assert!(matches!(
            c.monthly(48.0, 11.0, &s).await,
            Err(Error::PvGisEmpty)
        ));
    }

    #[tokio::test]
    async fn monthly_malformed_is_malformed_err() {
        let mock = MockHttpClient::new();
        let c = PvGisClient::new(mock);
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        c.transport.insert(
            c.build_mrcalc_url(48.0, 11.0, &s),
            HttpResponse {
                status: 200,
                body: "not json".into(),
            },
        );
        assert!(matches!(
            c.monthly(48.0, 11.0, &s).await,
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn radiation_database_query_strings() {
        assert_eq!(PvGisRadiationDatabase::Sarah2.as_query_value(), "PVGIS-SARAH2");
        assert_eq!(PvGisRadiationDatabase::Sarah3.as_query_value(), "PVGIS-SARAH3");
        assert_eq!(PvGisRadiationDatabase::Era5.as_query_value(), "PVGIS-ERA5");
    }
}
