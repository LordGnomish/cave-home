//! Soil-moisture assessment vs. a plant's ideal band.
//!
//! Given a measured soil-moisture percentage and the plant's profile, this
//! reports whether the soil is [`SoilFit::TooDry`], [`SoilFit::Ideal`] or
//! [`SoilFit::TooWet`], and — when it is too dry — a [`WaterRecommendation`].
//!
//! **Boundary with cave-home-water (ADR-013).** This crate *recommends* that a
//! plant be watered; it never opens a valve or computes a runtime. The actual
//! watering — circuit selection, runtime, rain delay, flow monitoring — is the
//! job of `cave-home-water`. We model the recommendation as data so a caller can
//! hand it to that crate, but we take no dependency on it (Charter boundary;
//! the integration is a Phase-1b `[[unmapped]]` item in the manifest).

use crate::label::Lang;
use crate::plant::CareProfile;

/// How the measured soil moisture compares to the plant's ideal band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoilFit {
    /// Below the plant's ideal band — the plant wants water.
    TooDry,
    /// Within the plant's ideal band.
    Ideal,
    /// Above the plant's ideal band — risk of root rot / overwatering.
    TooWet,
}

/// A recommendation to water a plant, to be carried out by cave-home-water.
///
/// This is intentionally just a *flag plus a reason*: it names the plant and
/// says "this plant wants water". It deliberately carries **no** runtime, valve
/// or circuit — those belong to cave-home-water (ADR-013), which owns the
/// hardware and the schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterRecommendation {
    /// The plant that wants water.
    pub plant_name: String,
    /// How far below the ideal band the soil currently is, in percentage
    /// points — a rough urgency hint, not a watering dose.
    pub deficit_percent: f64,
}

/// Assess soil moisture (%) against a plant's ideal band.
///
/// The band is inclusive on both ends: exactly at `soil_moisture_min` or
/// `soil_moisture_max` counts as [`SoilFit::Ideal`].
#[must_use]
pub fn assess(soil_moisture_percent: f64, profile: &CareProfile) -> SoilFit {
    if soil_moisture_percent < profile.soil_moisture_min {
        SoilFit::TooDry
    } else if soil_moisture_percent > profile.soil_moisture_max {
        SoilFit::TooWet
    } else {
        SoilFit::Ideal
    }
}

/// Recommend watering for a plant if (and only if) its soil is too dry.
///
/// Returns `Some(WaterRecommendation)` when the plant wants water, `None`
/// otherwise. The actual watering is deferred to cave-home-water (see module
/// docs).
#[must_use]
pub fn recommend_water(
    plant_name: &str,
    soil_moisture_percent: f64,
    profile: &CareProfile,
) -> Option<WaterRecommendation> {
    match assess(soil_moisture_percent, profile) {
        SoilFit::TooDry => Some(WaterRecommendation {
            plant_name: plant_name.to_string(),
            deficit_percent: profile.soil_moisture_min - soil_moisture_percent,
        }),
        SoilFit::Ideal | SoilFit::TooWet => None,
    }
}

impl SoilFit {
    /// A plain-language note for the household, naming the plant (Charter §6.3
    /// — "needs water", never "soil moisture %").
    #[must_use]
    pub fn message(self, plant_name: &str, lang: Lang) -> String {
        match (self, lang) {
            (Self::TooDry, Lang::En) => format!("{plant_name} need water."),
            (Self::TooDry, Lang::De) => format!("{plant_name} brauchen Wasser."),
            (Self::TooDry, Lang::Tr) => format!("{plant_name} suya ihtiyaç duyuyor."),
            (Self::Ideal, Lang::En) => format!("{plant_name} have plenty to drink."),
            (Self::Ideal, Lang::De) => format!("{plant_name} haben genug zu trinken."),
            (Self::Ideal, Lang::Tr) => format!("{plant_name} yeterince su almış."),
            (Self::TooWet, Lang::En) => format!("{plant_name} are sitting in too much water — let them dry out."),
            (Self::TooWet, Lang::De) => format!("{plant_name} stehen zu nass — etwas abtrocknen lassen."),
            (Self::TooWet, Lang::Tr) => format!("{plant_name} fazla ıslak — biraz kurumasını bekleyin."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plant::PlantKind;

    fn tomato() -> CareProfile {
        // min 40, max 70.
        PlantKind::Tomato.profile()
    }

    #[test]
    fn too_dry_below_band() {
        assert_eq!(assess(30.0, &tomato()), SoilFit::TooDry);
        assert_eq!(assess(39.999, &tomato()), SoilFit::TooDry);
    }

    #[test]
    fn ideal_within_band_inclusive() {
        assert_eq!(assess(40.0, &tomato()), SoilFit::Ideal); // min boundary
        assert_eq!(assess(55.0, &tomato()), SoilFit::Ideal);
        assert_eq!(assess(70.0, &tomato()), SoilFit::Ideal); // max boundary
    }

    #[test]
    fn too_wet_above_band() {
        assert_eq!(assess(70.001, &tomato()), SoilFit::TooWet);
        assert_eq!(assess(90.0, &tomato()), SoilFit::TooWet);
    }

    #[test]
    fn recommends_water_only_when_too_dry() {
        let dry = recommend_water("Tomatoes", 25.0, &tomato());
        assert!(dry.is_some());
        let rec = dry.expect("dry soil yields a recommendation");
        assert_eq!(rec.plant_name, "Tomatoes");
        assert!((rec.deficit_percent - 15.0).abs() < 1e-9); // 40 - 25

        assert!(recommend_water("Tomatoes", 55.0, &tomato()).is_none()); // ideal
        assert!(recommend_water("Tomatoes", 85.0, &tomato()).is_none()); // too wet
    }

    #[test]
    fn dry_loving_plant_has_a_low_band() {
        // Lavender is happy where a tomato would be parched.
        let lav = PlantKind::Lavender.profile();
        assert_eq!(assess(25.0, &lav), SoilFit::Ideal);
        assert_eq!(assess(25.0, &tomato()), SoilFit::TooDry);
    }
}
