//! The care-advice engine — the brain of cave-home-garden.
//!
//! Given a [`Plant`] and the current outdoor [`Conditions`] (soil moisture,
//! light, ambient temperature, forecast overnight minimum, and the month), this
//! produces [`CareAdvice`]: a frost verdict, a light verdict, a soil verdict,
//! whether the plant wants water, whether the air is too cold or too hot, and a
//! season flag — each a clearly-named decision the UI can explain in EN/DE/TR.
//!
//! The engine is a **pure function of its inputs**. It owns no sensors, no
//! weather feed and no clock: the caller supplies every reading. The watering
//! itself is deferred to cave-home-water (ADR-013) — this engine only produces
//! a [`WaterRecommendation`] (see [`crate::moisture`]).

use crate::frost::FrostRisk;
use crate::label::Lang;
use crate::light::{self, LightBand, LightFit};
use crate::moisture::{self, SoilFit, WaterRecommendation};
use crate::plant::Plant;
use crate::season::{self, Hemisphere, Season};

/// How the plant is doing on air temperature, against its comfort band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempFit {
    /// Colder than the plant likes.
    TooCold,
    /// Within the plant's comfortable band.
    Comfortable,
    /// Hotter than the plant likes.
    TooHot,
}

/// The light the plant is getting, either calibrated or coarse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightInput {
    /// A measured illuminance in lux.
    Lux(f64),
    /// A coarse dark/dim/bright/full-sun band.
    Band(LightBand),
}

/// The current outdoor conditions around one plant. The caller fills every
/// field from its own sensors / weather feed / clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    /// Measured soil moisture, percent (0–100).
    pub soil_moisture_percent: f64,
    /// The light the plant is currently getting.
    pub light: LightInput,
    /// Current ambient air temperature, °C.
    pub ambient_temp_c: f64,
    /// Forecast overnight minimum temperature, °C.
    pub forecast_min_c: f64,
    /// Current month, `1..=12` (1 = January).
    pub month: u8,
    /// Which hemisphere the garden is in (drives the season flag).
    pub hemisphere: Hemisphere,
}

/// The full set of care verdicts for one plant under given conditions.
#[derive(Debug, Clone, PartialEq)]
pub struct CareAdvice {
    /// Overnight frost risk for this plant.
    pub frost: FrostRisk,
    /// How the light fits the plant's need.
    pub light: LightFit,
    /// How the soil moisture fits the plant's band.
    pub soil: SoilFit,
    /// How the air temperature fits the plant's comfort band.
    pub temperature: TempFit,
    /// A watering recommendation to hand to cave-home-water, when the plant is
    /// too dry. `None` when no watering is recommended.
    pub water: Option<WaterRecommendation>,
    /// Whether the plant is in its growing season or dormant.
    pub season: Season,
}

/// Assess air temperature against a plant's comfort band (inclusive).
#[must_use]
fn assess_temp(ambient_c: f64, profile: &crate::plant::CareProfile) -> TempFit {
    if ambient_c < profile.temp_min_c {
        TempFit::TooCold
    } else if ambient_c > profile.temp_max_c {
        TempFit::TooHot
    } else {
        TempFit::Comfortable
    }
}

impl TempFit {
    /// A plain-language note for the household (Charter §6.3).
    #[must_use]
    pub fn message(self, plant_name: &str, lang: Lang) -> String {
        match (self, lang) {
            (Self::Comfortable, Lang::En) => format!("{plant_name} are comfortable."),
            (Self::Comfortable, Lang::De) => format!("{plant_name} fühlen sich wohl."),
            (Self::Comfortable, Lang::Tr) => format!("{plant_name} rahat."),
            (Self::TooCold, Lang::En) => format!("It is too cold out for {plant_name}."),
            (Self::TooCold, Lang::De) => format!("Es ist zu kalt draußen für {plant_name}."),
            (Self::TooCold, Lang::Tr) => format!("Dışarısı {plant_name} için fazla soğuk."),
            (Self::TooHot, Lang::En) => format!("It is too hot out for {plant_name}."),
            (Self::TooHot, Lang::De) => format!("Es ist zu heiß draußen für {plant_name}."),
            (Self::TooHot, Lang::Tr) => format!("Dışarısı {plant_name} için fazla sıcak."),
        }
    }
}

