//! Grandma-friendly front-door notifications (Charter §6.3, ADR-018).
//!
//! The household never sees "RTSP", "SIP INVITE" or an entity id — they see
//! "Someone is at the front door" / "Es ist jemand an der Haustür" / "Ön kapıda
//! biri var", localised to the Charter §6.3 mandatory languages EN / DE / TR.
//! This module turns a call outcome (and a missed-visit time) into that plain
//! line.

use crate::event::CallState;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// A wall-clock time of day for a notification, in whole hours and minutes.
/// Used to phrase "Missed visitor at 14:30". Validated so formatting never has
/// to defend against junk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOfDay {
    hour: u8,
    minute: u8,
}

/// Why a [`TimeOfDay`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeError {
    /// The hour was not 0..=23 or the minute was not 0..=59.
    OutOfRange,
}

impl core::fmt::Display for TimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("time of day out of range"),
        }
    }
}

impl std::error::Error for TimeError {}

impl TimeOfDay {
    /// Construct a validated 24-hour time of day.
    ///
    /// # Errors
    /// [`TimeError::OutOfRange`] if `hour > 23` or `minute > 59`.
    pub const fn new(hour: u8, minute: u8) -> Result<Self, TimeError> {
        if hour <= 23 && minute <= 59 {
            Ok(Self { hour, minute })
        } else {
            Err(TimeError::OutOfRange)
        }
    }

    /// `HH:MM`, zero-padded — the form every locale here shares.
    #[must_use]
    pub fn hhmm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// Localised front-door labels.
///
/// These are the headline lines a notification or Portal tile shows. They are
/// deliberately plain: a visitor, the front door, motion outside — never a
/// protocol or a device model.
#[must_use]
pub fn ringing(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Someone is at the front door",
        Lang::De => "Es ist jemand an der Haustür",
        Lang::Tr => "Ön kapıda biri var",
    }
}

/// The line for a motion-only alert (no button press).
#[must_use]
pub fn motion(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Front door — motion outside",
        Lang::De => "Haustür — Bewegung draußen",
        Lang::Tr => "Ön kapı — dışarıda hareket",
    }
}

/// The line for an answered visit.
#[must_use]
pub fn answered(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "You answered the front door",
        Lang::De => "Sie haben die Haustür beantwortet",
        Lang::Tr => "Ön kapıyı yanıtladınız",
    }
}

/// The line for a declined visit.
#[must_use]
pub fn declined(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "You turned the visitor away",
        Lang::De => "Sie haben den Besucher abgewiesen",
        Lang::Tr => "Ziyaretçiyi geri çevirdiniz",
    }
}

/// The line for a finished call.
#[must_use]
pub fn ended(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "The front-door call ended",
        Lang::De => "Das Gespräch an der Haustür ist beendet",
        Lang::Tr => "Ön kapı görüşmesi sona erdi",
    }
}

/// A "missed visitor at HH:MM" line for the history / notification.
#[must_use]
pub fn missed_at(lang: Lang, when: TimeOfDay) -> String {
    let t = when.hhmm();
    match lang {
        Lang::En => format!("Missed visitor at {t}"),
        Lang::De => format!("Verpasster Besucher um {t}"),
        Lang::Tr => format!("{t} sularında kaçırılan ziyaretçi"),
    }
}

/// The plain headline for a settled (or in-flight) call state, without a time.
/// `Idle` has no notification, so returns an empty string.
#[must_use]
pub fn for_state(state: CallState, lang: Lang) -> &'static str {
    match state {
        CallState::Idle => "",
        CallState::Ringing => ringing(lang),
        CallState::Answered => answered(lang),
        CallState::Declined => declined(lang),
        CallState::Ended => ended(lang),
        // A bare "missed" headline without a time; use `missed_at` for the
        // time-stamped form.
        CallState::Missed => match lang {
            Lang::En => "You missed a visitor",
            Lang::De => "Sie haben einen Besucher verpasst",
            Lang::Tr => "Bir ziyaretçiyi kaçırdınız",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];

    #[test]
    fn time_of_day_validates_range() {
        assert!(TimeOfDay::new(0, 0).is_ok());
        assert!(TimeOfDay::new(23, 59).is_ok());
        assert_eq!(TimeOfDay::new(24, 0), Err(TimeError::OutOfRange));
        assert_eq!(TimeOfDay::new(12, 60), Err(TimeError::OutOfRange));
    }

    #[test]
    fn hhmm_is_zero_padded() {
        assert_eq!(TimeOfDay::new(9, 5).unwrap().hhmm(), "09:05");
        assert_eq!(TimeOfDay::new(14, 30).unwrap().hhmm(), "14:30");
    }

    #[test]
    fn missed_at_embeds_the_time_in_each_language() {
        let t = TimeOfDay::new(14, 30).unwrap();
        assert_eq!(missed_at(Lang::En, t), "Missed visitor at 14:30");
        assert!(missed_at(Lang::De, t).contains("14:30"));
        assert!(missed_at(Lang::Tr, t).contains("14:30"));
    }

    #[test]
    fn every_state_has_a_line_in_every_language() {
        for state in [
            CallState::Ringing,
            CallState::Answered,
            CallState::Declined,
            CallState::Ended,
            CallState::Missed,
        ] {
            for lang in LANGS {
                assert!(!for_state(state, lang).is_empty(), "{state:?}/{lang:?} empty");
            }
        }
    }

    #[test]
    fn idle_has_no_notification() {
        for lang in LANGS {
            assert_eq!(for_state(CallState::Idle, lang), "");
        }
    }

    #[test]
    fn ringing_and_motion_lines_differ() {
        for lang in LANGS {
            assert_ne!(
                ringing(lang),
                motion(lang),
                "a press and bare motion must read differently"
            );
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-018: the front-door UI must never surface
        // protocol, transport or device-model terms.
        const BANNED: &[&str] = &[
            "RTSP", "SIP", "INVITE", "WebRTC", "MQTT", "entity_id", "entity id",
            "Reolink", "DoorBird", "Doorbird", "Amcrest", "UniFi", "Ring",
            "Aqara", "webhook", "PIR", "pod", "kubelet", "namespace",
        ];
        let t = TimeOfDay::new(14, 30).unwrap();
        let mut texts: Vec<String> = Vec::new();
        for lang in LANGS {
            texts.push(ringing(lang).to_owned());
            texts.push(motion(lang).to_owned());
            texts.push(answered(lang).to_owned());
            texts.push(declined(lang).to_owned());
            texts.push(ended(lang).to_owned());
            texts.push(missed_at(lang, t));
            for state in [
                CallState::Ringing,
                CallState::Answered,
                CallState::Declined,
                CallState::Ended,
                CallState::Missed,
            ] {
                texts.push(for_state(state, lang).to_owned());
            }
        }
        for text in &texts {
            for banned in BANNED {
                assert!(
                    !text.contains(banned),
                    "user-facing string leaks jargon {banned:?}: {text}"
                );
            }
        }
    }
}
