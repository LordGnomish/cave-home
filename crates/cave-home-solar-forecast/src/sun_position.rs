//! Where the sun is in the sky — pure first-principles astronomy.
//!
//! Implemented from the **public-domain NOAA Solar Calculator** equations
//! (the spreadsheet-form approximations NOAA publishes for the position of the
//! sun) together with Spencer's well-known Fourier series for the solar
//! declination and the equation of time. These are closed-form approximations
//! that are accurate to a fraction of a degree for terrestrial PV use — far
//! more than a household solar forecast needs — and they are entirely public
//! domain, so no copyleft library is read or ported (Charter §9, ADR-002).
//!
//! Everything here is deterministic: the caller supplies the day-of-year and
//! the time of day, so the engine has no clock and no time-zone database. All
//! angles are in degrees on the public API; radians are an internal detail.
//!
//! The geometry follows the standard convention used across solar engineering:
//! - **declination** δ — the latitude at which the sun is overhead, swinging
//!   between ±23.44° over the year.
//! - **hour angle** H — the sun's angular distance from solar noon, 15° per
//!   hour, negative in the morning and positive in the afternoon.
//! - **elevation** (altitude) — how high the sun is above the horizon, 0° at
//!   the horizon and 90° straight overhead.
//! - **azimuth** — the compass direction of the sun, measured clockwise from
//!   true north (0° = north, 90° = east, 180° = south, 270° = west).

use core::f64::consts::PI;

/// Obliquity of the ecliptic — the maximum solar declination, ±23.44°.
pub const MAX_DECLINATION_DEG: f64 = 23.44;

/// Degrees of hour angle swept per hour of solar time (the Earth turns 360°
/// in 24 hours).
pub const DEGREES_PER_HOUR: f64 = 15.0;

const DEG_PER_RAD: f64 = 180.0 / PI;
const RAD_PER_DEG: f64 = PI / 180.0;

const fn to_rad(deg: f64) -> f64 {
    deg * RAD_PER_DEG
}

const fn to_deg(rad: f64) -> f64 {
    rad * DEG_PER_RAD
}

/// The fractional day-angle Γ (in radians) for a day of the year, used by
/// Spencer's series. Day 1 is January 1st.
fn day_angle(day_of_year: u16) -> f64 {
    // Spencer (1971): Γ = 2π (n − 1) / 365.
    2.0 * PI * (f64::from(day_of_year) - 1.0) / 365.0
}

/// Solar declination δ in degrees for a day of the year, from Spencer's
/// truncated Fourier series.
///
/// Spencer (1971), as reproduced in Iqbal, *An Introduction to Solar
/// Radiation*. Accurate to about 0.01° — well inside what a household forecast
/// needs. Positive in the northern summer, negative in the northern winter.
#[must_use]
pub fn declination_deg(day_of_year: u16) -> f64 {
    let g = day_angle(day_of_year);
    // δ (radians) = 0.006918 − 0.399912 cosΓ + 0.070257 sinΓ
    //               − 0.006758 cos2Γ + 0.000907 sin2Γ
    //               − 0.002697 cos3Γ + 0.001480 sin3Γ
    let dec_rad = 0.006_918 - 0.399_912 * g.cos() + 0.070_257 * g.sin()
        - 0.006_758 * (2.0 * g).cos()
        + 0.000_907 * (2.0 * g).sin()
        - 0.002_697 * (3.0 * g).cos()
        + 0.001_480 * (3.0 * g).sin();
    to_deg(dec_rad)
}

/// The equation of time (in minutes) for a day of the year — the difference
/// between apparent solar time and mean (clock) time caused by the Earth's
/// elliptical orbit and axial tilt.
///
/// Spencer (1971). Positive means the sundial is ahead of the clock.
#[must_use]
pub fn equation_of_time_min(day_of_year: u16) -> f64 {
    let g = day_angle(day_of_year);
    // EoT (radians) Fourier form, converted to minutes via 229.18.
    let eot_rad = 0.000_075 + 0.001_868 * g.cos() - 0.032_077 * g.sin()
        - 0.014_615 * (2.0 * g).cos()
        - 0.040_849 * (2.0 * g).sin();
    229.18 * eot_rad
}

