//! Light assessment — measured light vs. a plant's light need.
//!
//! A plant wants a certain amount of sun ([`LightNeed`]). Given the light the
//! plant is actually getting — either a measured illuminance in lux or a coarse
//! [`LightBand`] from a sensor that only reports dark/dim/bright — this maps it
//! to [`LightFit`]: is the plant getting too little, about right, or too much?
//!
//! Lux anchors (rough daylight references, full sun outdoors ≈ 100 000 lux):
//! - shade plants are happy from deep shade up to bright shade,
//! - part-shade plants want dappled / morning sun,
//! - full-sun plants want most of the day in direct sun and suffer in shade.

use crate::label::Lang;

/// How much direct light a plant wants. Part of a plant's care profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightNeed {
    /// Wants most of the day in direct sun (e.g. tomatoes, lavender).
    FullSun,
    /// Wants dappled light or a few hours of gentle sun (e.g. lettuce).
    PartShade,
    /// Wants out of direct sun (e.g. ferns).
    Shade,
}

/// A coarse light band for sensors that report only dark / dim / bright,
/// rather than a calibrated lux figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightBand {
    /// Deep shade or night.
    Dark,
    /// Indoor-bright / shaded outdoors.
    Dim,
    /// Open shade / bright indirect light.
    Bright,
    /// Direct sun.
    FullSun,
}

impl LightBand {
    /// A representative illuminance (lux) for this band, so the same logic can
    /// serve both calibrated and coarse sensors.
    #[must_use]
    pub const fn typical_lux(self) -> f64 {
        match self {
            Self::Dark => 200.0,
            Self::Dim => 2_000.0,
            Self::Bright => 15_000.0,
            Self::FullSun => 60_000.0,
        }
    }
}

/// How well the light a plant is getting fits what it wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightFit {
    /// Not enough light — the plant will get leggy / pale.
    TooLittle,
    /// About right.
    Adequate,
    /// Too much direct sun — the plant will scorch.
    TooMuch,
}

/// The comfortable illuminance band (lux) for a given light need, as
/// `(min, max)`. Below `min` is too little; above `max` is too much.
const fn comfort_band(need: LightNeed) -> (f64, f64) {
    match need {
        // Full-sun plants want a lot and rarely get "too much" outdoors.
        LightNeed::FullSun => (10_000.0, 120_000.0),
        // Part-shade: happy in bright indirect to a little direct sun.
        LightNeed::PartShade => (3_000.0, 30_000.0),
        // Shade: bright enough to grow, but scorches in direct sun.
        LightNeed::Shade => (500.0, 12_000.0),
    }
}

/// Assess a measured illuminance (lux) against a plant's light need.
#[must_use]
pub fn assess_lux(lux: f64, need: LightNeed) -> LightFit {
    let (min, max) = comfort_band(need);
    if lux < min {
        LightFit::TooLittle
    } else if lux > max {
        LightFit::TooMuch
    } else {
        LightFit::Adequate
    }
}

/// Assess a coarse light band against a plant's light need.
#[must_use]
pub fn assess_band(band: LightBand, need: LightNeed) -> LightFit {
    assess_lux(band.typical_lux(), need)
}

impl LightFit {
    /// A plain-language note for the household, naming the plant (Charter §6.3
    /// — "too much sun", never "lux" or "illuminance").
    #[must_use]
    pub fn message(self, plant_name: &str, lang: Lang) -> String {
        match (self, lang) {
            (Self::Adequate, Lang::En) => format!("{plant_name} is getting just the right amount of light."),
            (Self::Adequate, Lang::De) => format!("{plant_name} bekommt genau die richtige Menge Licht."),
            (Self::Adequate, Lang::Tr) => format!("{plant_name} tam doğru miktarda ışık alıyor."),
            (Self::TooLittle, Lang::En) => format!("{plant_name} needs more light — move it somewhere brighter."),
            (Self::TooLittle, Lang::De) => format!("{plant_name} braucht mehr Licht — an einen helleren Platz stellen."),
            (Self::TooLittle, Lang::Tr) => format!("{plant_name} daha çok ışığa ihtiyaç duyuyor — daha aydınlık bir yere taşıyın."),
            (Self::TooMuch, Lang::En) => format!("{plant_name} is getting too much sun — give it some shade."),
            (Self::TooMuch, Lang::De) => format!("{plant_name} bekommt zu viel Sonne — etwas Schatten geben."),
            (Self::TooMuch, Lang::Tr) => format!("{plant_name} çok fazla güneş alıyor — biraz gölge verin."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sun_plant_in_shade_is_too_little() {
        assert_eq!(assess_lux(1_000.0, LightNeed::FullSun), LightFit::TooLittle);
        assert_eq!(assess_lux(60_000.0, LightNeed::FullSun), LightFit::Adequate);
    }

    #[test]
    fn shade_plant_in_full_sun_is_too_much() {
        assert_eq!(assess_lux(60_000.0, LightNeed::Shade), LightFit::TooMuch);
        assert_eq!(assess_lux(2_000.0, LightNeed::Shade), LightFit::Adequate);
        assert_eq!(assess_lux(100.0, LightNeed::Shade), LightFit::TooLittle);
    }

    #[test]
    fn part_shade_band() {
        assert_eq!(assess_lux(2_000.0, LightNeed::PartShade), LightFit::TooLittle);
        assert_eq!(assess_lux(10_000.0, LightNeed::PartShade), LightFit::Adequate);
        assert_eq!(assess_lux(50_000.0, LightNeed::PartShade), LightFit::TooMuch);
    }

    #[test]
    fn comfort_band_boundaries_are_inclusive() {
        // Exactly at min and max counts as adequate.
        let (min, max) = comfort_band(LightNeed::Shade);
        assert_eq!(assess_lux(min, LightNeed::Shade), LightFit::Adequate);
        assert_eq!(assess_lux(max, LightNeed::Shade), LightFit::Adequate);
        // Just outside is not.
        assert_eq!(assess_lux(min - 0.1, LightNeed::Shade), LightFit::TooLittle);
        assert_eq!(assess_lux(max + 0.1, LightNeed::Shade), LightFit::TooMuch);
    }

    #[test]
    fn coarse_band_agrees_with_lux() {
        // A fern (shade) under direct sun: too much.
        assert_eq!(assess_band(LightBand::FullSun, LightNeed::Shade), LightFit::TooMuch);
        // A fern in dim light: adequate.
        assert_eq!(assess_band(LightBand::Dim, LightNeed::Shade), LightFit::Adequate);
        // A tomato (full sun) in the dark: too little.
        assert_eq!(assess_band(LightBand::Dark, LightNeed::FullSun), LightFit::TooLittle);
    }
}
