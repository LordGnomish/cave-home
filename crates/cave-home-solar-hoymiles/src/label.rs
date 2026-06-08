//! Grandma-friendly status + localisation (Charter §6.3, ADR-007).
//!
//! The radio protocol, CRC names, register offsets and family model numbers
//! never reach the end-user. The Portal and voice replies show a plain status
//! and a one-line summary in EN / DE / TR (the Charter §6.3 languages).

use crate::telemetry::Telemetry;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// The operating state of an inverter, in household terms.
///
/// This maps the inverter's reported condition (producing, idle, faulted) onto
/// something a household can act on — never a raw alarm code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolarStatus {
    /// Producing power right now.
    Producing,
    /// Connected and healthy but not producing (e.g. before sunrise).
    Idle,
    /// At least one panel is shaded or under-performing versus the others.
    PanelShaded,
    /// The grid voltage / frequency is out of the safe range.
    GridProblem,
    /// The inverter is too hot and has throttled or stopped.
    Overheated,
    /// No telemetry — the inverter is offline (often: no sun yet).
    Offline,
}

/// Raw operating condition reported by the inverter, before friendly mapping.
///
/// Clean-room: these correspond to the documented Hoymiles producing / idle /
/// alarm states, named in our own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmState {
    /// Normal operation.
    Ok,
    /// Grid out-of-range fault (voltage or frequency).
    GridFault,
    /// Over-temperature protection tripped.
    OverTemperature,
    /// Inverter not reachable.
    NotReachable,
}

impl SolarStatus {
    /// Derive a household status from a telemetry snapshot and the inverter's
    /// reported alarm state.
    ///
    /// A panel is flagged as shaded when one panel produces far less than the
    /// strongest one while the inverter is otherwise producing.
    #[must_use]
    pub fn from_telemetry(t: &Telemetry, alarm: AlarmState) -> Self {
        match alarm {
            AlarmState::NotReachable => return Self::Offline,
            AlarmState::GridFault => return Self::GridProblem,
            AlarmState::OverTemperature => return Self::Overheated,
            AlarmState::Ok => {}
        }
        if t.ac_power_w <= 0.0 {
            return Self::Idle;
        }
        if Self::has_shaded_panel(t) {
            return Self::PanelShaded;
        }
        Self::Producing
    }

    // A panel is "shaded" if it makes less than 40% of the best panel's power
    // while the array as a whole is meaningfully producing.
    fn has_shaded_panel(t: &Telemetry) -> bool {
        let best = t
            .panels
            .iter()
            .map(|p| p.power_w)
            .fold(0.0_f64, f64::max);
        if best < 20.0 || t.panels.len() < 2 {
            return false;
        }
        t.panels.iter().any(|p| p.power_w < best * 0.4)
    }