/// The hour angle H in degrees for a given *true solar time* (in hours, where
/// 12.0 is solar noon).
///
/// Negative before noon, zero at noon, positive after noon: 15° per hour.
#[must_use]
pub fn hour_angle_deg(true_solar_time_h: f64) -> f64 {
    (true_solar_time_h - 12.0) * DEGREES_PER_HOUR
}

/// Convert a UTC clock time and a site longitude into the *true solar time*
/// (in hours) at that site, applying the equation of time.
///
/// Longitude is degrees east of Greenwich (negative for the western
/// hemisphere). This lets a caller who only has wall-clock UTC and a location
/// drive the rest of the engine without a time-zone database.
#[must_use]
pub fn true_solar_time_h(utc_hour: f64, longitude_deg: f64, day_of_year: u16) -> f64 {
    // Mean solar time advances 4 minutes per degree of longitude east; convert
    // that and the equation of time (minutes) into an hours offset from UTC.
    let offset_h = (longitude_deg * 4.0 + equation_of_time_min(day_of_year)) / 60.0;
    let t = utc_hour + offset_h;
    // Wrap into [0, 24) so callers can pass any UTC hour.
    t.rem_euclid(24.0)
}

/// Where the sun is: its elevation above the horizon and its compass azimuth,
/// both in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPosition {
    /// Elevation (altitude) above the horizon, degrees. Negative when the sun
    /// is below the horizon (night).
    pub elevation_deg: f64,
    /// Compass azimuth, degrees clockwise from true north
    /// (0 = N, 90 = E, 180 = S, 270 = W).
    pub azimuth_deg: f64,
}

impl SunPosition {
    /// Whether the sun is above the horizon (daytime) for this position.
    #[must_use]
    pub fn is_daylight(self) -> bool {
        self.elevation_deg > 0.0
    }
}

/// Compute the sun's position from latitude, declination and hour angle (all
/// in degrees).
///
/// This is the core spherical-astronomy step; callers usually reach it through
/// [`sun_position_at`]. Latitude is degrees north (negative south); the hour
/// angle comes from [`hour_angle_deg`].
#[must_use]
pub fn position_from_angles(latitude_deg: f64, declination_deg: f64, hour_angle_deg: f64) -> SunPosition {
    let lat = to_rad(latitude_deg);
    let dec = to_rad(declination_deg);
    let ha = to_rad(hour_angle_deg);

    // Elevation: sin(elev) = sinφ sinδ + cosφ cosδ cosH.
    let sin_elev =
        lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos();
    let sin_elev = sin_elev.clamp(-1.0, 1.0);
    let elev = sin_elev.asin();

    // Azimuth (from north, clockwise). Use the atan2 form that is stable at
    // the horizon and through the poles of the standard cos-azimuth formula.
    //   az = atan2( sinH, cosH sinφ − tanδ cosφ )
    // measured from SOUTH; we rotate to a from-NORTH compass bearing below.
    let az_from_south = ha
        .sin()
        .atan2(ha.cos() * lat.sin() - dec.tan() * lat.cos());
    // Convert "clockwise from south" to "clockwise from north" in [0, 360).
    let az_deg = (to_deg(az_from_south) + 180.0).rem_euclid(360.0);

    SunPosition { elevation_deg: to_deg(elev), azimuth_deg: az_deg }
}

/// Compute the sun's position at a site for a true solar time.
///
/// `latitude_deg` is degrees north, `day_of_year` is 1..=366, and
/// `true_solar_time_h` is the local apparent solar time in hours (12.0 = solar
/// noon). For callers who only have UTC + longitude, derive the solar time with
/// [`true_solar_time_h`] first.
#[must_use]
pub fn sun_position_at(latitude_deg: f64, day_of_year: u16, true_solar_time_h: f64) -> SunPosition {
    let dec = declination_deg(day_of_year);
    let ha = hour_angle_deg(true_solar_time_h);
    position_from_angles(latitude_deg, dec, ha)
}

