//! How much sunlight reaches the ground on a clear day.
//!
//! A simple, well-known clear-sky model:
//! - the **extraterrestrial irradiance** (the sunlight arriving at the top of
//!   the atmosphere) varies a little over the year as the Earth-Sun distance
//!   changes — the standard ±3.3% eccentricity correction on the solar
//!   constant of 1361 W/m².
//! - that beam is attenuated through the air it passes on the way down, which
//!   grows with the **air mass** (the slant path length, ≈ 1/sin(elevation)).
//!   We use Kasten & Young's air-mass formula and a single broadband
//!   atmospheric transmittance, the textbook clear-sky simplification.
//!
//! All of this is public-domain solar-engineering math (Spencer 1971 for the
//! eccentricity factor; Kasten & Young 1989 for the air mass; the broadband
//! transmittance is the standard Meinel/Laue form). No copyleft source is read.
//!
//! Irradiance is in watts per square metre (W/m²). The numbers are kept inside
//! the engine — the household never sees a W/m² figure (Charter §6.3).

use core::f64::consts::PI;

/// The solar constant: mean extraterrestrial irradiance on a surface normal to
/// the sun's rays, W/m² (WMO/ASTM ≈ 1361 W/m²).
pub const SOLAR_CONSTANT_W_M2: f64 = 1361.0;

/// Default broadband clear-sky atmospheric transmittance at zenith (sea level,
/// average aerosol). A dimensionless 0..1 factor: at the zenith roughly 75% of
/// the beam survives one air mass on a clear day.
pub const DEFAULT_CLEAR_SKY_TRANSMITTANCE: f64 = 0.75;

/// Extraterrestrial normal irradiance for a day of the year (W/m²).
///
/// The solar constant scaled by the Earth's orbital eccentricity correction
/// (Spencer 1971): about +3.4% near the January perihelion and −3.4% near the
/// July aphelion.
#[must_use]
pub fn extraterrestrial_normal_w_m2(day_of_year: u16) -> f64 {
    let g = 2.0 * PI * (f64::from(day_of_year) - 1.0) / 365.0;
    let eccentricity = 1.000_110 + 0.034_221 * g.cos() + 0.001_280 * g.sin()
        + 0.000_719 * (2.0 * g).cos()
        + 0.000_077 * (2.0 * g).sin();
    SOLAR_CONSTANT_W_M2 * eccentricity
}

/// Relative optical air mass for a solar elevation (degrees), Kasten & Young
/// (1989).
///
/// 1.0 at the zenith, ≈ 2 at 30° elevation, growing rapidly near the horizon.
/// Returns `None` when the sun is at or below the horizon (no direct path).
#[must_use]
pub fn air_mass(elevation_deg: f64) -> Option<f64> {
    if elevation_deg <= 0.0 {
        return None;
    }
    // Kasten & Young: 1 / ( sin h + 0.50572 (h + 6.07995)^-1.6364 ), h in deg.
    let denom =
        (elevation_deg * PI / 180.0).sin() + 0.505_72 * (elevation_deg + 6.079_95).powf(-1.636_4);
    Some(1.0 / denom)
}

/// Direct (beam) normal irradiance on a clear day, W/m² — the irradiance on a
/// surface pointed straight at the sun.
///
/// `transmittance` is the at-zenith clear-sky transmittance (use
/// [`DEFAULT_CLEAR_SKY_TRANSMITTANCE`]); it is raised to the air mass to
/// account for the longer slant path at low sun. Zero at night.
#[must_use]
pub fn clear_sky_dni_w_m2(elevation_deg: f64, day_of_year: u16, transmittance: f64) -> f64 {
    air_mass(elevation_deg).map_or(0.0, |am| {
        let t = transmittance.clamp(0.0, 1.0);
        extraterrestrial_normal_w_m2(day_of_year) * t.powf(am)
    })
}

/// Global horizontal irradiance on a clear day, W/m² — the total sunlight on a
/// flat, level surface.
///
/// A simple beam + diffuse split: the beam contributes
/// `DNI · sin(elevation)`, and we add a modest diffuse fraction (about 10% of
/// the beam horizontal component) so a horizontal panel still sees some light
/// from the whole sky dome. Zero at night.
#[must_use]
pub fn clear_sky_ghi_w_m2(elevation_deg: f64, day_of_year: u16, transmittance: f64) -> f64 {
    if elevation_deg <= 0.0 {
        return 0.0;
    }
    let dni = clear_sky_dni_w_m2(elevation_deg, day_of_year, transmittance);
    let beam_horizontal = dni * (elevation_deg * PI / 180.0).sin();
    // Small diffuse-sky contribution on top of the beam.
    beam_horizontal * 1.10
}

