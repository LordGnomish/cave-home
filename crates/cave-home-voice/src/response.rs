//! Spoken-reply generation — the assistant's grandma-friendly answer.
//!
//! After an action is routed, the assistant says something back. This module
//! turns a [`IntentAction`] into a natural spoken sentence in the household's
//! language (Charter §6.3: EN / DE / TR). The wording is deliberately plain —
//! "Turning on the living-room light", never "intent slot confidence 0.8".
//!
//! Query actions need the *current* value to answer, so [`respond`] takes an
//! optional answer payload the caller supplies once it has read the state.

use crate::label::Lang;
use crate::route::{IntentAction, QueryKind};

/// Extra information the caller provides so a query can be answered out loud.
/// For a temperature query this is the measured temperature; for an on/off
/// query it is whether the thing is currently on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Answer {
    /// Current temperature in whole °C, if known.
    pub temperature_c: Option<i32>,
    /// Current on/off state, if known.
    pub is_on: Option<bool>,
}

/// Generate the spoken reply for an action.
///
/// `answer` is only consulted for [`IntentAction::QueryState`]; pass
/// `Answer::default()` for command actions. When a query has no answer payload
/// the assistant gives a polite "I can't tell right now" reply rather than
/// inventing a value.
#[must_use]
pub fn respond(action: &IntentAction, lang: Lang, answer: Answer) -> String {
    match action {
        IntentAction::SetLight { target, on } => light_reply(target, *on, lang),
        IntentAction::SetBrightness { target, percent } => brightness_reply(target, *percent, lang),
        IntentAction::SetTemperature { target, celsius } => temp_set_reply(target, *celsius, lang),
        IntentAction::SetCover { target, open } => cover_reply(target, *open, lang),
        IntentAction::ActivateScene { name } => scene_reply(name, lang),
        IntentAction::QueryState { target, what } => query_reply(target, *what, lang, answer),
    }
}

fn light_reply(target: &str, on: bool, lang: Lang) -> String {
    match (lang, on) {
        (Lang::En, true) => format!("Turning on the {target} light."),
        (Lang::En, false) => format!("Turning off the {target} light."),
        (Lang::De, true) => format!("Ich schalte das Licht im {target} ein."),
        (Lang::De, false) => format!("Ich schalte das Licht im {target} aus."),
        (Lang::Tr, true) => format!("{target} ışığını açıyorum."),
        (Lang::Tr, false) => format!("{target} ışığını kapatıyorum."),
    }
}

fn brightness_reply(target: &str, percent: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Setting the {target} light to {percent} percent."),
        Lang::De => format!("Ich stelle das Licht im {target} auf {percent} Prozent."),
        Lang::Tr => format!("{target} ışığını yüzde {percent} yapıyorum."),
    }
}

fn temp_set_reply(target: &str, celsius: u32, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Setting the {target} to {celsius} degrees."),
        Lang::De => format!("Ich stelle {target} auf {celsius} Grad."),
        Lang::Tr => format!("{target} sıcaklığını {celsius} dereceye ayarlıyorum."),
    }
}

fn cover_reply(target: &str, open: bool, lang: Lang) -> String {
    match (lang, open) {
        (Lang::En, true) => format!("Opening the {target}."),
        (Lang::En, false) => format!("Closing the {target}."),
        (Lang::De, true) => format!("Ich öffne {target}."),
        (Lang::De, false) => format!("Ich schließe {target}."),
        (Lang::Tr, true) => format!("{target} açılıyor."),
        (Lang::Tr, false) => format!("{target} kapanıyor."),
    }
}

fn scene_reply(name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Starting {name}."),
        Lang::De => format!("Ich starte {name}."),
        Lang::Tr => format!("{name} başlatılıyor."),
    }
}

fn query_reply(target: &str, what: QueryKind, lang: Lang, answer: Answer) -> String {
    match what {
        QueryKind::Temperature => match answer.temperature_c {
            Some(c) => match lang {
                Lang::En => format!("The {target} is {c} degrees."),
                Lang::De => format!("Im {target} sind es {c} Grad."),
                Lang::Tr => format!("{target} {c} derece."),
            },
            None => unknown_reply(lang),
        },
        QueryKind::OnState => match answer.is_on {
            Some(true) => match lang {
                Lang::En => format!("The {target} light is on."),
                Lang::De => format!("Das Licht im {target} ist an."),
                Lang::Tr => format!("{target} ışığı açık."),
            },
            Some(false) => match lang {
                Lang::En => format!("The {target} light is off."),
                Lang::De => format!("Das Licht im {target} ist aus."),
                Lang::Tr => format!("{target} ışığı kapalı."),
            },
            None => unknown_reply(lang),
        },
    }
}

/// The assistant could not answer — say so plainly rather than guessing.
fn unknown_reply(lang: Lang) -> String {
    match lang {
        Lang::En => "I can't tell right now.".to_string(),
        Lang::De => "Das kann ich gerade nicht sagen.".to_string(),
        Lang::Tr => "Şu anda bunu söyleyemiyorum.".to_string(),
    }
}

