//! US EPA Air Quality Index engine.
//!
//! Implemented from the public EPA technical assistance document
//! *"Technical Assistance Document for the Reporting of Daily Air Quality —
//! the Air Quality Index (AQI)"* (EPA-454/B-24-002, the **May 2024 revision**,
//! which lowered the PM2.5 breakpoints). Per Charter §7 (always-latest) the
//! engine ships the current breakpoint tables, not a historical snapshot.
//!
//! The AQI for a pollutant is a piecewise-linear interpolation between
//! breakpoints:
//!
//! ```text
//! I_p = (I_hi - I_lo) / (C_hi - C_lo) * (C_p - C_lo) + I_lo
//! ```
//!
//! where the truncated concentration `C_p` falls in the breakpoint band
//! `[C_lo, C_hi]` mapping to the index band `[I_lo, I_hi]`. EPA requires the
//! input concentration to be truncated (not rounded) to the precision of the
//! breakpoint table before the formula is applied; we do that here.

use crate::reading::Pollutant;

/// One breakpoint row: a concentration band mapping to an AQI band.
#[derive(Debug, Clone, Copy)]
struct Breakpoint {
    c_lo: f64,
    c_hi: f64,
    i_lo: f64,
    i_hi: f64,
}

/// Number of decimal places the concentration is truncated to before lookup,
/// per the EPA table precision for each pollutant.
const fn truncation_decimals(p: Pollutant) -> i32 {
    match p {
        // PM2.5 tables are stated to 0.1 µg/m³.
        Pollutant::Pm25 => 1,
        // O3 (ppm) is stated to 0.001 ppm.
        Pollutant::Ozone => 3,
        // CO (ppm) is stated to 0.1 ppm.
        Pollutant::CarbonMonoxide => 1,
        // PM10 (µg/m³), NO2 / SO2 (ppb) are stated to integer precision.
        _ => 0,
    }
}

/// Build a breakpoint row.
const fn bp(c_lo: f64, c_hi: f64, i_lo: f64, i_hi: f64) -> Breakpoint {
    Breakpoint { c_lo, c_hi, i_lo, i_hi }
}

// PM2.5, 24-hour, µg/m³ — EPA 2024 revision (lowered breakpoints).
const PM25: [Breakpoint; 6] = [
    bp(0.0, 9.0, 0.0, 50.0),
    bp(9.1, 35.4, 51.0, 100.0),
    bp(35.5, 55.4, 101.0, 150.0),
    bp(55.5, 125.4, 151.0, 200.0),
    bp(125.5, 225.4, 201.0, 300.0),
    bp(225.5, 325.4, 301.0, 500.0),
];
// PM10, 24-hour, µg/m³.
const PM10: [Breakpoint; 6] = [
    bp(0.0, 54.0, 0.0, 50.0),
    bp(55.0, 154.0, 51.0, 100.0),
    bp(155.0, 254.0, 101.0, 150.0),
    bp(255.0, 354.0, 151.0, 200.0),
    bp(355.0, 424.0, 201.0, 300.0),
    bp(425.0, 604.0, 301.0, 500.0),
];
// Ozone, 8-hour, ppm. (8-hour O3 is not reported above AQI 300.)
const OZONE: [Breakpoint; 5] = [
    bp(0.000, 0.054, 0.0, 50.0),
    bp(0.055, 0.070, 51.0, 100.0),
    bp(0.071, 0.085, 101.0, 150.0),
    bp(0.086, 0.105, 151.0, 200.0),
    bp(0.106, 0.200, 201.0, 300.0),
];
// Nitrogen dioxide, 1-hour, ppb.
const NO2: [Breakpoint; 6] = [
    bp(0.0, 53.0, 0.0, 50.0),
    bp(54.0, 100.0, 51.0, 100.0),
    bp(101.0, 360.0, 101.0, 150.0),
    bp(361.0, 649.0, 151.0, 200.0),
    bp(650.0, 1249.0, 201.0, 300.0),
    bp(1250.0, 2049.0, 301.0, 500.0),
];
// Sulfur dioxide, 1-hour, ppb.
const SO2: [Breakpoint; 6] = [
    bp(0.0, 35.0, 0.0, 50.0),
    bp(36.0, 75.0, 51.0, 100.0),
    bp(76.0, 185.0, 101.0, 150.0),
    bp(186.0, 304.0, 151.0, 200.0),
    bp(305.0, 604.0, 201.0, 300.0),
    bp(605.0, 1004.0, 301.0, 500.0),
];
// Carbon monoxide, 8-hour, ppm.
const CO: [Breakpoint; 6] = [
    bp(0.0, 4.4, 0.0, 50.0),
    bp(4.5, 9.4, 51.0, 100.0),
    bp(9.5, 12.4, 101.0, 150.0),
    bp(12.5, 15.4, 151.0, 200.0),
    bp(15.5, 30.4, 201.0, 300.0),
    bp(30.5, 50.4, 301.0, 500.0),
];

/// EPA breakpoint table for an AQI pollutant. `None` for pollutants graded by
/// [`crate::classify`] instead (CO₂, VOC, radon).
fn table(p: Pollutant) -> Option<&'static [Breakpoint]> {
    match p {
        Pollutant::Pm25 => Some(&PM25),
        Pollutant::Pm10 => Some(&PM10),
        Pollutant::Ozone => Some(&OZONE),
        Pollutant::NitrogenDioxide => Some(&NO2),
        Pollutant::SulfurDioxide => Some(&SO2),
        Pollutant::CarbonMonoxide => Some(&CO),
        _ => None,
    }
}

