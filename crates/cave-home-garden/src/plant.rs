//! The plant model — a plant's name plus its care profile.
//!
//! A [`Plant`] pairs a household-friendly name with a [`CareProfile`]: how much
//! sun it wants, the soil-moisture band it is happy in, how it copes with frost,
//! and the air-temperature band it likes. The care engine ([`crate::advice`])
//! reasons purely off this profile and the current outdoor conditions — it owns
//! no sensors and no clock.
//!
//! A handful of common plants ship as named [`PlantKind`] presets so a caller
//! can say `Plant::of(PlantKind::Tomato, "Tomatoes")` without hand-tuning every
//! threshold; bespoke plants are built with [`Plant::custom`].

use crate::light::LightNeed;

/// How tender a plant is to cold — drives [`crate::frost`] classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrostSensitivity {
    /// Damaged by light frost; warn well above freezing (e.g. tomatoes, basil).
    Tender,
    /// Tolerates a touch of frost but not a hard freeze (e.g. lettuce, herbs).
    HalfHardy,
    /// Shrugs off frost down to a hard freeze (e.g. kale, many shrubs).
    Hardy,
}

/// A plant's care profile: the bands it is happy in.
///
/// All bands are inclusive ranges. Soil moisture is a percentage (0–100);
/// temperatures are in degrees Celsius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CareProfile {
    /// How much direct light the plant wants.
    pub light_need: LightNeed,
    /// Lowest soil-moisture percentage the plant is happy at (below → too dry).
    pub soil_moisture_min: f64,
    /// Highest soil-moisture percentage the plant is happy at (above → too wet).
    pub soil_moisture_max: f64,
    /// How the plant copes with cold.
    pub frost_sensitivity: FrostSensitivity,
    /// Coolest ambient temperature (°C) the plant is comfortable in.
    pub temp_min_c: f64,
    /// Warmest ambient temperature (°C) the plant is comfortable in.
    pub temp_max_c: f64,
}

/// A named preset for a common plant, so callers need not hand-tune thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantKind {
    /// Sun-loving, thirsty, very frost-tender summer crop.
    Tomato,
    /// Shade-loving, moisture-loving, half-hardy foliage plant.
    Fern,
    /// Sun-loving, drought-tolerant, hardy aromatic shrub.
    Lavender,
    /// Part-shade leafy green, half-hardy, dislikes drying out.
    Lettuce,
    /// Sun-loving, hardy brassica that tolerates real frost.
    Kale,
    /// Drought-tolerant, sun-loving, tender (frost kills) succulent.
    Succulent,
}

impl PlantKind {
    /// The care profile for this preset.
    #[must_use]
    pub const fn profile(self) -> CareProfile {
        match self {
            Self::Tomato => CareProfile {
                light_need: LightNeed::FullSun,
                soil_moisture_min: 40.0,
                soil_moisture_max: 70.0,
                frost_sensitivity: FrostSensitivity::Tender,
                temp_min_c: 12.0,
                temp_max_c: 32.0,
            },
            Self::Fern => CareProfile {
                light_need: LightNeed::Shade,
                soil_moisture_min: 50.0,
                soil_moisture_max: 80.0,
                frost_sensitivity: FrostSensitivity::HalfHardy,
                temp_min_c: 10.0,
                temp_max_c: 27.0,
            },
            Self::Lavender => CareProfile {
                light_need: LightNeed::FullSun,
                soil_moisture_min: 15.0,
                soil_moisture_max: 45.0,
                frost_sensitivity: FrostSensitivity::Hardy,
                temp_min_c: 5.0,
                temp_max_c: 35.0,
            },
            Self::Lettuce => CareProfile {
                light_need: LightNeed::PartShade,
                soil_moisture_min: 45.0,
                soil_moisture_max: 75.0,
                frost_sensitivity: FrostSensitivity::HalfHardy,
                temp_min_c: 7.0,
                temp_max_c: 24.0,
            },
            Self::Kale => CareProfile {
                light_need: LightNeed::FullSun,
                soil_moisture_min: 40.0,
                soil_moisture_max: 75.0,
                frost_sensitivity: FrostSensitivity::Hardy,
                temp_min_c: 2.0,
                temp_max_c: 28.0,
            },
            Self::Succulent => CareProfile {
                light_need: LightNeed::FullSun,
                soil_moisture_min: 5.0,
                soil_moisture_max: 35.0,
                frost_sensitivity: FrostSensitivity::Tender,
                temp_min_c: 10.0,
                temp_max_c: 38.0,
            },
        }
    }
}

/// A named plant in the household's garden.
#[derive(Debug, Clone, PartialEq)]
pub struct Plant {
    name: String,
    profile: CareProfile,
}

impl Plant {
    /// Build a plant from a named preset, with a household-friendly name.
    #[must_use]
    pub fn of(kind: PlantKind, name: impl Into<String>) -> Self {
        Self { name: name.into(), profile: kind.profile() }
    }

    /// Build a plant with a fully custom care profile.
    #[must_use]
    pub fn custom(name: impl Into<String>, profile: CareProfile) -> Self {
        Self { name: name.into(), profile }
    }

    /// The plant's household-friendly name (e.g. "Tomatoes", "The hallway fern").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The plant's care profile.
    #[must_use]
    pub const fn profile(&self) -> &CareProfile {
        &self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_carries_its_profile() {
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        assert_eq!(tomato.name(), "Tomatoes");
        assert_eq!(tomato.profile().frost_sensitivity, FrostSensitivity::Tender);
        assert_eq!(tomato.profile().light_need, LightNeed::FullSun);
    }

    #[test]
    fn presets_differ_in_the_expected_ways() {
        // Lavender is dry-loving; the fern wants it damp.
        assert!(
            PlantKind::Lavender.profile().soil_moisture_max
                < PlantKind::Fern.profile().soil_moisture_min
        );
        // Kale shrugs off frost; the succulent does not.
        assert_eq!(PlantKind::Kale.profile().frost_sensitivity, FrostSensitivity::Hardy);
        assert_eq!(
            PlantKind::Succulent.profile().frost_sensitivity,
            FrostSensitivity::Tender
        );
    }

    #[test]
    fn custom_profile_round_trips() {
        let p = CareProfile {
            light_need: LightNeed::PartShade,
            soil_moisture_min: 30.0,
            soil_moisture_max: 60.0,
            frost_sensitivity: FrostSensitivity::Hardy,
            temp_min_c: 8.0,
            temp_max_c: 26.0,
        };
        let plant = Plant::custom("My orchid", p);
        assert_eq!(plant.name(), "My orchid");
        assert_eq!(*plant.profile(), p);
    }

    #[test]
    fn every_preset_has_a_sane_moisture_band() {
        for kind in [
            PlantKind::Tomato,
            PlantKind::Fern,
            PlantKind::Lavender,
            PlantKind::Lettuce,
            PlantKind::Kale,
            PlantKind::Succulent,
        ] {
            let p = kind.profile();
            assert!(p.soil_moisture_min < p.soil_moisture_max, "{kind:?} band inverted");
            assert!(p.temp_min_c < p.temp_max_c, "{kind:?} temp band inverted");
        }
    }
}
