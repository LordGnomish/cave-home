// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Grandma-friendly, localized solar-inverter status (Charter §6.3, ADR-007).
//!
//! The Portal and mobile app never see "Modbus register 40083", "holding
//! register", an `St` enum number, or a model id. They see a plain sentence
//! about the household's solar panels, in EN / DE / TR (the Charter §6.3
//! mandatory languages). "solar inverter", "producing", "sleeping" are
//! home-world words explicitly blessed by the charter; protocol terms are not.

use crate::inverter::{InverterReading, OperatingState};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// A grandma-friendly summary band for the inverter, ignoring the exact
/// numbers — what the household actually cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolarStatus {
    /// Actively producing power from the sun.
    Producing,
    /// Asleep — no sun, nothing to do (normal at night).
    Sleeping,
    /// Off or shutting down.
    Off,
    /// Waking up / coming online.
    Starting,
    /// A fault the installer should look at.
    Fault,
    /// State the device reported that we do not have a friendly word for.
    Unknown,
}

impl SolarStatus {
    /// Map a raw operating state to a friendly band.
    #[must_use]
    pub const fn from_state(state: OperatingState) -> Self {
        match state {
            OperatingState::Mppt | OperatingState::Throttled => Self::Producing,
            OperatingState::Sleeping => Self::Sleeping,
            OperatingState::Off | OperatingState::ShuttingDown | OperatingState::Standby => {
                Self::Off
            }
            OperatingState::Starting => Self::Starting,
            OperatingState::Fault => Self::Fault,
            OperatingState::Other(_) => Self::Unknown,
        }
    }

    /// A short localized status line *without* any production figure. Use
    /// [`describe`] when you have a reading and want the kilowatts woven in.
    #[must_use]
    pub const fn line(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Producing, Lang::En) => "Solar inverter is producing power",
            (Self::Producing, Lang::De) => "Der Solar-Wechselrichter erzeugt Strom",
            (Self::Producing, Lang::Tr) => "Güneş invertörü elektrik üretiyor",
            (Self::Sleeping, Lang::En) => "Inverter is sleeping (no sun)",
            (Self::Sleeping, Lang::De) => "Wechselrichter schläft (keine Sonne)",
            (Self::Sleeping, Lang::Tr) => "İnvertör uyuyor (güneş yok)",
            (Self::Off, Lang::En) => "Solar inverter is off",
            (Self::Off, Lang::De) => "Der Solar-Wechselrichter ist aus",
            (Self::Off, Lang::Tr) => "Güneş invertörü kapalı",
            (Self::Starting, Lang::En) => "Inverter is waking up",
            (Self::Starting, Lang::De) => "Wechselrichter fährt hoch",
            (Self::Starting, Lang::Tr) => "İnvertör açılıyor",
            (Self::Fault, Lang::En) => "Inverter fault — call your installer",
            (Self::Fault, Lang::De) => "Wechselrichter-Störung — rufen Sie Ihren Installateur",
            (Self::Fault, Lang::Tr) => "İnvertör arızası — kurulumcunuzu arayın",
            (Self::Unknown, Lang::En) => "Inverter status is unclear",
            (Self::Unknown, Lang::De) => "Wechselrichter-Status ist unklar",
            (Self::Unknown, Lang::Tr) => "İnvertör durumu belirsiz",
        }
    }
}

/// Round a wattage to a friendly kilowatt string, e.g. `3200.0 → "3.2 kW"`.
fn kw_phrase(power_w: f64, lang: Lang) -> String {
    let kw = (power_w / 1000.0 * 10.0).round() / 10.0;
    match lang {
        // German uses a comma as the decimal separator.
        Lang::De => format!("{kw:.1} kW").replace('.', ","),
        Lang::En | Lang::Tr => format!("{kw:.1} kW"),
    }
}