    /// The one-based index of the weakest (likely shaded) panel, if any panel
    /// is under-performing.
    #[must_use]
    pub fn shaded_panel_index(t: &Telemetry) -> Option<usize> {
        if !Self::has_shaded_panel(t) {
            return None;
        }
        t.panels
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.power_w.total_cmp(&b.1.power_w))
            .map(|(i, _)| i + 1)
    }

    /// Localised status name (no jargon — Charter §6.3).
    #[must_use]
    pub const fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Producing, Lang::En) => "Making power",
            (Self::Producing, Lang::De) => "Erzeugt Strom",
            (Self::Producing, Lang::Tr) => "Elektrik üretiyor",
            (Self::Idle, Lang::En) => "Resting",
            (Self::Idle, Lang::De) => "Im Ruhezustand",
            (Self::Idle, Lang::Tr) => "Dinleniyor",
            (Self::PanelShaded, Lang::En) => "A panel is shaded",
            (Self::PanelShaded, Lang::De) => "Ein Panel ist verschattet",
            (Self::PanelShaded, Lang::Tr) => "Bir panel gölgede",
            (Self::GridProblem, Lang::En) => "Grid problem",
            (Self::GridProblem, Lang::De) => "Netzproblem",
            (Self::GridProblem, Lang::Tr) => "Şebeke sorunu",
            (Self::Overheated, Lang::En) => "Too hot",
            (Self::Overheated, Lang::De) => "Zu heiß",
            (Self::Overheated, Lang::Tr) => "Çok sıcak",
            (Self::Offline, Lang::En | Lang::De) => "Offline",
            (Self::Offline, Lang::Tr) => "Çevrimdışı",
        }
    }

    /// A plain-language explanation a household can act on.
    #[must_use]
    pub const fn advice(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Producing, Lang::En) => "Your solar panels are making power.",
            (Self::Producing, Lang::De) => "Ihre Solarmodule erzeugen Strom.",
            (Self::Producing, Lang::Tr) => "Güneş panelleriniz elektrik üretiyor.",
            (Self::Idle, Lang::En) => "All fine — just no sun right now.",
            (Self::Idle, Lang::De) => "Alles in Ordnung — gerade keine Sonne.",
            (Self::Idle, Lang::Tr) => "Her şey yolunda — şu an güneş yok.",
            (Self::PanelShaded, Lang::En) => "One panel is in shade — check for leaves or dirt.",
            (Self::PanelShaded, Lang::De) => "Ein Panel liegt im Schatten — auf Laub oder Schmutz prüfen.",
            (Self::PanelShaded, Lang::Tr) => "Bir panel gölgede — yaprak veya kir olup olmadığına bakın.",
            (Self::GridProblem, Lang::En) => "The house power looks unstable — call an electrician if it lasts.",
            (Self::GridProblem, Lang::De) => "Der Hausstrom wirkt instabil — bei Dauer einen Elektriker rufen.",
            (Self::GridProblem, Lang::Tr) => "Ev elektriği dengesiz görünüyor — sürerse elektrikçi çağırın.",
            (Self::Overheated, Lang::En) => "The inverter is hot and has slowed down — it will recover when it cools.",
            (Self::Overheated, Lang::De) => "Der Wechselrichter ist heiß und drosselt — er erholt sich beim Abkühlen.",
            (Self::Overheated, Lang::Tr) => "Çevirici ısındı ve yavaşladı — soğuyunca düzelir.",
            (Self::Offline, Lang::En) => "No reading yet — probably no sun this early.",
            (Self::Offline, Lang::De) => "Noch keine Daten — wahrscheinlich noch keine Sonne.",
            (Self::Offline, Lang::Tr) => "Henüz veri yok — muhtemelen daha güneş yok.",
        }
    }
}

