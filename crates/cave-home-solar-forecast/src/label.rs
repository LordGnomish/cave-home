//! Plain-language summaries of the solar forecast (Charter §6.3, ADR-007).
//!
//! The household never sees a W/m², an azimuth or a kWh of "plane-of-array
//! irradiance". They see "Sunny — solar is covering the house" and "Peak solar
//! around noon", in English, German or Turkish (the Charter §6.3 mandatory
//! languages from M1). This module turns a [`DailyForecast`] into those
//! phrases.

use crate::forecast::DailyForecast;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// A grandma-friendly summary of how much sun the day will bring.
///
/// Bands run from a bright clear day down to an overcast one, mirroring the
/// cloud-cover input rather than any raw irradiance number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolarOutlook {
    /// Clear or nearly clear — solar is doing the heavy lifting.
    Sunny,
    /// A mix of sun and cloud — some solar, not a full day of it.
    PartlyCloudy,
    /// Mostly or fully overcast — little solar today.
    Cloudy,
}

impl SolarOutlook {
    /// Pick an outlook from a cloud-cover fraction (0 = clear, 1 = overcast).
    #[must_use]
    pub fn from_cloud_cover(cloud_cover: f64) -> Self {
        let c = cloud_cover.clamp(0.0, 1.0);
        if c < 0.30 {
            Self::Sunny
        } else if c < 0.70 {
            Self::PartlyCloudy
        } else {
            Self::Cloudy
        }
    }

    /// A short, friendly headline for the day's solar.
    #[must_use]
    pub const fn headline(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Sunny, Lang::En) => "Sunny — solar is covering the house",
            (Self::Sunny, Lang::De) => "Sonnig — die Sonne versorgt das Haus",
            (Self::Sunny, Lang::Tr) => "Güneşli — güneş evi besliyor",
            (Self::PartlyCloudy, Lang::En) => "Some sun today — solar helps out",
            (Self::PartlyCloudy, Lang::De) => "Etwas Sonne heute — die Sonne hilft mit",
            (Self::PartlyCloudy, Lang::Tr) => "Bugün biraz güneş var — güneş katkı sağlıyor",
            (Self::Cloudy, Lang::En) => "Cloudy — little sun from the panels today",
            (Self::Cloudy, Lang::De) => "Bewölkt — heute wenig Sonne vom Dach",
            (Self::Cloudy, Lang::Tr) => "Bulutlu — bugün çatıdan az güneş",
        }
    }
}

/// The part of the day where solar production is at its strongest, in friendly
/// terms based on the solar time of the peak.
#[must_use]
pub fn peak_time_phrase(forecast: &DailyForecast, lang: Lang) -> &'static str {
    let h = forecast.peak_solar_time_h;
    if h < 11.0 {
        match lang {
            Lang::En => "Most solar in the late morning",
            Lang::De => "Die meiste Sonne am späten Vormittag",
            Lang::Tr => "En çok güneş öğleden önce",
        }
    } else if h <= 13.0 {
        match lang {
            Lang::En => "Peak solar around noon",
            Lang::De => "Die meiste Sonne um die Mittagszeit",
            Lang::Tr => "En çok güneş öğle saatlerinde",
        }
    } else {
        match lang {
            Lang::En => "Most solar in the early afternoon",
            Lang::De => "Die meiste Sonne am frühen Nachmittag",
            Lang::Tr => "En çok güneş öğleden sonra",
        }
    }
}

/// A one-line, plain-language summary of the whole day's forecast.
///
/// Combines the outlook headline, the rounded energy for the day and the peak
/// timing — all in home-world words, never an irradiance or geometry term.
#[must_use]
pub fn daily_summary(forecast: &DailyForecast, lang: Lang) -> String {
    let outlook = SolarOutlook::from_cloud_cover(forecast.cloud_cover);
    let kwh = forecast.energy_kwh.round();
    let energy_phrase = match lang {
        Lang::En => format!("about {kwh:.0} units of solar power for the day"),
        Lang::De => format!("etwa {kwh:.0} Einheiten Sonnenstrom für den Tag"),
        Lang::Tr => format!("gün için yaklaşık {kwh:.0} birim güneş enerjisi"),
    };
    format!(
        "{}. {}. {}.",
        outlook.headline(lang),
        energy_phrase,
        peak_time_phrase(forecast, lang)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forecast(cloud: f64, peak_h: f64) -> DailyForecast {
        DailyForecast {
            energy_kwh: 30.0,
            peak_power_kw: 5.0,
            peak_solar_time_h: peak_h,
            daylight_hours: 16.0,
            cloud_cover: cloud,
        }
    }

    #[test]
    fn outlook_bands_from_cloud_cover() {
        assert_eq!(SolarOutlook::from_cloud_cover(0.0), SolarOutlook::Sunny);
        assert_eq!(SolarOutlook::from_cloud_cover(0.2), SolarOutlook::Sunny);
        assert_eq!(SolarOutlook::from_cloud_cover(0.5), SolarOutlook::PartlyCloudy);
        assert_eq!(SolarOutlook::from_cloud_cover(0.9), SolarOutlook::Cloudy);
        // Out-of-range cover is clamped, not panicked on.
        assert_eq!(SolarOutlook::from_cloud_cover(-1.0), SolarOutlook::Sunny);
        assert_eq!(SolarOutlook::from_cloud_cover(2.0), SolarOutlook::Cloudy);
    }

    #[test]
    fn all_outlooks_have_three_language_headlines() {
        for outlook in [SolarOutlook::Sunny, SolarOutlook::PartlyCloudy, SolarOutlook::Cloudy] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!outlook.headline(lang).is_empty());
            }
        }
    }

    #[test]
    fn peak_phrase_tracks_time_of_day() {
        let morning = forecast(0.0, 9.5);
        let noon = forecast(0.0, 12.0);
        let afternoon = forecast(0.0, 15.0);
        assert!(peak_time_phrase(&morning, Lang::En).contains("morning"));
        assert!(peak_time_phrase(&noon, Lang::En).contains("noon"));
        assert!(peak_time_phrase(&afternoon, Lang::En).contains("afternoon"));
        // All languages produce something for each band.
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!peak_time_phrase(&noon, lang).is_empty());
        }
    }

    #[test]
    fn daily_summary_is_friendly_and_localized() {
        let sunny = forecast(0.0, 12.0);
        assert!(daily_summary(&sunny, Lang::En).contains("Sunny"));
        assert!(daily_summary(&sunny, Lang::De).contains("Sonnig"));
        assert!(daily_summary(&sunny, Lang::Tr).contains("Güneşli"));
        let cloudy = forecast(0.9, 12.0);
        assert!(daily_summary(&cloudy, Lang::En).contains("Cloudy"));
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface engineering terms.
        const BANNED: &[&str] = &[
            "W/m²", "DNI", "GHI", "irradiance", "azimuth", "kWp", "kWh",
            "declination", "air mass", "MQTT", "entity_id", "API", "PVGIS",
            "transmittance", "derate", "plane-of-array",
        ];
        let f = forecast(0.0, 12.0);
        for outlook in [SolarOutlook::Sunny, SolarOutlook::PartlyCloudy, SolarOutlook::Cloudy] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = format!(
                    "{} {} {}",
                    outlook.headline(lang),
                    peak_time_phrase(&f, lang),
                    daily_summary(&f, lang)
                );
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "outlook {outlook:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
