// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! PV site / string definitions used as inputs to the forecast APIs.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// One PV string — the unit cave-home sends per Forecast.Solar
/// /estimate call. A multi-MPPT inverter can declare multiple strings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PvString {
    /// Tilt angle of the panels, 0 (flat) to 90 (vertical), degrees.
    pub tilt_deg: f64,
    /// Azimuth in degrees, -180..=180. 0 == south (Forecast.Solar
    /// convention); negative ⇒ east of south, positive ⇒ west of south.
    pub azimuth_deg: f64,
    /// Peak power in kWp (DC nameplate).
    pub peak_kwp: f64,
}

impl PvString {
    /// # Errors
    ///
    /// Returns [`Error::InvalidOrientation`] if tilt/azimuth are out
    /// of range, or [`Error::InvalidPeakPower`] for non-positive
    /// peak power.
    pub fn new(tilt_deg: f64, azimuth_deg: f64, peak_kwp: f64) -> Result<Self> {
        let me = Self {
            tilt_deg,
            azimuth_deg,
            peak_kwp,
        };
        me.validate()?;
        Ok(me)
    }

    pub fn validate(&self) -> Result<()> {
        if !(0.0..=90.0).contains(&self.tilt_deg) || !(-180.0..=180.0).contains(&self.azimuth_deg) {
            return Err(Error::InvalidOrientation {
                tilt: self.tilt_deg,
                azimuth: self.azimuth_deg,
            });
        }
        if self.peak_kwp <= 0.0 {
            return Err(Error::InvalidPeakPower(self.peak_kwp));
        }
        Ok(())
    }
}

/// PV site — coordinates + 1..N strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PvSite {
    pub latitude: f64,
    pub longitude: f64,
    pub strings: Vec<PvString>,
}

impl PvSite {
    /// # Errors
    ///
    /// Returns [`Error::InvalidCoordinates`] if lat/lon fall outside
    /// the legal ranges.
    pub fn new(latitude: f64, longitude: f64, strings: Vec<PvString>) -> Result<Self> {
        let me = Self {
            latitude,
            longitude,
            strings,
        };
        me.validate()?;
        Ok(me)
    }

    pub fn validate(&self) -> Result<()> {
        if !(-90.0..=90.0).contains(&self.latitude) || !(-180.0..=180.0).contains(&self.longitude) {
            return Err(Error::InvalidCoordinates {
                lat: self.latitude,
                lon: self.longitude,
            });
        }
        for s in &self.strings {
            s.validate()?;
        }
        Ok(())
    }

    /// Total peak-power across all strings, kWp.
    #[must_use]
    pub fn total_peak_kwp(&self) -> f64 {
        self.strings.iter().map(|s| s.peak_kwp).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pv_string_valid_construction() {
        let s = PvString::new(30.0, 0.0, 8.2).unwrap();
        assert_eq!(s.tilt_deg, 30.0);
    }

    #[test]
    fn pv_string_tilt_out_of_range() {
        assert!(matches!(
            PvString::new(95.0, 0.0, 8.2),
            Err(Error::InvalidOrientation { .. })
        ));
    }

    #[test]
    fn pv_string_azimuth_out_of_range() {
        assert!(matches!(
            PvString::new(30.0, 200.0, 8.2),
            Err(Error::InvalidOrientation { .. })
        ));
    }

    #[test]
    fn pv_string_zero_peak_rejected() {
        assert!(matches!(
            PvString::new(30.0, 0.0, 0.0),
            Err(Error::InvalidPeakPower(_))
        ));
    }

    #[test]
    fn pv_site_lat_out_of_range() {
        assert!(matches!(
            PvSite::new(91.0, 10.0, vec![]),
            Err(Error::InvalidCoordinates { .. })
        ));
    }

    #[test]
    fn pv_site_total_peak_sums_strings() {
        let s1 = PvString::new(30.0, 0.0, 4.0).unwrap();
        let s2 = PvString::new(30.0, 90.0, 3.5).unwrap();
        let site = PvSite::new(48.0, 11.0, vec![s1, s2]).unwrap();
        assert!((site.total_peak_kwp() - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn pv_site_invalid_string_rejected() {
        let bad = PvString {
            tilt_deg: 200.0,
            azimuth_deg: 0.0,
            peak_kwp: 4.0,
        };
        assert!(PvSite::new(48.0, 11.0, vec![bad]).is_err());
    }
}
