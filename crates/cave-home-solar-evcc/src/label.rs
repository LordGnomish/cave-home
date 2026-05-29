//! Grandma-friendly status — what the household sees about their car.
//!
//! The Portal tile, the mobile app and the voice reply never see watts, amps,
//! phases or charge modes. They see a [`ChargeStatus`] — a plain-language line
//! in EN / DE / TR (the Charter §6.3 mandatory languages from M1) that says,
//! in home words, what cave-home is doing with the car and why.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// What the household should be told about the car right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeStatus {
    /// Charging from spare sunshine.
    ChargingFromSun,
    /// Charging fast, pulling from the grid because the household asked for it
    /// now.
    ChargingFast,
    /// Topping up from the grid to be ready by the deadline.
    ToppingUpForDeadline,
    /// Paused — there isn't enough sun and we're waiting for more.
    PausedNotEnoughSun,
    /// The car is already charged to the level you asked for.
    AlreadyFull,
    /// Charging is switched off.
    Off,
}

impl ChargeStatus {
    /// The localised, jargon-free line for the household.
    #[must_use]
    pub const fn message(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::ChargingFromSun, Lang::En) => "Charging your car from the sun",
            (Self::ChargingFromSun, Lang::De) => "Ihr Auto lädt mit Sonnenstrom",
            (Self::ChargingFromSun, Lang::Tr) => "Arabanız güneşten şarj oluyor",

            (Self::ChargingFast, Lang::En) => "Charging your car quickly",
            (Self::ChargingFast, Lang::De) => "Ihr Auto lädt schnell",
            (Self::ChargingFast, Lang::Tr) => "Arabanız hızlıca şarj oluyor",

            (Self::ToppingUpForDeadline, Lang::En) => {
                "Topping up from the grid to reach your deadline"
            }
            (Self::ToppingUpForDeadline, Lang::De) => {
                "Wird aus dem Netz nachgeladen, um rechtzeitig fertig zu sein"
            }
            (Self::ToppingUpForDeadline, Lang::Tr) => {
                "Zamanında hazır olması için şebekeden takviye ediliyor"
            }

            (Self::PausedNotEnoughSun, Lang::En) => "Paused — not enough sun",
            (Self::PausedNotEnoughSun, Lang::De) => "Pausiert — nicht genug Sonne",
            (Self::PausedNotEnoughSun, Lang::Tr) => "Duraklatıldı — yeterli güneş yok",

            (Self::AlreadyFull, Lang::En) => "Your car is already charged",
            (Self::AlreadyFull, Lang::De) => "Ihr Auto ist bereits geladen",
            (Self::AlreadyFull, Lang::Tr) => "Arabanız zaten şarj edildi",

            (Self::Off, Lang::En) => "Car charging is off",
            (Self::Off, Lang::De) => "Das Laden des Autos ist aus",
            (Self::Off, Lang::Tr) => "Araba şarjı kapalı",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[ChargeStatus] = &[
        ChargeStatus::ChargingFromSun,
        ChargeStatus::ChargingFast,
        ChargeStatus::ToppingUpForDeadline,
        ChargeStatus::PausedNotEnoughSun,
        ChargeStatus::AlreadyFull,
        ChargeStatus::Off,
    ];
    const LANGS: &[Lang] = &[Lang::En, Lang::De, Lang::Tr];

    #[test]
    fn every_status_has_all_three_languages() {
        for &s in ALL {
            for &l in LANGS {
                assert!(!s.message(l).is_empty(), "{s:?}/{l:?} missing");
            }
        }
    }

    #[test]
    fn home_words_are_present() {
        assert!(ChargeStatus::ChargingFromSun.message(Lang::En).contains("sun"));
        assert!(ChargeStatus::ChargingFromSun.message(Lang::En).contains("car"));
        assert!(ChargeStatus::ToppingUpForDeadline.message(Lang::En).contains("grid"));
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol / electrical /
        // infrastructure terms. The household sees "your car", "the sun",
        // "the grid" — never the machinery behind them.
        const BANNED: &[&str] = &[
            "OCPP",
            "EVSE",
            "wallbox",
            "Modbus",
            "phase",
            "contactor",
            "ampere",
            "amp",
            "watt",
            "kWh",
            "SoC",
            "PV",
            "hysteresis",
            "setpoint",
            "MQTT",
            "entity_id",
            "loadpoint",
            "pod",
            "kubelet",
        ];
        for &s in ALL {
            for &l in LANGS {
                let text = s.message(l).to_lowercase();
                for banned in BANNED {
                    assert!(
                        !text.contains(&banned.to_lowercase()),
                        "status {s:?} ({l:?}) leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
