//! The solar panel array on the roof, and how much power it makes.
//!
//! A validated description of a household PV array — its peak power, the way
//! the panels are tilted and which way they face, and a single derate factor
//! for everything between the panels and the meter (inverter losses, wiring,
//! soiling, temperature). From that plus the sun's position and the irradiance
//! model we project the **plane-of-array** irradiance (the sunlight actually
//! landing on the tilted panels) and turn it into instantaneous AC power.
//!
//! The plane-of-array geometry is the standard beam-on-tilted-surface relation
//! (Duffie & Beckman, public-domain solar-engineering text): the cosine of the
//! angle of incidence between the sun and the panel normal. No copyleft source
//! is read.

use crate::irradiance;
use crate::sun_position::SunPosition;
use core::f64::consts::PI;

/// Reference plane-of-array irradiance the kWp rating is defined at: Standard
/// Test Conditions use 1000 W/m².
pub const STC_IRRADIANCE_W_M2: f64 = 1000.0;

/// Why a [`PvArray`] could not be configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayError {
    /// Peak power must be finite and at least zero.
    BadPeakPower,
    /// Tilt must be between 0° (flat) and 90° (vertical).
    BadTilt,
    /// The facing direction must be a compass bearing in 0..=360°.
    BadAzimuth,
    /// The system derate must be a fraction in 0..=1.
    BadDerate,
}

impl core::fmt::Display for ArrayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::BadPeakPower => "panel peak power must be zero or more",
            Self::BadTilt => "panel tilt must be between flat and upright",
            Self::BadAzimuth => "panel facing must be a compass direction",
            Self::BadDerate => "system efficiency must be a fraction up to one",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for ArrayError {}

/// A household solar panel array.
///
/// Construct with [`PvArray::new`]; the fields are validated up front so the
/// power model never has to defend against impossible geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvArray {
    peak_power_kw: f64,
    tilt_deg: f64,
    azimuth_deg: f64,
    system_derate: f64,
}

impl PvArray {
    /// Configure an array.
    ///
    /// - `peak_power_kw` — the nameplate peak power in kilowatts (kWp), ≥ 0.
    /// - `tilt_deg` — panel tilt from horizontal, 0° (flat) to 90° (vertical).
    /// - `azimuth_deg` — the compass direction the panels face, 0..=360°
    ///   (180° = due south, the best in the northern hemisphere).
    /// - `system_derate` — overall efficiency from panel to meter, 0..=1
    ///   (e.g. 0.85 keeps 85% after inverter, wiring and soiling losses).
    ///
    /// # Errors
    /// Returns [`ArrayError`] when any value is non-finite or out of range.
    pub fn new(
        peak_power_kw: f64,
        tilt_deg: f64,
        azimuth_deg: f64,
        system_derate: f64,
    ) -> Result<Self, ArrayError> {
        if !peak_power_kw.is_finite() || peak_power_kw < 0.0 {
            return Err(ArrayError::BadPeakPower);
        }
        if !tilt_deg.is_finite() || !(0.0..=90.0).contains(&tilt_deg) {
            return Err(ArrayError::BadTilt);
        }
        if !azimuth_deg.is_finite() || !(0.0..=360.0).contains(&azimuth_deg) {
            return Err(ArrayError::BadAzimuth);
        }
        if !system_derate.is_finite() || !(0.0..=1.0).contains(&system_derate) {
            return Err(ArrayError::BadDerate);
        }
        Ok(Self { peak_power_kw, tilt_deg, azimuth_deg, system_derate })
    }

    #[must_use]
    pub const fn peak_power_kw(self) -> f64 {
        self.peak_power_kw
    }

    #[must_use]
    pub const fn tilt_deg(self) -> f64 {
        self.tilt_deg
    }

    #[must_use]
    pub const fn azimuth_deg(self) -> f64 {
        self.azimuth_deg
    }

    #[must_use]
    pub const fn system_derate(self) -> f64 {
        self.system_derate
    }

