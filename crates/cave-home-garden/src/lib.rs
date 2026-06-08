//! `cave-home-garden` — outdoor plant-care intelligence for cave-home (ADR-029).
//!
//! This crate is the **brain** that turns a plant's care needs plus the current
//! outdoor conditions into plain-language garden advice a household can act on:
//! does this plant need water, is it too dry or too wet, is frost coming
//! tonight, is it getting too little or too much sun, is the air too cold or too
//! hot — and is the garden growing or resting for the winter. Every verdict is
//! surfaced in EN / DE / TR.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here:
//! - [`plant`] — the plant model: a name plus a [`plant::CareProfile`], with a
//!   set of common-plant presets ([`plant::PlantKind`]).
//! - [`frost`] — overnight frost-risk classification from a forecast minimum
//!   and the plant's frost sensitivity (None / Watch / Warning / Danger).
//! - [`light`] — light assessment: measured lux (or a coarse band) vs. the
//!   plant's light need → too little / adequate / too much.
//! - [`moisture`] — soil-moisture assessment vs. the plant's ideal band, plus a
//!   [`moisture::WaterRecommendation`] handed off to cave-home-water.
//! - [`season`] — the growing-season / dormancy flag from a caller-supplied
//!   month (northern-hemisphere default, documented).
//! - [`advice`] — the combined care engine, the surface the rest of cave-home
//!   consumes.
//! - [`label`] — the localisation surface (Charter §6.3, ADR-007).
//!
//! # Boundaries
//!
//! The engine is a **pure function of its inputs** — it owns no sensors, no
//! weather feed and no clock. The caller supplies soil moisture, light, ambient
//! temperature, the forecast overnight minimum, and the month. The **outdoor
//! sensor adapters** (soil-moisture probes, light/temperature sensors over
//! Zigbee / BLE / `ESPHome`), the **live weather-forecast feed**, the
//! **plant-database import** and the **cave-home-water + cave-home-core
//! integration** are network / hardware / clock-bound and are deferred to Phase
//! 1b — every one is enumerated in `parity.manifest.toml` `[[unmapped]]` with an
//! ADR-029 disposition. In particular the actual watering is **not** done here:
//! this crate only *recommends* watering (a [`moisture::WaterRecommendation`]),
//! which a caller hands to `cave-home-water` (ADR-013). We model that boundary
//! and take no dependency on it.
//!
//! # Example
//!
//! ```
//! use cave_home_garden::{advise, Plant, PlantKind, Conditions, LightInput,
//!     Hemisphere, Lang, FrostRisk};
//!
//! let tomatoes = Plant::of(PlantKind::Tomato, "Tomatoes");
//!
//! // A dry bed on a clear evening with frost forecast for tonight.
//! let conditions = Conditions {
//!     soil_moisture_percent: 25.0,
//!     light: LightInput::Lux(50_000.0),
//!     ambient_temp_c: 8.0,
//!     forecast_min_c: -1.0,
//!     month: 5,
//!     hemisphere: Hemisphere::Northern,
//! };
//!
//! let care = advise(&tomatoes, &conditions);
//! assert_eq!(care.frost, FrostRisk::Danger);
//! assert!(care.water.is_some()); // recommend watering — cave-home-water acts
//!
//! // The household sees one plain-language headline — frost leads.
//! println!("{}", care.headline("Tomatoes", Lang::En));
//! ```

pub mod advice;
pub mod frost;
pub mod label;
pub mod light;
pub mod moisture;
pub mod plant;
pub mod season;

pub use advice::{advise, CareAdvice, Conditions, LightInput, TempFit};
pub use frost::FrostRisk;
pub use label::Lang;
pub use light::{assess_band, assess_lux, LightBand, LightFit, LightNeed};
pub use moisture::{recommend_water, SoilFit, WaterRecommendation};
pub use plant::{CareProfile, FrostSensitivity, Plant, PlantKind};
pub use season::{growing_season, growing_season_in, Hemisphere, Season};

#[cfg(test)]
mod tests {
    use super::*;

