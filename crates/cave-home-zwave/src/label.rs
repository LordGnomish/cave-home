// SPDX-License-Identifier: Apache-2.0
//! Grandma-friendly, localized labels for decoded values (Charter §6.3, ADR-007).
//!
//! Nothing in the protocol layer ever reaches a household member. This module
//! turns a [`Value`] (and a room name they chose) into a sentence a grandmother
//! reads: "Bedroom switch on", "Sensor: 21°", "Battery low" — in EN / DE / TR,
//! the Charter §6.3 mandatory languages from M1.
//!
//! It deliberately knows nothing about Command Classes, node ids, endpoints or
//! scales: those are protocol facts the [`crate::command_class`] layer has
//! already resolved into a plain [`Value`].

use crate::value::{Quantity, TemperatureUnit, Value};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lang {
    /// English.
    En,
    /// German.
    De,
    /// Turkish.
    Tr,
}

/// Render a decoded value as a household sentence, optionally prefixed with a
/// room/device name the user chose (e.g. "Bedroom").
///
/// The protocol never appears: a switch is a "switch", a sensor reading is a
/// "sensor" number, a low battery is "battery low".
#[must_use]
pub fn describe(value: &Value, place: Option<&str>, lang: Lang) -> String {
    let body = describe_value(value, lang);
    match place {
        Some(p) if !p.is_empty() => format!("{p} {body}"),
        _ => capitalize_first(&body),
    }
}

fn describe_value(value: &Value, lang: Lang) -> String {
    match value {
        Value::Bool(on) => on_off(*on, lang).to_string(),

        Value::Level(level) => match lang {
            Lang::En => format!("brightness {level}%"),
            Lang::De => format!("Helligkeit {level}%"),
            Lang::Tr => format!("parlaklık %{level}"),
        },

        Value::Temperature { value, unit } => {
            let degree = match unit {
                TemperatureUnit::Celsius => "°",
                TemperatureUnit::Fahrenheit => "°F",
            };
            let n = round1(*value);
            match lang {
                Lang::En => format!("sensor: {n}{degree}"),
                Lang::De => format!("Sensor: {n}{degree}"),
                Lang::Tr => format!("sensör: {n}{degree}"),
            }
        }

        Value::Humidity(pct) => {
            let n = round1(*pct);
            match lang {
                Lang::En => format!("humidity {n}%"),
                Lang::De => format!("Feuchtigkeit {n}%"),
                Lang::Tr => format!("nem %{n}"),
            }
        }

        Value::Measurement { value, quantity } => {
            let n = round1(*value);
            let what = quantity_word(*quantity, lang);
            // The word is already localized; the number reads the same after it
            // in all three languages.
            format!("{what} {n}")
        }

        Value::BatteryPercent(pct) => match lang {
            Lang::En => format!("battery {pct}%"),
            Lang::De => format!("Batterie {pct}%"),
            Lang::Tr => format!("pil %{pct}"),
        },

        Value::BatteryLow => match lang {
            Lang::En => "battery low".to_string(),
            Lang::De => "Batterie schwach".to_string(),
            Lang::Tr => "pil zayıf".to_string(),
        },

        Value::ColorComponent { .. } => match lang {
            Lang::En => "colour set".to_string(),
            Lang::De => "Farbe eingestellt".to_string(),
            Lang::Tr => "renk ayarlandı".to_string(),
        },

        Value::Notification { .. } => match lang {
            Lang::En => "alert".to_string(),
            Lang::De => "Hinweis".to_string(),
            Lang::Tr => "uyarı".to_string(),
        },

        Value::ConfigParam { .. } => match lang {
            Lang::En => "setting changed".to_string(),
            Lang::De => "Einstellung geändert".to_string(),
            Lang::Tr => "ayar değişti".to_string(),
        },
    }
}

const fn on_off(on: bool, lang: Lang) -> &'static str {
    match (on, lang) {
        (true, Lang::En) => "switch on",
        (false, Lang::En) => "switch off",
        (true, Lang::De) => "Schalter an",
        (false, Lang::De) => "Schalter aus",
        (true, Lang::Tr) => "anahtar açık",
        (false, Lang::Tr) => "anahtar kapalı",
    }
}