/// Produce a full grandma-friendly sentence for a reading.
///
/// When the inverter is producing and a power figure is present, the
/// kilowatts are woven in ("Solar inverter is producing 3.2 kW"). Otherwise
/// the plain status line is returned.
#[must_use]
pub fn describe(reading: &InverterReading, lang: Lang) -> String {
    let status = SolarStatus::from_state(reading.state);
    match (status, reading.ac_power_w) {
        (SolarStatus::Producing, Some(power_w)) if power_w > 0.0 => {
            let kw = kw_phrase(power_w, lang);
            match lang {
                Lang::En => format!("Solar inverter is producing {kw}"),
                Lang::De => format!("Der Solar-Wechselrichter erzeugt {kw}"),
                Lang::Tr => format!("Güneş invertörü {kw} üretiyor"),
            }
        }
        _ => status.line(lang).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inverter::InverterPhase;

    fn reading(state: OperatingState, power_w: Option<f64>) -> InverterReading {
        InverterReading {
            phase: InverterPhase::Three,
            ac_power_w: power_w,
            ac_current_a: None,
            ac_voltage_v: None,
            frequency_hz: None,
            dc_power_w: None,
            lifetime_energy_wh: None,
            temperature_c: None,
            state,
        }
    }

    #[test]
    fn status_band_mapping() {
        assert_eq!(SolarStatus::from_state(OperatingState::Mppt), SolarStatus::Producing);
        assert_eq!(SolarStatus::from_state(OperatingState::Throttled), SolarStatus::Producing);
        assert_eq!(SolarStatus::from_state(OperatingState::Sleeping), SolarStatus::Sleeping);
        assert_eq!(SolarStatus::from_state(OperatingState::Off), SolarStatus::Off);
        assert_eq!(SolarStatus::from_state(OperatingState::Fault), SolarStatus::Fault);
        assert_eq!(SolarStatus::from_state(OperatingState::Other(42)), SolarStatus::Unknown);
    }

    #[test]
    fn describe_producing_weaves_in_kilowatts() {
        let r = reading(OperatingState::Mppt, Some(3210.0));
        assert_eq!(describe(&r, Lang::En), "Solar inverter is producing 3.2 kW");
        assert_eq!(describe(&r, Lang::Tr), "Güneş invertörü 3.2 kW üretiyor");
        // German comma decimal.
        assert_eq!(describe(&r, Lang::De), "Der Solar-Wechselrichter erzeugt 3,2 kW");
    }

    #[test]
    fn describe_sleeping_says_no_sun() {
        let r = reading(OperatingState::Sleeping, None);
        assert_eq!(describe(&r, Lang::En), "Inverter is sleeping (no sun)");
        assert!(describe(&r, Lang::De).contains("Sonne"));
        assert!(describe(&r, Lang::Tr).contains("güneş yok"));
    }

    #[test]
    fn describe_fault_tells_user_to_call_installer() {
        let r = reading(OperatingState::Fault, None);
        assert!(describe(&r, Lang::En).contains("call your installer"));
        assert!(describe(&r, Lang::De).contains("Installateur"));
        assert!(describe(&r, Lang::Tr).contains("kurulumcunuzu"));
    }

    #[test]
    fn all_bands_have_three_language_lines() {
        for status in [
            SolarStatus::Producing,
            SolarStatus::Sleeping,
            SolarStatus::Off,
            SolarStatus::Starting,
            SolarStatus::Fault,
            SolarStatus::Unknown,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!status.line(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol/decode terms.
        const BANNED: &[&str] = &[
            "Modbus", "register", "holding", "MQTT", "Zigbee", "enum16",
            "model 10", "model 11", "sunssf", "acc32", "uint16", "int16",
            "float32", "entity_id", "pod", "kubelet", "0x", "0X",
        ];
        let powers = [Some(3210.0), Some(0.0), None];
        let states = [
            OperatingState::Mppt,
            OperatingState::Throttled,
            OperatingState::Sleeping,
            OperatingState::Off,
            OperatingState::ShuttingDown,
            OperatingState::Standby,
            OperatingState::Starting,
            OperatingState::Fault,
            OperatingState::Other(99),
        ];
        for state in states {
            for power in powers {
                for lang in [Lang::En, Lang::De, Lang::Tr] {
                    let text = describe(&reading(state, power), lang);
                    for banned in BANNED {
                        assert!(
                            !text.contains(banned),
                            "state {state:?} leaks jargon {banned:?}: {text}"
                        );
                    }
                }
            }
        }
    }
}