/// Scale a clear-sky irradiance by a cloud-cover fraction (0 = clear sky,
/// 1 = fully overcast).
///
/// Even under full overcast a panel still receives diffuse light, so the
/// derate does not go to zero. We use the common linear cloud-cover model
/// `(1 − 0.75 · cover³)` (Kasten-Czeplak form): a clear sky passes everything,
/// thin cloud costs little, and a fully overcast sky still leaves about 25% of
/// the clear-sky energy as diffuse light.
///
/// `cloud_cover` outside `0..=1` is clamped.
#[must_use]
pub fn cloud_derate(clear_sky_w_m2: f64, cloud_cover: f64) -> f64 {
    let c = cloud_cover.clamp(0.0, 1.0);
    clear_sky_w_m2 * (1.0 - 0.75 * c.powi(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "expected {a} ≈ {b} (within {tol})");
    }

    #[test]
    fn extraterrestrial_tracks_earth_sun_distance() {
        // Strongest near the January perihelion (~day 3), weakest near the July
        // aphelion (~day 185). Both within a few percent of the solar constant.
        let jan = extraterrestrial_normal_w_m2(3);
        let jul = extraterrestrial_normal_w_m2(185);
        assert!(jan > SOLAR_CONSTANT_W_M2, "perihelion {jan} should exceed constant");
        assert!(jul < SOLAR_CONSTANT_W_M2, "aphelion {jul} should be below constant");
        // The full swing is about 6.8% peak-to-peak.
        close(jan, 1361.0 * 1.034, 3.0);
        close(jul, 1361.0 * 0.966, 3.0);
    }

    #[test]
    fn air_mass_is_one_at_zenith() {
        // Straight overhead: one atmosphere thickness.
        close(air_mass(90.0).unwrap(), 1.0, 0.01);
    }

    #[test]
    fn air_mass_grows_toward_horizon() {
        let high = air_mass(60.0).unwrap();
        let mid = air_mass(30.0).unwrap();
        let low = air_mass(5.0).unwrap();
        assert!(high < mid && mid < low, "{high} < {mid} < {low}");
        // ~30° elevation is close to 2 air masses.
        close(mid, 2.0, 0.05);
        // Near the horizon the path is dozens of air masses.
        assert!(low > 10.0, "low-sun air mass {low}");
    }

    #[test]
    fn no_air_mass_below_horizon() {
        assert!(air_mass(0.0).is_none());
        assert!(air_mass(-10.0).is_none());
    }

    #[test]
    fn dni_is_zero_at_night() {
        assert_eq!(clear_sky_dni_w_m2(-5.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE), 0.0);
        assert_eq!(clear_sky_dni_w_m2(0.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE), 0.0);
    }

    #[test]
    fn ghi_is_zero_at_night() {
        assert_eq!(clear_sky_ghi_w_m2(-1.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE), 0.0);
    }

    #[test]
    fn high_sun_clear_sky_is_in_realistic_range() {
        // High summer sun, clear sky: GHI in the ~900-1050 W/m² ballpark that a
        // ground station actually records around solar noon.
        let ghi = clear_sky_ghi_w_m2(65.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE);
        assert!((850.0..=1100.0).contains(&ghi), "noon GHI {ghi}");
    }

    #[test]
    fn ghi_increases_with_sun_elevation() {
        let low = clear_sky_ghi_w_m2(10.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE);
        let mid = clear_sky_ghi_w_m2(35.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE);
        let high = clear_sky_ghi_w_m2(65.0, 172, DEFAULT_CLEAR_SKY_TRANSMITTANCE);
        assert!(low < mid && mid < high, "{low} < {mid} < {high}");
    }

    #[test]
    fn clearer_atmosphere_passes_more_light() {
        let hazy = clear_sky_dni_w_m2(45.0, 172, 0.6);
        let clear = clear_sky_dni_w_m2(45.0, 172, 0.8);
        assert!(clear > hazy, "clear {clear} should beat hazy {hazy}");
    }

    #[test]
    fn cloud_derate_clear_sky_is_unchanged() {
        close(cloud_derate(900.0, 0.0), 900.0, 1e-9);
    }

    #[test]
    fn cloud_derate_overcast_leaves_diffuse_floor() {
        // Full overcast: about 25% of clear-sky energy survives as diffuse.
        close(cloud_derate(900.0, 1.0), 900.0 * 0.25, 1e-9);
    }

    #[test]
    fn cloud_derate_is_monotonic_and_clamped() {
        let clear = cloud_derate(900.0, 0.0);
        let half = cloud_derate(900.0, 0.5);
        let full = cloud_derate(900.0, 1.0);
        assert!(full < half && half < clear, "{full} < {half} < {clear}");
        // Out-of-range cover is clamped, not panicked on.
        assert_eq!(cloud_derate(900.0, -1.0), clear);
        assert_eq!(cloud_derate(900.0, 2.0), full);
    }
}
