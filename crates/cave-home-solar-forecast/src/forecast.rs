//! The day's solar forecast — instantaneous power and a daily energy estimate.
//!
//! This is the surface the rest of cave-home consumes: give it where the house
//! is, which day it is, the panel array and how cloudy it is, and it returns
//! the power right now and the energy expected across the whole day. The daily
//! figure is a simple time integral of the instantaneous model, sampled on a
//! caller-supplied step (the engine has no clock, so the caller decides the
//! resolution).

use crate::array::PvArray;
use crate::irradiance::DEFAULT_CLEAR_SKY_TRANSMITTANCE;
use crate::sun_position::{self, SunPosition};

/// Why a forecast input was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastError {
    /// Latitude must be between the poles, −90°..=90°.
    BadLatitude,
    /// Longitude must be −180°..=180°.
    BadLongitude,
    /// Day-of-year must be 1..=366.
    BadDayOfYear,
    /// Cloud cover must be a fraction 0..=1.
    BadCloudCover,
    /// The integration step (hours) must be finite and greater than zero.
    BadStep,
}

impl core::fmt::Display for ForecastError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::BadLatitude => "latitude must be between the poles",
            Self::BadLongitude => "longitude must be on the globe",
            Self::BadDayOfYear => "day of the year must be 1 to 366",
            Self::BadCloudCover => "cloudiness must be a fraction up to one",
            Self::BadStep => "the time step must be more than zero",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for ForecastError {}

/// Where the house is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    latitude_deg: f64,
    longitude_deg: f64,
}

impl Site {
    /// Validate and build a site.
    ///
    /// # Errors
    /// Returns [`ForecastError`] for impossible latitude/longitude.
    pub fn new(latitude_deg: f64, longitude_deg: f64) -> Result<Self, ForecastError> {
        if !latitude_deg.is_finite() || !(-90.0..=90.0).contains(&latitude_deg) {
            return Err(ForecastError::BadLatitude);
        }
        if !longitude_deg.is_finite() || !(-180.0..=180.0).contains(&longitude_deg) {
            return Err(ForecastError::BadLongitude);
        }
        Ok(Self { latitude_deg, longitude_deg })
    }

    #[must_use]
    pub const fn latitude_deg(self) -> f64 {
        self.latitude_deg
    }

    #[must_use]
    pub const fn longitude_deg(self) -> f64 {
        self.longitude_deg
    }
}

/// Validate a day-of-year (1..=366).
fn check_day(day_of_year: u16) -> Result<u16, ForecastError> {
    if (1..=366).contains(&day_of_year) {
        Ok(day_of_year)
    } else {
        Err(ForecastError::BadDayOfYear)
    }
}

/// Validate a cloud-cover fraction (0..=1).
fn check_cloud(cloud_cover: f64) -> Result<f64, ForecastError> {
    if cloud_cover.is_finite() && (0.0..=1.0).contains(&cloud_cover) {
        Ok(cloud_cover)
    } else {
        Err(ForecastError::BadCloudCover)
    }
}

/// The instantaneous production at one moment of the day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instant {
    /// The sun's position at that moment.
    pub sun: SunPosition,
    /// Plane-of-array irradiance on the panels, W/m² (engine-internal detail).
    pub plane_of_array_w_m2: f64,
    /// AC power output right then, in kilowatts.
    pub power_kw: f64,
}

/// Production at one solar-time instant for a site, day, array and cloud cover.
///
/// `true_solar_time_h` is the local apparent solar time in hours (12.0 = solar
/// noon). For wall-clock UTC, convert with
/// [`sun_position::true_solar_time_h`] first.
///
/// # Errors
/// Returns [`ForecastError`] for a bad day-of-year or cloud cover.
pub fn instant_at(
    site: Site,
    array: PvArray,
    day_of_year: u16,
    true_solar_time_h: f64,
    cloud_cover: f64,
) -> Result<Instant, ForecastError> {
    let day = check_day(day_of_year)?;
    let cloud = check_cloud(cloud_cover)?;
    let sun = sun_position::sun_position_at(site.latitude_deg, day, true_solar_time_h);
    let poa = array.plane_of_array_w_m2(sun, day, DEFAULT_CLEAR_SKY_TRANSMITTANCE);
    let power = array.ac_power_kw(poa, cloud);
    Ok(Instant { sun, plane_of_array_w_m2: poa, power_kw: power })
}