/// Produce care advice for one plant under the given conditions.
#[must_use]
pub fn advise(plant: &Plant, conditions: &Conditions) -> CareAdvice {
    let profile = plant.profile();

    let frost = FrostRisk::classify(conditions.forecast_min_c, profile.frost_sensitivity);

    let light = match conditions.light {
        LightInput::Lux(lux) => light::assess_lux(lux, profile.light_need),
        LightInput::Band(band) => light::assess_band(band, profile.light_need),
    };

    let soil = moisture::assess(conditions.soil_moisture_percent, profile);
    let temperature = assess_temp(conditions.ambient_temp_c, profile);
    let water =
        moisture::recommend_water(plant.name(), conditions.soil_moisture_percent, profile);
    let season = season::growing_season_in(conditions.month, conditions.hemisphere);

    CareAdvice { frost, light, soil, temperature, water, season }
}

impl CareAdvice {
    /// Whether anything here warrants the household's attention — a frost risk,
    /// a light or temperature problem, or soil that is too dry or too wet.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.frost.needs_action()
            || self.light != LightFit::Adequate
            || self.soil != SoilFit::Ideal
            || self.temperature != TempFit::Comfortable
    }

    /// The single most-actionable plain-language line for this plant, in the
    /// household's language (Charter §6.3). Frost danger trumps everything;
    /// otherwise the most pressing care issue is surfaced, falling back to an
    /// all-clear.
    #[must_use]
    pub fn headline(&self, plant_name: &str, lang: Lang) -> String {
        if self.frost.needs_action() {
            return self.frost.message(lang).to_string();
        }
        if self.temperature != TempFit::Comfortable {
            return self.temperature.message(plant_name, lang);
        }
        if self.soil != SoilFit::Ideal {
            return self.soil.message(plant_name, lang);
        }
        if self.light != LightFit::Adequate {
            return self.light.message(plant_name, lang);
        }
        // All clear.
        match lang {
            Lang::En => format!("{plant_name} are happy — nothing to do."),
            Lang::De => format!("{plant_name} sind zufrieden — nichts zu tun."),
            Lang::Tr => format!("{plant_name} mutlu — yapılacak bir şey yok."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::light::LightNeed;
    use crate::plant::{CareProfile, FrostSensitivity, PlantKind};

    fn happy_tomato_conditions() -> Conditions {
        // Tomato wants: full sun, soil 40–70, tender, temp 12–32.
        Conditions {
            soil_moisture_percent: 55.0,
            light: LightInput::Lux(50_000.0),
            ambient_temp_c: 22.0,
            forecast_min_c: 14.0,
            month: 7, // July
            hemisphere: Hemisphere::Northern,
        }
    }

    #[test]
    fn a_happy_plant_needs_no_attention() {
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        let advice = advise(&tomato, &happy_tomato_conditions());
        assert_eq!(advice.frost, FrostRisk::None);
        assert_eq!(advice.light, LightFit::Adequate);
        assert_eq!(advice.soil, SoilFit::Ideal);
        assert_eq!(advice.temperature, TempFit::Comfortable);
        assert!(advice.water.is_none());
        assert_eq!(advice.season, Season::Growing);
        assert!(!advice.needs_attention());
        assert_eq!(advice.headline("Tomatoes", Lang::En), "Tomatoes are happy — nothing to do.");
    }

    #[test]
    fn dry_tomato_gets_a_water_recommendation_and_headline() {
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        let mut c = happy_tomato_conditions();
        c.soil_moisture_percent = 25.0;
        let advice = advise(&tomato, &c);
        assert_eq!(advice.soil, SoilFit::TooDry);
        assert!(advice.water.is_some());
        assert!(advice.needs_attention());
        assert_eq!(advice.headline("Tomatoes", Lang::En), "Tomatoes need water.");
    }

    #[test]
    fn frost_danger_trumps_everything_in_the_headline() {
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        let mut c = happy_tomato_conditions();
        c.soil_moisture_percent = 20.0; // also too dry
        c.forecast_min_c = -2.0; // hard freeze for a tender plant
        let advice = advise(&tomato, &c);
        assert_eq!(advice.frost, FrostRisk::Danger);
        assert!(advice.water.is_some()); // still recommends water as data
        // But the headline leads with the frost — the urgent thing.
        assert_eq!(
            advice.headline("Tomatoes", Lang::En),
            "Hard freeze tonight — bring tender plants inside or cover them well."
        );
    }

    #[test]
    fn fern_in_full_sun_is_too_much_light() {
        let fern = Plant::of(PlantKind::Fern, "The hallway fern");
        let c = Conditions {
            soil_moisture_percent: 65.0,
            light: LightInput::Band(LightBand::FullSun),
            ambient_temp_c: 20.0,
            forecast_min_c: 12.0,
            month: 6,
            hemisphere: Hemisphere::Northern,
        };
        let advice = advise(&fern, &c);
        assert_eq!(advice.light, LightFit::TooMuch);
        assert_eq!(
            advice.headline("The hallway fern", Lang::En),
            "The hallway fern is getting too much sun — give it some shade."
        );
    }

    #[test]
    fn too_cold_air_is_reported() {
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        let mut c = happy_tomato_conditions();
        c.ambient_temp_c = 5.0; // below tomato's 12 °C min
        c.forecast_min_c = 10.0; // not yet frost
        let advice = advise(&tomato, &c);
        assert_eq!(advice.temperature, TempFit::TooCold);
        assert_eq!(advice.frost, FrostRisk::None);
        assert!(advice.headline("Tomatoes", Lang::En).contains("too cold"));
    }

    #[test]
    fn too_hot_air_is_reported() {
        let lettuce = Plant::of(PlantKind::Lettuce, "Lettuce");
        // Lettuce max is 24 °C.
        let c = Conditions {
            soil_moisture_percent: 60.0,
            light: LightInput::Lux(10_000.0),
            ambient_temp_c: 33.0,
            forecast_min_c: 15.0,
            month: 7,
            hemisphere: Hemisphere::Northern,
        };
        let advice = advise(&lettuce, &c);
        assert_eq!(advice.temperature, TempFit::TooHot);
    }

    #[test]
    fn dormant_season_flag_in_winter() {
        let kale = Plant::of(PlantKind::Kale, "Kale");
        let mut c = happy_tomato_conditions();
        c.month = 1; // January, northern hemisphere
        let advice = advise(&kale, &c);
        assert_eq!(advice.season, Season::Dormant);
    }

    #[test]
    fn temp_band_boundaries_are_inclusive() {
        let p = CareProfile {
            light_need: LightNeed::FullSun,
            soil_moisture_min: 30.0,
            soil_moisture_max: 60.0,
            frost_sensitivity: FrostSensitivity::Hardy,
            temp_min_c: 10.0,
            temp_max_c: 30.0,
        };
        assert_eq!(assess_temp(10.0, &p), TempFit::Comfortable);
        assert_eq!(assess_temp(30.0, &p), TempFit::Comfortable);
        assert_eq!(assess_temp(9.9, &p), TempFit::TooCold);
        assert_eq!(assess_temp(30.1, &p), TempFit::TooHot);
    }

    #[test]
    fn water_recommendation_defers_to_water_crate_no_runtime() {
        // The recommendation is a flag + reason, never a runtime/valve/circuit.
        let tomato = Plant::of(PlantKind::Tomato, "Tomatoes");
        let mut c = happy_tomato_conditions();
        c.soil_moisture_percent = 30.0;
        let advice = advise(&tomato, &c);
        let rec = advice.water.expect("dry plant recommends water");
        assert_eq!(rec.plant_name, "Tomatoes");
        assert!((rec.deficit_percent - 10.0).abs() < 1e-9); // 40 - 30
    }
}
