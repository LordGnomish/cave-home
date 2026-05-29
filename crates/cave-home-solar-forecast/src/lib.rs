// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-solar-forecast` — a first-principles solar PV production forecast
//! engine for cave-home.
//!
//! This crate is the **brain** that turns a house's location, its roof's panel
//! array and the day's cloudiness into a production forecast a household can
//! act on: how much power the panels are making right now, how much energy to
//! expect across the day, and when solar peaks — all phrased for the household,
//! never in engineering units.
//!
//! It is built entirely from public-domain astronomy and solar-engineering
//! math (the NOAA Solar Calculator equations, Spencer's 1971 declination /
//! equation-of-time series, Kasten & Young's air mass, and the textbook
//! clear-sky and plane-of-array relations). Nothing here reads the network or a
//! clock, and nothing depends on a copyleft library — the caller supplies the
//! day-of-year and the time of day, so the engine is fully deterministic and
//! testable against published reference values (Charter §7 always-latest, §9
//! OSS-first, ADR-002 Apache-2.0-only).
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`sun_position`] — solar declination, hour angle, elevation and azimuth
//!   from latitude, day-of-year and solar time (NOAA / Spencer formulas).
//! - [`irradiance`] — extraterrestrial + clear-sky beam/global irradiance and
//!   the cloud-cover derate.
//! - [`array`] — the validated panel-array model and the plane-of-array →
//!   AC-power conversion.
//! - [`forecast`] — the daily energy integral, the instantaneous reading and
//!   all input validation.
//! - [`label`] — the grandma-friendly EN / DE / TR summaries (Charter §6.3,
//!   ADR-007).
//!
//! The **live weather / forecast feeds** (Forecast.Solar, Solcast, Open-Meteo,
//! DWD), horizon/shading profiles, and cave-home-core / cave-home-history
//! integration are network-bound and deferred to Phase 1b — each is enumerated
//! in `parity.manifest.toml` `[[unmapped]]` with a disposition. They feed a
//! cloud-cover (or clearness) fraction into this engine and otherwise reuse it
//! unchanged.
//!
//! # Example
//!
//! ```
//! use cave_home_solar_forecast::{forecast_day, daily_summary, Lang, PvArray, Site};
//!
//! // A house near Iphofen (~49.7°N, 10.3°E) with a 6 kWp south-facing roof,
//! // 30° tilt, 85% system efficiency.
//! let site = Site::new(49.7, 10.3).unwrap();
//! let array = PvArray::new(6.0, 30.0, 180.0, 0.85).unwrap();
//!
//! // The June solstice (day 172), clear sky, sampled every 15 minutes.
//! let day = forecast_day(site, array, 172, 0.0, 0.25).unwrap();
//! assert!(day.energy_kwh > 0.0);
//!
//! // The household sees a plain-language line, never a W/m² figure.
//! let summary = daily_summary(&day, Lang::En);
//! assert!(summary.contains("Sunny"));
//! ```

// The astronomy/irradiance polynomial series (Spencer 1971, the orbital
// eccentricity correction, etc.) are written in their published a + b·cos +
// c·sin… form so each term can be checked term-by-term against the reference.
// Folding them into nested `mul_add` calls would obscure that verifiability for
// no meaningful accuracy gain at f64, so the FLOP-shape lint is allowed here.
#![allow(clippy::suboptimal_flops)]

pub mod array;
pub mod forecast;
pub mod irradiance;
pub mod label;
pub mod sun_position;

pub use array::{ArrayError, PvArray};
pub use forecast::{forecast_day, instant_at, DailyForecast, ForecastError, Instant, Site};
pub use label::{daily_summary, peak_time_phrase, Lang, SolarOutlook};
pub use sun_position::{daylight_hours, sun_position_at, SunPosition};