/// Truncate `value` to `decimals` decimal places (EPA: truncate, never round).
fn truncate(value: f64, decimals: i32) -> f64 {
    if decimals <= 0 {
        return value.trunc();
    }
    let scale = 10f64.powi(decimals);
    (value * scale).trunc() / scale
}

/// The outcome of an AQI computation for a single pollutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AqiOutcome {
    /// A computed AQI sub-index, clamped to the table domain.
    Value(u16),
    /// The concentration exceeds the top of the published table — the AQI is
    /// "beyond the index" (EPA reports these as > the table maximum). We keep
    /// the maximum index for that pollutant rather than fabricating a number.
    AboveTable(u16),
    /// This pollutant has no EPA AQI table (CO₂, VOC, radon).
    NotApplicable,
}

impl AqiOutcome {
    /// The numeric AQI for aggregation. `NotApplicable` yields `None`;
    /// `AboveTable(max)` yields `max` so the overall index never under-reports
    /// a pollutant that has run off the top of the chart.
    #[must_use]
    pub const fn as_index(self) -> Option<u16> {
        match self {
            Self::Value(v) | Self::AboveTable(v) => Some(v),
            Self::NotApplicable => None,
        }
    }
}

/// Compute the US EPA AQI sub-index for a single pollutant concentration.
///
/// Returns [`AqiOutcome::NotApplicable`] for pollutants without an EPA table.
#[must_use]
pub fn sub_index(pollutant: Pollutant, concentration: f64) -> AqiOutcome {
    let Some(rows) = table(pollutant) else {
        return AqiOutcome::NotApplicable;
    };
    let c = truncate(concentration, truncation_decimals(pollutant));
    for row in rows {
        if c >= row.c_lo && c <= row.c_hi {
            let slope = (row.i_hi - row.i_lo) / (row.c_hi - row.c_lo);
            let idx = slope.mul_add(c - row.c_lo, row.i_lo);
            // EPA rounds the resulting index to the nearest integer.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let rounded = idx.round() as u16;
            return AqiOutcome::Value(rounded);
        }
    }
    // Concentration above the published maximum: report the table ceiling.
    let max_index =
        rows.last().map_or(0.0, |r| r.i_hi);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ceil = max_index as u16;
    AqiOutcome::AboveTable(ceil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pm25_breakpoint_endpoints_2024() {
        // EPA 2024: 9.0 µg/m³ is the top of the "Good" band -> AQI 50.
        assert_eq!(sub_index(Pollutant::Pm25, 9.0), AqiOutcome::Value(50));
        // 9.1 is the bottom of "Moderate" -> AQI 51.
        assert_eq!(sub_index(Pollutant::Pm25, 9.1), AqiOutcome::Value(51));
        // 0.0 -> 0.
        assert_eq!(sub_index(Pollutant::Pm25, 0.0), AqiOutcome::Value(0));
    }

    #[test]
    fn pm25_interpolates_midband() {
        // 20.0 µg/m³ sits in [9.1, 35.4] -> [51, 100].
        // I = (100-51)/(35.4-9.1)*(20.0-9.1)+51 = 49/26.3*10.9+51 ≈ 71.3 -> 71.
        assert_eq!(sub_index(Pollutant::Pm25, 20.0), AqiOutcome::Value(71));
    }

    #[test]
    fn pm25_truncates_not_rounds_input() {
        // 9.09 truncates to 9.0 (Good, AQI 50), NOT up into the Moderate band.
        assert_eq!(sub_index(Pollutant::Pm25, 9.09), AqiOutcome::Value(50));
    }

    #[test]
    fn co_eight_hour_band() {
        // 9.4 ppm is the top of the "Moderate" CO band -> AQI 100.
        assert_eq!(sub_index(Pollutant::CarbonMonoxide, 9.4), AqiOutcome::Value(100));
    }

    #[test]
    fn pm10_integer_truncation() {
        // 54 -> 50 (top of Good). 54.9 truncates to 54 -> still 50.
        assert_eq!(sub_index(Pollutant::Pm10, 54.0), AqiOutcome::Value(50));
        assert_eq!(sub_index(Pollutant::Pm10, 54.9), AqiOutcome::Value(50));
        assert_eq!(sub_index(Pollutant::Pm10, 55.0), AqiOutcome::Value(51));
    }

    #[test]
    fn ozone_three_decimal_precision() {
        assert_eq!(sub_index(Pollutant::Ozone, 0.054), AqiOutcome::Value(50));
        assert_eq!(sub_index(Pollutant::Ozone, 0.055), AqiOutcome::Value(51));
    }

    #[test]
    fn above_table_reports_ceiling_not_garbage() {
        // PM2.5 table tops out at 325.4 µg/m³ -> ceiling AQI 500.
        assert_eq!(sub_index(Pollutant::Pm25, 9_999.0), AqiOutcome::AboveTable(500));
        // O3 8-hour table tops out at AQI 300.
        assert_eq!(sub_index(Pollutant::Ozone, 1.0), AqiOutcome::AboveTable(300));
    }

    #[test]
    fn non_aqi_pollutants_are_not_applicable() {
        assert_eq!(sub_index(Pollutant::CarbonDioxide, 800.0), AqiOutcome::NotApplicable);
        assert_eq!(sub_index(Pollutant::VocIndex, 100.0), AqiOutcome::NotApplicable);
        assert_eq!(sub_index(Pollutant::Radon, 100.0), AqiOutcome::NotApplicable);
    }

    #[test]
    fn outcome_as_index() {
        assert_eq!(AqiOutcome::Value(42).as_index(), Some(42));
        assert_eq!(AqiOutcome::AboveTable(500).as_index(), Some(500));
        assert_eq!(AqiOutcome::NotApplicable.as_index(), None);
    }
}