/// The reply when nothing was understood. The assistant asks the household to
/// rephrase rather than acting on a guess.
#[must_use]
pub fn not_understood(lang: Lang) -> String {
    match lang {
        Lang::En => "Sorry, I didn't catch that.".to_string(),
        Lang::De => "Entschuldigung, das habe ich nicht verstanden.".to_string(),
        Lang::Tr => "Üzgünüm, anlayamadım.".to_string(),
    }
}

/// The reply when several things could have been meant — the assistant asks
/// the household to be more specific.
#[must_use]
pub fn please_clarify(lang: Lang) -> String {
    match lang {
        Lang::En => "Did you mean one thing or another? Please say it again.".to_string(),
        Lang::De => "Meinten Sie das eine oder das andere? Bitte sagen Sie es noch einmal."
            .to_string(),
        Lang::Tr => "Hangisini kastettiniz? Lütfen tekrar söyleyin.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_reply_three_languages() {
        let a = IntentAction::SetLight {
            target: "living room".into(),
            on: true,
        };
        assert_eq!(
            respond(&a, Lang::En, Answer::default()),
            "Turning on the living room light."
        );
        assert!(respond(&a, Lang::De, Answer::default()).contains("living room"));
        assert!(respond(&a, Lang::Tr, Answer::default()).contains("açıyorum"));
    }

    #[test]
    fn brightness_temp_cover_scene_replies() {
        assert!(respond(
            &IntentAction::SetBrightness {
                target: "bedroom".into(),
                percent: 40
            },
            Lang::En,
            Answer::default()
        )
        .contains("40 percent"));
        assert!(respond(
            &IntentAction::SetTemperature {
                target: "living room".into(),
                celsius: 21
            },
            Lang::De,
            Answer::default()
        )
        .contains("21 Grad"));
        assert!(respond(
            &IntentAction::SetCover {
                target: "blinds".into(),
                open: true
            },
            Lang::En,
            Answer::default()
        )
        .starts_with("Opening"));
        assert!(respond(
            &IntentAction::ActivateScene {
                name: "movie night".into()
            },
            Lang::Tr,
            Answer::default()
        )
        .contains("movie night"));
    }

    #[test]
    fn query_uses_answer_payload() {
        let a = IntentAction::QueryState {
            target: "bedroom".into(),
            what: QueryKind::Temperature,
        };
        assert_eq!(
            respond(
                &a,
                Lang::En,
                Answer {
                    temperature_c: Some(19),
                    ..Answer::default()
                }
            ),
            "The bedroom is 19 degrees."
        );
    }

    #[test]
    fn query_without_answer_does_not_invent() {
        let a = IntentAction::QueryState {
            target: "bedroom".into(),
            what: QueryKind::Temperature,
        };
        assert_eq!(
            respond(&a, Lang::En, Answer::default()),
            "I can't tell right now."
        );
    }

    #[test]
    fn on_state_query_reads_both_states() {
        let a = IntentAction::QueryState {
            target: "kitchen".into(),
            what: QueryKind::OnState,
        };
        assert!(respond(
            &a,
            Lang::En,
            Answer {
                is_on: Some(true),
                ..Answer::default()
            }
        )
        .contains("on"));
        assert!(respond(
            &a,
            Lang::En,
            Answer {
                is_on: Some(false),
                ..Answer::default()
            }
        )
        .contains("off"));
    }

    #[test]
    fn fallback_replies_exist_in_all_languages() {
        for lang in Lang::ALL {
            assert!(!not_understood(lang).is_empty());
            assert!(!please_clarify(lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-024: spoken replies must never leak engine terms.
        const BANNED: &[&str] = &[
            "intent", "slot", "NLU", "confidence", "MQTT", "entity_id", "entity",
            "Padatious", "Adapt", "token", "whisper", "piper", "wake word",
            "pod", "kubelet",
        ];
        let sample_actions = [
            IntentAction::SetLight {
                target: "living room".into(),
                on: true,
            },
            IntentAction::SetLight {
                target: "living room".into(),
                on: false,
            },
            IntentAction::SetBrightness {
                target: "bedroom".into(),
                percent: 50,
            },
            IntentAction::SetTemperature {
                target: "living room".into(),
                celsius: 21,
            },
            IntentAction::SetCover {
                target: "blinds".into(),
                open: true,
            },
            IntentAction::ActivateScene {
                name: "movie night".into(),
            },
            IntentAction::QueryState {
                target: "bedroom".into(),
                what: QueryKind::Temperature,
            },
            IntentAction::QueryState {
                target: "kitchen".into(),
                what: QueryKind::OnState,
            },
        ];
        let full_answer = Answer {
            temperature_c: Some(20),
            is_on: Some(true),
        };
        for lang in Lang::ALL {
            let mut texts: Vec<String> = sample_actions
                .iter()
                .map(|a| respond(a, lang, full_answer))
                .collect();
            texts.push(not_understood(lang));
            texts.push(please_clarify(lang));
            texts.push(unknown_reply(lang));
            for text in texts {
                let lower = text.to_lowercase();
                for banned in BANNED {
                    assert!(
                        !lower.contains(&banned.to_lowercase()),
                        "reply leaks jargon {banned:?}: {text}"
                    );
                }
            }
        }
    }
}
