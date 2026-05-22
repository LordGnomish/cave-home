// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    /// Site coordinates were out of the legal range. Forecast.Solar
    /// rejects latitudes outside ±90° and longitudes outside ±180°.
    #[error("coordinates ({lat:.3}, {lon:.3}) are out of range")]
    InvalidCoordinates { lat: f64, lon: f64 },

    /// PV panel tilt outside `0..=90`° or azimuth outside `-180..=180`°.
    #[error("orientation: tilt {tilt}° / azimuth {azimuth}° is out of range")]
    InvalidOrientation { tilt: f64, azimuth: f64 },

    /// Declared peak power was non-positive.
    #[error("declared peak power {0} kWp is not positive")]
    InvalidPeakPower(f64),

    /// Transport returned a non-2xx status code.
    #[error("upstream HTTP error {status}: {body}")]
    HttpStatus { status: u16, body: String },

    /// JSON body could not be parsed in the upstream-documented shape.
    #[error("malformed upstream response: {0}")]
    Malformed(String),

    /// Forecast.Solar quota or tier-related rejection (HTTP 429).
    #[error("forecast.solar quota exhausted (HTTP 429)")]
    QuotaExhausted,

    /// PVGIS API returned an empty hourly series for the requested
    /// window.
    #[error("PVGIS returned no monthly data for the requested period")]
    PvGisEmpty,

    /// Generic transport error (network down, TLS handshake failure,
    /// timeout). Stringified to keep the crate transport-free.
    #[error("transport error: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_coordinates_message() {
        let e = Error::InvalidCoordinates { lat: 91.0, lon: 0.0 };
        assert!(e.to_string().contains("91"));
    }

    #[test]
    fn http_status_message() {
        let e = Error::HttpStatus {
            status: 503,
            body: "upstream down".into(),
        };
        assert!(e.to_string().contains("503"));
    }

    #[test]
    fn quota_message_stable() {
        assert_eq!(
            Error::QuotaExhausted.to_string(),
            "forecast.solar quota exhausted (HTTP 429)"
        );
    }
}