/// A complete, localised one-line headline for a telemetry snapshot, e.g.
/// "Your solar panels are making 640 W".
#[must_use]
pub fn headline(t: &Telemetry, alarm: AlarmState, lang: Lang) -> String {
    let status = SolarStatus::from_telemetry(t, alarm);
    if status == SolarStatus::Producing {
        // Whole watts for the headline; rounded and clamped into [0, u32::MAX]
        // so the cast below cannot truncate or lose a sign.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "value is clamped into u32 range immediately before the cast"
        )]
        let watts = t.ac_power_w.round().clamp(0.0, f64::from(u32::MAX)) as u32;
        return match lang {
            Lang::En => format!("Your solar panels are making {watts} W"),
            Lang::De => format!("Ihre Solarmodule erzeugen {watts} W"),
            Lang::Tr => format!("Güneş panelleriniz {watts} W üretiyor"),
        };
    }
    if status == SolarStatus::PanelShaded {
        if let Some(idx) = SolarStatus::shaded_panel_index(t) {
            return match lang {
                Lang::En => format!("Panel {idx} is shaded"),
                Lang::De => format!("Panel {idx} ist verschattet"),
                Lang::Tr => format!("Panel {idx} gölgede"),
            };
        }
    }
    status.advice(lang).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::PanelReading;

    fn telem(ac_power: f64, panels: Vec<PanelReading>) -> Telemetry {
        Telemetry {
            panels,
            grid_voltage_v: 230.0,
            grid_frequency_hz: 50.0,
            ac_power_w: ac_power,
            today_wh: 100.0,
            total_wh: 1000.0,
            temperature_c: 40.0,
        }
    }

    fn panel(power: f64) -> PanelReading {
        PanelReading { voltage_v: 34.0, current_a: 1.0, power_w: power }
    }

    #[test]
    fn producing_when_ac_power_positive() {
        let t = telem(640.0, vec![panel(330.0), panel(330.0)]);
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::Ok),
            SolarStatus::Producing
        );
    }

    #[test]
    fn idle_when_no_ac_power() {
        let t = telem(0.0, vec![panel(0.0)]);
        assert_eq!(SolarStatus::from_telemetry(&t, AlarmState::Ok), SolarStatus::Idle);
    }

    #[test]
    fn shaded_when_one_panel_far_below_best() {
        let t = telem(400.0, vec![panel(330.0), panel(40.0)]);
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::Ok),
            SolarStatus::PanelShaded
        );
        assert_eq!(SolarStatus::shaded_panel_index(&t), Some(2));
    }

    #[test]
    fn no_shade_flag_for_single_panel() {
        let t = telem(100.0, vec![panel(100.0)]);
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::Ok),
            SolarStatus::Producing
        );
    }

    #[test]
    fn alarm_states_override_production() {
        let t = telem(640.0, vec![panel(640.0)]);
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::GridFault),
            SolarStatus::GridProblem
        );
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::OverTemperature),
            SolarStatus::Overheated
        );
        assert_eq!(
            SolarStatus::from_telemetry(&t, AlarmState::NotReachable),
            SolarStatus::Offline
        );
    }

    #[test]
    fn headline_reports_watts_for_producing() {
        let t = telem(640.4, vec![panel(330.0), panel(330.0)]);
        assert_eq!(
            headline(&t, AlarmState::Ok, Lang::En),
            "Your solar panels are making 640 W"
        );
        assert_eq!(
            headline(&t, AlarmState::Ok, Lang::De),
            "Ihre Solarmodule erzeugen 640 W"
        );
    }

    #[test]
    fn headline_names_shaded_panel() {
        let t = telem(400.0, vec![panel(330.0), panel(40.0)]);
        assert_eq!(headline(&t, AlarmState::Ok, Lang::En), "Panel 2 is shaded");
        assert_eq!(headline(&t, AlarmState::Ok, Lang::Tr), "Panel 2 gölgede");
    }

    #[test]
    fn headline_for_offline_is_friendly() {
        let t = telem(0.0, vec![panel(0.0)]);
        let h = headline(&t, AlarmState::NotReachable, Lang::En);
        assert!(h.to_lowercase().contains("sun"));
    }

    #[test]
    fn all_statuses_have_three_language_strings() {
        for s in [
            SolarStatus::Producing,
            SolarStatus::Idle,
            SolarStatus::PanelShaded,
            SolarStatus::GridProblem,
            SolarStatus::Overheated,
            SolarStatus::Offline,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!s.name(lang).is_empty());
                assert!(!s.advice(lang).is_empty());
            }
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3: the UI must never surface protocol / radio terms.
        const BANNED: &[&str] = &[
            "NRF24", "CMT", "CRC", "Modbus", "MQTT", "opcode", "fragment",
            "register", "DTU", "HM-", "entity_id", "payload",
        ];
        for s in [
            SolarStatus::Producing,
            SolarStatus::Idle,
            SolarStatus::PanelShaded,
            SolarStatus::GridProblem,
            SolarStatus::Overheated,
            SolarStatus::Offline,
        ] {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let text = format!("{} {}", s.name(lang), s.advice(lang));
                for banned in BANNED {
                    assert!(
                        !text.contains(banned),
                        "status {s:?} leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
