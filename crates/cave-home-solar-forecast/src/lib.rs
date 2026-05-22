// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-forecast` — solar production forecasting via the
//! public [Forecast.Solar](https://doc.forecast.solar/api) and
//! [PVGIS](https://re.jrc.ec.europa.eu/api/v5_2/) REST APIs.
//!
//! Both APIs are public — no upstream source code is read or ported.
//! This crate is a hand-written HTTP client + response parser pair.
//!
//! # Charter §6.3 grandma-friendly UX
//!
//! Output type [`Forecast`] uses home-world fields:
//!
//! * `kwh_today` / `kwh_tomorrow` — energy production estimates
//! * `peak_kw` — peak instantaneous power
//! * `hourly_kwh` — per-hour estimates
//!
//! The CLI / Portal never exposes raw API URLs or PV-system geometry.
//!
//! # Transport abstraction
//!
//! The HTTP client is abstracted behind [`HttpClient`] so unit tests
//! can use [`MockHttpClient`] with pre-canned JSON responses. The
//! production transport (reqwest-based) lives in `cave-home-binary`
//! to keep this crate dependency-light and compileable on macOS dev
//! machines without an OpenSSL toolchain.

#![allow(clippy::module_name_repetitions)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

pub mod error;
pub mod forecast_solar;
pub mod http;
pub mod pvgis;
pub mod site;
pub mod summary;

pub use error::{Error, Result};
pub use forecast_solar::{ForecastSolarClient, ForecastSolarEstimate, ForecastSolarTier};
pub use http::{HttpClient, HttpRequest, HttpResponse, MockHttpClient};
pub use pvgis::{PvGisClient, PvGisMonthly, PvGisMounting, PvGisRadiationDatabase};
pub use site::{PvSite, PvString};
pub use summary::Forecast;

/// Forecast.Solar public base URL. Source:
/// <https://doc.forecast.solar/api>.
pub const FORECAST_SOLAR_BASE_URL: &str = "https://api.forecast.solar";

/// PVGIS v5.2 public base URL. Source: <https://re.jrc.ec.europa.eu/api/v5_2/>.
pub const PVGIS_BASE_URL: &str = "https://re.jrc.ec.europa.eu/api/v5_2";
