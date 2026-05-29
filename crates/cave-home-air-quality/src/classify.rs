//! Classifiers for the non-AQI quantities: CO₂, the Sensirion VOC Index, and
//! radon. These pollutants have no EPA AQI breakpoint table, so each is mapped
//! to the shared [`AirCategory`] band from its own public reference thresholds.

use crate::category::AirCategory;
use crate::reading::Pollutant;

/// Classify CO₂ in ppm.
///
/// Thresholds follow common indoor-air-quality guidance (outdoor baseline
/// ≈ 420 ppm; OSHA's 8-hour permissible exposure limit is 5000 ppm, which
/// anchors the top "dangerous" band).
#[must_use]
fn classify_co2(ppm: f64) -> AirCategory {
    match ppm {
        v if v < 800.0 => AirCategory::Good,
        v if v < 1000.0 => AirCategory::Fair,
        v if v < 1400.0 => AirCategory::Sensitive,
        v if v < 2000.0 => AirCategory::Poor,
        v if v < 5000.0 => AirCategory::VeryPoor,
        _ => AirCategory::Hazardous,
    }
}

/// Classify the Sensirion VOC Index (1..=500; 100 ≈ the rolling-average
/// "normal" for the room). The index is bounded at 500, so there is no
/// `Hazardous` band — a sustained high index means "find and remove the
/// source", which maps to `VeryPoor`.
#[must_use]
fn classify_voc(index: f64) -> AirCategory {
    match index {
        v if v <= 100.0 => AirCategory::Good,
        v if v <= 200.0 => AirCategory::Fair,
        v if v <= 300.0 => AirCategory::Sensitive,
        v if v <= 400.0 => AirCategory::Poor,
        _ => AirCategory::VeryPoor,
    }
}

/// Classify radon in Bq/m³.
///
/// Anchored on the WHO reference level (100 Bq/m³) and the US EPA action level
/// (148 Bq/m³ ≈ 4 pCi/L).
#[must_use]
fn classify_radon(bq_m3: f64) -> AirCategory {
    match bq_m3 {
        v if v < 100.0 => AirCategory::Good,
        v if v < 148.0 => AirCategory::Fair,
        v if v < 300.0 => AirCategory::Sensitive,
        v if v < 600.0 => AirCategory::Poor,
        v if v < 1000.0 => AirCategory::VeryPoor,
        _ => AirCategory::Hazardous,
    }
}

/// Classify a non-AQI pollutant value into an [`AirCategory`].
///
/// Returns `None` for pollutants that are graded through the EPA AQI engine
/// ([`crate::aqi`]) instead — callers should route those through `aqi` and then
/// [`AirCategory::from_aqi`].
#[must_use]
pub fn classify(pollutant: Pollutant, value: f64) -> Option<AirCategory> {
    match pollutant {
        Pollutant::CarbonDioxide => Some(classify_co2(value)),
        Pollutant::VocIndex => Some(classify_voc(value)),
        Pollutant::Radon => Some(classify_radon(value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn co2_bands() {
        assert_eq!(classify_co2(420.0), AirCategory::Good);
        assert_eq!(classify_co2(799.0), AirCategory::Good);
        assert_eq!(classify_co2(800.0), AirCategory::Fair);
        assert_eq!(classify_co2(1200.0), AirCategory::Sensitive);
        assert_eq!(classify_co2(1800.0), AirCategory::Poor);
        assert_eq!(classify_co2(3000.0), AirCategory::VeryPoor);
        assert_eq!(classify_co2(6000.0), AirCategory::Hazardous);
    }

    #[test]
    fn voc_index_bands_and_cap() {
        assert_eq!(classify_voc(100.0), AirCategory::Good);
        assert_eq!(classify_voc(150.0), AirCategory::Fair);
        assert_eq!(classify_voc(250.0), AirCategory::Sensitive);
        assert_eq!(classify_voc(350.0), AirCategory::Poor);
        assert_eq!(classify_voc(500.0), AirCategory::VeryPoor);
    }

    #[test]
    fn radon_anchored_on_who_and_epa() {
        assert_eq!(classify_radon(99.0), AirCategory::Good);
        assert_eq!(classify_radon(100.0), AirCategory::Fair); // WHO reference
        assert_eq!(classify_radon(148.0), AirCategory::Sensitive); // EPA action
        assert_eq!(classify_radon(500.0), AirCategory::Poor);
        assert_eq!(classify_radon(800.0), AirCategory::VeryPoor);
        assert_eq!(classify_radon(1500.0), AirCategory::Hazardous);
    }

    #[test]
    fn aqi_pollutants_return_none() {
        assert_eq!(classify(Pollutant::Pm25, 12.0), None);
        assert_eq!(classify(Pollutant::Ozone, 0.05), None);
    }

    #[test]
    fn non_aqi_pollutants_return_some() {
        assert!(classify(Pollutant::CarbonDioxide, 700.0).is_some());
        assert!(classify(Pollutant::VocIndex, 100.0).is_some());
        assert!(classify(Pollutant::Radon, 50.0).is_some());
    }
}