/// The half-day length expressed as an hour angle, in degrees: the magnitude of
/// the hour angle at sunrise/sunset (where geometric elevation crosses 0°).
///
/// Returns `360.0` for the polar day (sun never sets), `0.0` for the polar
/// night (sun never rises), and the sunrise hour angle otherwise. This is the
/// classic `cos(H0) = −tanφ tanδ` relation.
#[must_use]
pub fn sunrise_hour_angle_deg(latitude_deg: f64, day_of_year: u16) -> f64 {
    let lat = to_rad(latitude_deg);
    let dec = to_rad(declination_deg(day_of_year));
    let cos_h0 = -lat.tan() * dec.tan();
    if cos_h0 <= -1.0 {
        360.0 // polar day: sun is up the whole 24h
    } else if cos_h0 >= 1.0 {
        0.0 // polar night: sun never clears the horizon
    } else {
        to_deg(cos_h0.acos())
    }
}

/// Daylight length in hours for a site and day (0..=24).
#[must_use]
pub fn daylight_hours(latitude_deg: f64, day_of_year: u16) -> f64 {
    // The sun is up for 2·H0, and H0 sweeps 15° per hour. The polar-day
    // sentinel (H0 = 360°) means a full 24 hours of daylight.
    (2.0 * sunrise_hour_angle_deg(latitude_deg, day_of_year) / DEGREES_PER_HOUR).min(24.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert two angles are equal within `tol` degrees.
    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "expected {a} ≈ {b} (within {tol})");
    }

    #[test]
    fn declination_is_near_zero_at_equinoxes() {
        // ~20 March (day 79) and ~22/23 September (day 266) the sun crosses the
        // equator: declination ≈ 0°. Spencer's series puts these within ~0.5°.
        close(declination_deg(80), 0.0, 0.6);
        close(declination_deg(266), 0.0, 0.6);
    }

    #[test]
    fn declination_peaks_at_solstices() {
        // June solstice (day ~172): δ ≈ +23.44°. December (day ~355): −23.44°.
        close(declination_deg(172), MAX_DECLINATION_DEG, 0.4);
        close(declination_deg(355), -MAX_DECLINATION_DEG, 0.4);
    }

    #[test]
    fn declination_stays_within_obliquity() {
        for day in 1..=366u16 {
            let d = declination_deg(day);
            assert!(d.abs() <= MAX_DECLINATION_DEG + 0.1, "day {day}: δ={d}");
        }
    }

    #[test]
    fn equation_of_time_is_small_and_bounded() {
        // The equation of time never exceeds ~16.5 minutes in magnitude.
        for day in 1..=366u16 {
            let e = equation_of_time_min(day);
            assert!(e.abs() < 17.0, "day {day}: EoT={e}");
        }
        // Early November the sundial is ~16 min ahead of the clock.
        assert!(equation_of_time_min(307) > 14.0);
        // Mid-February the sundial is ~14 min behind.
        assert!(equation_of_time_min(43) < -12.0);
    }

    #[test]
    fn hour_angle_sign_convention() {
        close(hour_angle_deg(12.0), 0.0, 1e-9); // noon
        close(hour_angle_deg(6.0), -90.0, 1e-9); // morning negative
        close(hour_angle_deg(18.0), 90.0, 1e-9); // afternoon positive
    }

    #[test]
    fn noon_elevation_matches_geometry_at_equator_equinox() {
        // Equator, equinox, solar noon: sun is essentially straight overhead.
        let p = sun_position_at(0.0, 80, 12.0);
        close(p.elevation_deg, 90.0, 0.6);
    }

    #[test]
    fn noon_elevation_equinox_known_latitudes() {
        // At solar noon on the equinox, elevation ≈ 90 − |latitude|.
        for lat in [0.0, 23.44, 40.0, 49.7, 51.5, 60.0] {
            let p = sun_position_at(lat, 80, 12.0);
            close(p.elevation_deg, 90.0 - lat, 0.6);
        }
    }

    #[test]
    fn noon_elevation_solstice_known_latitudes() {
        // Summer solstice noon elevation ≈ 90 − |lat − δ|, δ ≈ +23.44°.
        // Iphofen (the headline residence) sits near 49.7°N.
        let p = sun_position_at(49.7, 172, 12.0);
        close(p.elevation_deg, 90.0 - (49.7 - MAX_DECLINATION_DEG), 0.5);

        // Winter solstice noon at the same site, δ ≈ −23.44°.
        let w = sun_position_at(49.7, 355, 12.0);
        close(w.elevation_deg, 90.0 - (49.7 + MAX_DECLINATION_DEG), 0.5);
    }

    #[test]
    fn azimuth_is_due_south_at_solar_noon_northern_hemisphere() {
        // North of the tropics, the sun is due south (180°) at solar noon.
        let p = sun_position_at(49.7, 172, 12.0);
        close(p.azimuth_deg, 180.0, 0.5);
        let q = sun_position_at(40.0, 80, 12.0);
        close(q.azimuth_deg, 180.0, 0.5);
    }

    #[test]
    fn sun_rises_in_the_east_sets_in_the_west() {
        // Morning: azimuth in the eastern half (< 180°). Afternoon: western.
        let morning = sun_position_at(49.7, 172, 8.0);
        assert!(morning.azimuth_deg < 180.0, "morning az={}", morning.azimuth_deg);
        let afternoon = sun_position_at(49.7, 172, 16.0);
        assert!(afternoon.azimuth_deg > 180.0, "afternoon az={}", afternoon.azimuth_deg);
    }

    #[test]
    fn elevation_is_near_zero_at_sunrise_and_sunset() {
        // The sunrise hour angle gives the moment elevation crosses 0°.
        let lat = 49.7;
        let day = 172;
        let h0 = sunrise_hour_angle_deg(lat, day);
        let sunrise_solar_h = 12.0 - h0 / DEGREES_PER_HOUR;
        let sunset_solar_h = 12.0 + h0 / DEGREES_PER_HOUR;
        let rise = sun_position_at(lat, day, sunrise_solar_h);
        let set = sun_position_at(lat, day, sunset_solar_h);
        close(rise.elevation_deg, 0.0, 0.05);
        close(set.elevation_deg, 0.0, 0.05);
    }

    #[test]
    fn sun_is_below_horizon_at_midnight() {
        let p = sun_position_at(49.7, 172, 0.0);
        assert!(p.elevation_deg < 0.0, "midnight elev={}", p.elevation_deg);
        assert!(!p.is_daylight());
    }

    #[test]
    fn daylight_is_longer_in_summer_than_winter_in_north() {
        let summer = daylight_hours(49.7, 172);
        let winter = daylight_hours(49.7, 355);
        let equinox = daylight_hours(49.7, 80);
        assert!(summer > 15.0, "summer daylight {summer}");
        assert!(winter < 9.0, "winter daylight {winter}");
        // Equinox is ~12 hours everywhere.
        close(equinox, 12.0, 0.3);
    }

    #[test]
    fn polar_day_and_night_are_handled() {
        // Above the Arctic circle on the June solstice: midnight sun.
        assert_eq!(sunrise_hour_angle_deg(80.0, 172), 360.0);
        assert_eq!(daylight_hours(80.0, 172), 24.0);
        // Same latitude in December: polar night.
        assert_eq!(sunrise_hour_angle_deg(80.0, 355), 0.0);
        assert_eq!(daylight_hours(80.0, 355), 0.0);
    }

    #[test]
    fn true_solar_time_applies_longitude_and_equation_of_time() {
        // At the prime meridian, solar time ≈ UTC ± the equation of time only.
        let day = 172;
        let t = true_solar_time_h(12.0, 0.0, day);
        close(t, 12.0 + equation_of_time_min(day) / 60.0, 1e-6);
        // 15° east advances solar time by ~1 hour.
        let east = true_solar_time_h(12.0, 15.0, day);
        let west = true_solar_time_h(12.0, -15.0, day);
        close(east - west, 2.0, 1e-6);
    }
}