    /// The cosine of the angle of incidence between the sun and the panel
    /// normal, clamped to `0..=1` (light hitting the back of the panel counts
    /// as zero).
    ///
    /// This is the geometric factor that turns direct-normal irradiance into
    /// the beam component landing on the tilted panel.
    #[must_use]
    pub fn cos_incidence(self, sun: SunPosition) -> f64 {
        let elev = sun.elevation_deg * PI / 180.0;
        let tilt = self.tilt_deg * PI / 180.0;
        // Difference between sun azimuth and panel azimuth.
        let d_az = (sun.azimuth_deg - self.azimuth_deg) * PI / 180.0;
        // cosθ = sin(elev)cos(tilt) + cos(elev)sin(tilt)cos(Δaz).
        let cos_theta =
            elev.sin() * tilt.cos() + elev.cos() * tilt.sin() * d_az.cos();
        cos_theta.clamp(0.0, 1.0)
    }

    /// Plane-of-array irradiance on the tilted panels, W/m², for a sun position
    /// and a clear-sky transmittance.
    ///
    /// Beam component is `DNI · cos(incidence)`; we add the same modest diffuse
    /// fraction the horizontal model uses so a steeply tilted or away-facing
    /// panel still collects sky light. Zero at night.
    #[must_use]
    pub fn plane_of_array_w_m2(
        self,
        sun: SunPosition,
        day_of_year: u16,
        transmittance: f64,
    ) -> f64 {
        if !sun.is_daylight() {
            return 0.0;
        }
        let dni = irradiance::clear_sky_dni_w_m2(sun.elevation_deg, day_of_year, transmittance);
        let beam = dni * self.cos_incidence(sun);
        // Isotropic diffuse: a slice of the sky dome seen by the tilted panel.
        let tilt = self.tilt_deg * PI / 180.0;
        let sky_view = f64::midpoint(1.0, tilt.cos());
        let diffuse = dni * 0.10 * sky_view;
        beam + diffuse
    }