    /// Charter §6.3: no implementation jargon may leak into any user-facing
    /// string this crate produces. We exercise every localised surface in all
    /// three languages and assert none contains a banned term. Mirrors the
    /// air-quality crate's `ui_strings_carry_no_implementation_jargon`.
    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        const BANNED: &[&str] = &[
            "lux", "ADC", "probe", "MQTT", "Zigbee", "ESPHome",
            "entity_id", "soil moisture", "threshold", "percent",
            "valve", "kubelet", "Automower", "Landroid",
        ];
        let langs = [Lang::En, Lang::De, Lang::Tr];
        let name = "The tomatoes";

        let mut strings: Vec<String> = Vec::new();

        for lang in langs {
            // Frost messages.
            for risk in [
                FrostRisk::None,
                FrostRisk::Watch,
                FrostRisk::Warning,
                FrostRisk::Danger,
            ] {
                strings.push(risk.message(lang).to_string());
            }
            // Light messages.
            for fit in [LightFit::TooLittle, LightFit::Adequate, LightFit::TooMuch] {
                strings.push(fit.message(name, lang));
            }
            // Soil messages.
            for fit in [SoilFit::TooDry, SoilFit::Ideal, SoilFit::TooWet] {
                strings.push(fit.message(name, lang));
            }
            // Temperature messages.
            for fit in [TempFit::TooCold, TempFit::Comfortable, TempFit::TooHot] {
                strings.push(fit.message(name, lang));
            }
            // Season messages.
            for season in [Season::Growing, Season::Dormant] {
                strings.push(season.message(lang).to_string());
            }
            // Combined headlines, dry/frosty and happy.
            let plant = Plant::of(PlantKind::Tomato, name);
            let dry = Conditions {
                soil_moisture_percent: 20.0,
                light: LightInput::Band(LightBand::Dim),
                ambient_temp_c: 4.0,
                forecast_min_c: -3.0,
                month: 5,
                hemisphere: Hemisphere::Northern,
            };
            let happy = Conditions {
                soil_moisture_percent: 55.0,
                light: LightInput::Lux(50_000.0),
                ambient_temp_c: 22.0,
                forecast_min_c: 14.0,
                month: 7,
                hemisphere: Hemisphere::Northern,
            };
            strings.push(advise(&plant, &dry).headline(name, lang));
            strings.push(advise(&plant, &happy).headline(name, lang));
        }

        for text in &strings {
            for banned in BANNED {
                assert!(
                    !text.to_lowercase().contains(&banned.to_lowercase()),
                    "user-facing string leaks jargon {banned:?}: {text:?}"
                );
            }
        }
    }

    #[test]
    fn engine_produces_a_complete_verdict_for_a_mixed_bed() {
        // A spring evening: tomatoes (dry, frost coming), a fern (too sunny),
        // lavender (perfectly happy).
        let tomatoes = Plant::of(PlantKind::Tomato, "Tomatoes");
        let fern = Plant::of(PlantKind::Fern, "The fern");
        let lavender = Plant::of(PlantKind::Lavender, "Lavender");

        let evening = Conditions {
            soil_moisture_percent: 25.0,
            light: LightInput::Band(LightBand::FullSun),
            ambient_temp_c: 9.0,
            forecast_min_c: -1.0,
            month: 4,
            hemisphere: Hemisphere::Northern,
        };

        let t = advise(&tomatoes, &evening);
        assert_eq!(t.frost, FrostRisk::Danger); // tender, below 0 °C
        assert!(t.water.is_some()); // dry → recommend water (handed to water crate)

        let f = advise(&fern, &evening);
        assert_eq!(f.light, LightFit::TooMuch); // shade plant in full sun

        // Lavender is dry-loving, hardy, sun-loving: comfy on a dry sunny bed,
        // and a hardy plant shrugs off a -1 °C night.
        let dry_sunny = Conditions {
            soil_moisture_percent: 30.0,
            light: LightInput::Band(LightBand::FullSun),
            ambient_temp_c: 18.0,
            forecast_min_c: 3.0,
            month: 6,
            hemisphere: Hemisphere::Northern,
        };
        let l = advise(&lavender, &dry_sunny);
        assert_eq!(l.frost, FrostRisk::None);
        assert_eq!(l.soil, SoilFit::Ideal);
        assert_eq!(l.light, LightFit::Adequate);
        assert!(!l.needs_attention());
    }
}