const fn quantity_word(q: Quantity, lang: Lang) -> &'static str {
    match (q, lang) {
        (Quantity::Temperature, Lang::En) => "temperature",
        (Quantity::Temperature, Lang::De) => "Temperatur",
        (Quantity::Temperature, Lang::Tr) => "sıcaklık",
        (Quantity::Humidity, Lang::En) => "humidity",
        (Quantity::Humidity, Lang::De) => "Feuchtigkeit",
        (Quantity::Humidity, Lang::Tr) => "nem",
        (Quantity::Luminance, Lang::En) => "brightness",
        (Quantity::Luminance, Lang::De) => "Helligkeit",
        (Quantity::Luminance, Lang::Tr) => "ışık",
        (Quantity::Power, Lang::En) => "power",
        (Quantity::Power, Lang::De) => "Leistung",
        (Quantity::Power, Lang::Tr) => "güç",
        (Quantity::Energy, Lang::En) => "energy",
        (Quantity::Energy, Lang::De) => "Energie",
        (Quantity::Energy, Lang::Tr) => "enerji",
        (Quantity::Generic, Lang::En) => "sensor",
        (Quantity::Generic, Lang::De) => "Sensor",
        (Quantity::Generic, Lang::Tr) => "sensör",
    }
}

/// Format a value to at most one decimal place, dropping a trailing ".0".
fn round1(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract().abs() < 1e-9 {
        // Whole number after rounding: print it without a decimal point. The
        // cast is intentional and only reached when `r` has no fractional part.
        #[allow(clippy::cast_possible_truncation)]
        let whole = r as i64;
        format!("{whole}")
    } else {
        format!("{r:.1}")
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_on_with_room() {
        let v = Value::Bool(true);
        assert_eq!(describe(&v, Some("Bedroom"), Lang::En), "Bedroom switch on");
        assert_eq!(describe(&v, Some("Schlafzimmer"), Lang::De), "Schlafzimmer Schalter an");
        assert_eq!(describe(&v, Some("Yatak odası"), Lang::Tr), "Yatak odası anahtar açık");
    }

    #[test]
    fn temperature_reads_as_sensor_degrees() {
        let v = Value::Temperature { value: 21.0, unit: TemperatureUnit::Celsius };
        assert_eq!(describe(&v, None, Lang::En), "Sensor: 21°");
        assert_eq!(describe(&v, None, Lang::Tr), "Sensör: 21°");
    }

    #[test]
    fn fractional_temperature_keeps_one_decimal() {
        let v = Value::Temperature { value: 24.4, unit: TemperatureUnit::Celsius };
        assert_eq!(describe(&v, None, Lang::En), "Sensor: 24.4°");
    }

    #[test]
    fn battery_low_in_all_languages() {
        let v = Value::BatteryLow;
        assert_eq!(describe(&v, None, Lang::En), "Battery low");
        assert_eq!(describe(&v, None, Lang::De), "Batterie schwach");
        assert_eq!(describe(&v, None, Lang::Tr), "Pil zayıf");
    }

    #[test]
    fn level_renders_as_brightness() {
        let v = Value::Level(50);
        assert_eq!(describe(&v, Some("Lamp"), Lang::En), "Lamp brightness 50%");
    }

    #[test]
    fn every_value_renders_non_empty_in_three_languages() {
        let samples = [
            Value::Bool(false),
            Value::Level(0),
            Value::Temperature { value: 19.5, unit: TemperatureUnit::Fahrenheit },
            Value::Humidity(40.0),
            Value::Measurement { value: 100.0, quantity: Quantity::Power },
            Value::BatteryPercent(80),
            Value::BatteryLow,
            Value::ColorComponent { component: 2, intensity: 255 },
            Value::Notification { notification_type: 1, event: 2 },
            Value::ConfigParam { parameter: 3, value: 1 },
        ];
        for v in &samples {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!describe(v, None, lang).is_empty());
                assert!(!describe(v, Some("Room"), lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: never surface protocol/cluster terms to a household.
        const BANNED: &[&str] = &[
            "Command Class",
            "CC",
            "node",
            "endpoint",
            "MQTT",
            "Zigbee",
            "Z-Wave",
            "precision",
            "scale",
            "payload",
            "0x",
            "Multilevel",
            "Notification CC",
        ];
        let samples = [
            Value::Bool(true),
            Value::Bool(false),
            Value::Level(50),
            Value::Temperature { value: 21.0, unit: TemperatureUnit::Celsius },
            Value::Humidity(42.0),
            Value::Measurement { value: 7.0, quantity: Quantity::Luminance },
            Value::Measurement { value: 7.0, quantity: Quantity::Energy },
            Value::BatteryPercent(90),
            Value::BatteryLow,
            Value::ColorComponent { component: 4, intensity: 10 },
            Value::Notification { notification_type: 1, event: 2 },
            Value::ConfigParam { parameter: 5, value: -1 },
        ];
        for v in &samples {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = describe(v, Some("Room"), lang);
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "value {v:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