/// A whole-day forecast for a site and array.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyForecast {
    /// Total energy expected across the day, in kilowatt-hours.
    pub energy_kwh: f64,
    /// The highest instantaneous power reached during the day, in kilowatts.
    pub peak_power_kw: f64,
    /// The solar time (hours) at which the peak occurs.
    pub peak_solar_time_h: f64,
    /// How many hours the sun is above the horizon.
    pub daylight_hours: f64,
    /// The cloud-cover fraction the forecast assumed.
    pub cloud_cover: f64,
}

/// Forecast a whole day by integrating instantaneous power over solar time.
///
/// The day is swept from 0h to 24h in `step_hours` increments (a smaller step
/// is more accurate; 0.25 h = 15-minute resolution is a good default). Energy
/// is the midpoint Riemann sum of power × step. The peak and daylight length
/// fall out of the same sweep.
///
/// # Errors
/// Returns [`ForecastError`] for a bad day-of-year, cloud cover or step.
pub fn forecast_day(
    site: Site,
    array: PvArray,
    day_of_year: u16,
    cloud_cover: f64,
    step_hours: f64,
) -> Result<DailyForecast, ForecastError> {
    let day = check_day(day_of_year)?;
    let cloud = check_cloud(cloud_cover)?;
    if !step_hours.is_finite() || step_hours <= 0.0 {
        return Err(ForecastError::BadStep);
    }

    let mut energy_kwh = 0.0;
    let mut peak_power_kw = 0.0;
    let mut peak_solar_time_h = 12.0;

    // Midpoint rule: sample at the centre of each step across the 24-hour day.
    // Count steps with an integer so the loop bound never drifts on a float.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let steps = (24.0 / step_hours).ceil() as u32;
    for i in 0..steps {
        let t = (f64::from(i) + 0.5) * step_hours;
        if t >= 24.0 {
            break;
        }
        let moment = instant_at(site, array, day, t, cloud)?;
        // Energy in this slice: power (kW) × step (h) = kWh.
        energy_kwh += moment.power_kw * step_hours;
        if moment.power_kw > peak_power_kw {
            peak_power_kw = moment.power_kw;
            peak_solar_time_h = t;
        }
    }

    let daylight = sun_position::daylight_hours(site.latitude_deg, day);

    Ok(DailyForecast {
        energy_kwh,
        peak_power_kw,
        peak_solar_time_h,
        daylight_hours: daylight,
        cloud_cover: cloud,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "expected {a} ≈ {b} (within {tol})");
    }

    fn iphofen() -> Site {
        // The headline residence: ~49.7°N, ~10.3°E.
        Site::new(49.7, 10.3).expect("valid site")
    }

    fn roof() -> PvArray {
        PvArray::new(6.0, 30.0, 180.0, 0.85).expect("valid array")
    }

    #[test]
    fn rejects_impossible_latitude() {
        assert_eq!(Site::new(91.0, 0.0), Err(ForecastError::BadLatitude));
        assert_eq!(Site::new(-90.5, 0.0), Err(ForecastError::BadLatitude));
        assert_eq!(Site::new(f64::NAN, 0.0), Err(ForecastError::BadLatitude));
    }

    #[test]
    fn rejects_impossible_longitude() {
        assert_eq!(Site::new(49.7, 181.0), Err(ForecastError::BadLongitude));
        assert_eq!(Site::new(49.7, -200.0), Err(ForecastError::BadLongitude));
    }

    #[test]
    fn accepts_pole_and_antimeridian_boundaries() {
        assert!(Site::new(90.0, 180.0).is_ok());
        assert!(Site::new(-90.0, -180.0).is_ok());
    }

    #[test]
    fn rejects_bad_day_of_year() {
        assert_eq!(
            forecast_day(iphofen(), roof(), 0, 0.0, 0.25),
            Err(ForecastError::BadDayOfYear)
        );
        assert_eq!(
            forecast_day(iphofen(), roof(), 367, 0.0, 0.25),
            Err(ForecastError::BadDayOfYear)
        );
        assert!(forecast_day(iphofen(), roof(), 1, 0.0, 0.25).is_ok());
        assert!(forecast_day(iphofen(), roof(), 366, 0.0, 0.25).is_ok());
    }

    #[test]
    fn rejects_bad_cloud_cover() {
        assert_eq!(
            forecast_day(iphofen(), roof(), 172, -0.1, 0.25),
            Err(ForecastError::BadCloudCover)
        );
        assert_eq!(
            forecast_day(iphofen(), roof(), 172, 1.1, 0.25),
            Err(ForecastError::BadCloudCover)
        );
    }

    #[test]
    fn rejects_bad_step() {
        assert_eq!(
            forecast_day(iphofen(), roof(), 172, 0.0, 0.0),
            Err(ForecastError::BadStep)
        );
        assert_eq!(
            forecast_day(iphofen(), roof(), 172, 0.0, -1.0),
            Err(ForecastError::BadStep)
        );
    }

    #[test]
    fn summer_day_produces_more_than_winter_day() {
        let summer = forecast_day(iphofen(), roof(), 172, 0.0, 0.1).unwrap();
        let winter = forecast_day(iphofen(), roof(), 355, 0.0, 0.1).unwrap();
        assert!(
            summer.energy_kwh > winter.energy_kwh,
            "summer {} vs winter {}",
            summer.energy_kwh,
            winter.energy_kwh
        );
    }

    #[test]
    fn clear_day_produces_more_than_cloudy_day() {
        let clear = forecast_day(iphofen(), roof(), 172, 0.0, 0.1).unwrap();
        let cloudy = forecast_day(iphofen(), roof(), 172, 1.0, 0.1).unwrap();
        assert!(cloudy.energy_kwh < clear.energy_kwh);
        // Full overcast leaves the ~25% diffuse floor of the clear-day energy.
        close(cloudy.energy_kwh, clear.energy_kwh * 0.25, clear.energy_kwh * 0.02);
    }

    #[test]
    fn daily_energy_is_in_plausible_kwh_range() {
        // A clear-sky 6 kWp south roof at ~50°N near the June solstice produces
        // on the order of 30-45 kWh — the right order of magnitude for the
        // simple clear-sky model.
        let f = forecast_day(iphofen(), roof(), 172, 0.0, 0.05).unwrap();
        assert!((25.0..=50.0).contains(&f.energy_kwh), "summer energy {}", f.energy_kwh);
    }

    #[test]
    fn peak_power_occurs_around_solar_noon() {
        let f = forecast_day(iphofen(), roof(), 172, 0.0, 0.1).unwrap();
        assert!((10.0..=14.0).contains(&f.peak_solar_time_h), "peak at {}", f.peak_solar_time_h);
        assert!(f.peak_power_kw > 0.0);
        // Peak never exceeds the rated power after derate.
        assert!(f.peak_power_kw <= roof().peak_power_kw());
    }

    #[test]
    fn energy_scales_with_array_size() {
        let small = PvArray::new(3.0, 30.0, 180.0, 0.85).unwrap();
        let big = PvArray::new(6.0, 30.0, 180.0, 0.85).unwrap();
        let es = forecast_day(iphofen(), small, 172, 0.0, 0.1).unwrap().energy_kwh;
        let eb = forecast_day(iphofen(), big, 172, 0.0, 0.1).unwrap().energy_kwh;
        close(eb, 2.0 * es, 1e-6);
    }

    #[test]
    fn finer_step_converges() {
        // Coarse and fine integration agree to a few percent.
        let coarse = forecast_day(iphofen(), roof(), 172, 0.0, 0.5).unwrap().energy_kwh;
        let fine = forecast_day(iphofen(), roof(), 172, 0.0, 0.02).unwrap().energy_kwh;
        assert!((coarse - fine).abs() / fine < 0.05, "coarse {coarse} vs fine {fine}");
    }

    #[test]
    fn instant_at_noon_has_power_and_daylight() {
        let m = instant_at(iphofen(), roof(), 172, 12.0, 0.0).unwrap();
        assert!(m.sun.is_daylight());
        assert!(m.power_kw > 0.0);
    }

    #[test]
    fn instant_at_night_has_no_power() {
        let m = instant_at(iphofen(), roof(), 172, 0.0, 0.0).unwrap();
        assert!(!m.sun.is_daylight());
        assert_eq!(m.power_kw, 0.0);
    }

    #[test]
    fn daylight_length_carried_into_forecast() {
        let f = forecast_day(iphofen(), roof(), 172, 0.0, 0.25).unwrap();
        assert!(f.daylight_hours > 15.0, "summer daylight {}", f.daylight_hours);
    }
}