    /// Instantaneous AC power output in kilowatts for a plane-of-array
    /// irradiance.
    ///
    /// PV output scales linearly with irradiance relative to the 1000 W/m²
    /// Standard Test Conditions the kWp rating is defined at, times the system
    /// derate. A `cloud_cover` fraction (0 = clear, 1 = overcast) scales it
    /// further via the diffuse-floor cloud model.
    #[must_use]
    pub fn ac_power_kw(self, plane_of_array_w_m2: f64, cloud_cover: f64) -> f64 {
        let after_cloud = irradiance::cloud_derate(plane_of_array_w_m2, cloud_cover);
        let fraction_of_stc = after_cloud / STC_IRRADIANCE_W_M2;
        (self.peak_power_kw * fraction_of_stc * self.system_derate).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun_position::sun_position_at;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "expected {a} ≈ {b} (within {tol})");
    }

    fn south_array() -> PvArray {
        // 6 kWp, 30° tilt, due south, 85% system efficiency — a typical roof.
        PvArray::new(6.0, 30.0, 180.0, 0.85).expect("valid array")
    }

    #[test]
    fn rejects_negative_peak_power() {
        assert_eq!(PvArray::new(-1.0, 30.0, 180.0, 0.85), Err(ArrayError::BadPeakPower));
    }

    #[test]
    fn rejects_nonfinite_peak_power() {
        assert_eq!(PvArray::new(f64::NAN, 30.0, 180.0, 0.85), Err(ArrayError::BadPeakPower));
    }

    #[test]
    fn rejects_out_of_range_tilt() {
        assert_eq!(PvArray::new(6.0, -1.0, 180.0, 0.85), Err(ArrayError::BadTilt));
        assert_eq!(PvArray::new(6.0, 91.0, 180.0, 0.85), Err(ArrayError::BadTilt));
    }

    #[test]
    fn rejects_out_of_range_azimuth() {
        assert_eq!(PvArray::new(6.0, 30.0, -10.0, 0.85), Err(ArrayError::BadAzimuth));
        assert_eq!(PvArray::new(6.0, 30.0, 361.0, 0.85), Err(ArrayError::BadAzimuth));
    }

    #[test]
    fn rejects_out_of_range_derate() {
        assert_eq!(PvArray::new(6.0, 30.0, 180.0, -0.1), Err(ArrayError::BadDerate));
        assert_eq!(PvArray::new(6.0, 30.0, 180.0, 1.5), Err(ArrayError::BadDerate));
    }

    #[test]
    fn accepts_boundary_values() {
        assert!(PvArray::new(0.0, 0.0, 0.0, 0.0).is_ok());
        assert!(PvArray::new(20.0, 90.0, 360.0, 1.0).is_ok());
    }

    #[test]
    fn power_is_zero_at_night() {
        let arr = south_array();
        let midnight = sun_position_at(49.7, 172, 0.0);
        let poa = arr.plane_of_array_w_m2(midnight, 172, 0.75);
        assert_eq!(poa, 0.0);
        assert_eq!(arr.ac_power_kw(poa, 0.0), 0.0);
    }

    #[test]
    fn power_scales_linearly_with_peak_power() {
        let small = PvArray::new(3.0, 30.0, 180.0, 0.85).unwrap();
        let big = PvArray::new(9.0, 30.0, 180.0, 0.85).unwrap();
        // Same irradiance: 3× the kWp makes 3× the power.
        close(big.ac_power_kw(800.0, 0.0), 3.0 * small.ac_power_kw(800.0, 0.0), 1e-9);
    }

    #[test]
    fn power_scales_with_system_derate() {
        let lossy = PvArray::new(6.0, 30.0, 180.0, 0.70).unwrap();
        let efficient = PvArray::new(6.0, 30.0, 180.0, 0.90).unwrap();
        assert!(efficient.ac_power_kw(800.0, 0.0) > lossy.ac_power_kw(800.0, 0.0));
    }

    #[test]
    fn cloud_cover_reduces_power() {
        let arr = south_array();
        let clear = arr.ac_power_kw(800.0, 0.0);
        let overcast = arr.ac_power_kw(800.0, 1.0);
        assert!(overcast < clear, "overcast {overcast} should be below clear {clear}");
        // Overcast retains the ~25% diffuse floor.
        close(overcast, clear * 0.25, 1e-9);
    }

    #[test]
    fn south_panel_beats_north_panel_in_north() {
        let south = PvArray::new(6.0, 30.0, 180.0, 0.85).unwrap();
        let north = PvArray::new(6.0, 30.0, 0.0, 0.85).unwrap();
        let noon = sun_position_at(49.7, 172, 12.0);
        let s = south.plane_of_array_w_m2(noon, 172, 0.75);
        let n = north.plane_of_array_w_m2(noon, 172, 0.75);
        assert!(s > n, "south {s} should beat north {n} at noon");
    }

    #[test]
    fn cos_incidence_clamped_to_unit_range() {
        let arr = south_array();
        let noon = sun_position_at(49.7, 172, 12.0);
        let c = arr.cos_incidence(noon);
        assert!((0.0..=1.0).contains(&c), "cosθ {c} out of range");
    }

    #[test]
    fn poa_peaks_near_solar_noon() {
        let arr = south_array();
        let morning = arr.plane_of_array_w_m2(sun_position_at(49.7, 172, 8.0), 172, 0.75);
        let noon = arr.plane_of_array_w_m2(sun_position_at(49.7, 172, 12.0), 172, 0.75);
        assert!(noon > morning, "noon {noon} should beat morning {morning}");
    }

    #[test]
    fn power_never_exceeds_peak_in_normal_conditions() {
        let arr = south_array();
        // Even at full STC irradiance, output is capped by derate × kWp.
        let p = arr.ac_power_kw(STC_IRRADIANCE_W_M2, 0.0);
        close(p, 6.0 * 0.85, 1e-9);
        assert!(p <= arr.peak_power_kw());
    }
}
