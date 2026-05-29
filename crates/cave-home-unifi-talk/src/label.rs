//! Grandma-friendly call notifications (Charter §6.3, ADR-007).
//!
//! The household never sees "SIP INVITE", "RTP", a codec name or an entity id —
//! they see "Someone is calling" / "Es ruft jemand an" / "Biri arıyor",
//! localised to the Charter §6.3 mandatory languages EN / DE / TR. This module
//! turns a call state (and a caller name / device kind) into that plain line.

use crate::call::CallState;
use crate::device::DeviceKind;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// The plain headline for a call state, without a caller name.
/// `Idle` and `Connecting` have no standalone notification and return "".
#[must_use]
pub fn for_state(state: CallState, lang: Lang) -> &'static str {
    match (state, lang) {
        (CallState::Idle | CallState::Connecting, _) => "",
        (CallState::Ringing, Lang::En) => "Someone is calling",
        (CallState::Ringing, Lang::De) => "Es ruft jemand an",
        (CallState::Ringing, Lang::Tr) => "Biri arıyor",
        (CallState::Active, Lang::En) => "On a call",
        (CallState::Active, Lang::De) => "Im Gespräch",
        (CallState::Active, Lang::Tr) => "Görüşme sürüyor",
        (CallState::Held, Lang::En) => "Call on hold",
        (CallState::Held, Lang::De) => "Gespräch gehalten",
        (CallState::Held, Lang::Tr) => "Görüşme beklemede",
        (CallState::Ended, Lang::En) => "Call ended",
        (CallState::Ended, Lang::De) => "Gespräch beendet",
        (CallState::Ended, Lang::Tr) => "Görüşme sona erdi",
        (CallState::Missed, Lang::En) => "Missed call",
        (CallState::Missed, Lang::De) => "Verpasster Anruf",
        (CallState::Missed, Lang::Tr) => "Cevapsız çağrı",
        (CallState::Voicemail, Lang::En) => "Caller left a voicemail",
        (CallState::Voicemail, Lang::De) => "Anrufer hat eine Nachricht hinterlassen",
        (CallState::Voicemail, Lang::Tr) => "Arayan sesli mesaj bıraktı",
    }
}

/// "The front-door intercom is calling" — an incoming-call line naming the
/// device that is ringing.
#[must_use]
pub fn incoming_from_device(kind: DeviceKind, lang: Lang) -> String {
    match (kind, lang) {
        (DeviceKind::Doorbell, Lang::En) => "The front-door intercom is calling".to_owned(),
        (DeviceKind::Doorbell, Lang::De) => "Die Türsprechanlage ruft an".to_owned(),
        (DeviceKind::Doorbell, Lang::Tr) => "Kapı interkomu arıyor".to_owned(),
        (DeviceKind::Intercom, Lang::En) => "The intercom is calling".to_owned(),
        (DeviceKind::Intercom, Lang::De) => "Die Sprechanlage ruft an".to_owned(),
        (DeviceKind::Intercom, Lang::Tr) => "İnterkom arıyor".to_owned(),
        (DeviceKind::DeskPhone, Lang::En) => "The phone is ringing".to_owned(),
        (DeviceKind::DeskPhone, Lang::De) => "Das Telefon klingelt".to_owned(),
        (DeviceKind::DeskPhone, Lang::Tr) => "Telefon çalıyor".to_owned(),
    }
}

/// "Missed call from the gate" — a missed-call line naming the caller.
#[must_use]
pub fn missed_from(caller: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Missed call from {caller}"),
        Lang::De => format!("Verpasster Anruf von {caller}"),
        Lang::Tr => format!("{caller} kişisinden cevapsız çağrı"),
    }
}

/// The "do not disturb is on" status line.
#[must_use]
pub fn do_not_disturb_on(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Do not disturb is on",
        Lang::De => "Bitte nicht stören ist aktiv",
        Lang::Tr => "Rahatsız etmeyin açık",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];
    const KINDS: [DeviceKind; 3] =
        [DeviceKind::Doorbell, DeviceKind::Intercom, DeviceKind::DeskPhone];
    const SETTLED: [CallState; 5] = [
        CallState::Ringing,
        CallState::Active,
        CallState::Held,
        CallState::Ended,
        CallState::Missed,
    ];

    #[test]
    fn every_meaningful_state_has_a_line_in_every_language() {
        let states = [
            CallState::Ringing,
            CallState::Active,
            CallState::Held,
            CallState::Ended,
            CallState::Missed,
            CallState::Voicemail,
        ];
        for state in states {
            for lang in LANGS {
                assert!(!for_state(state, lang).is_empty(), "{state:?}/{lang:?} empty");
            }
        }
    }

    #[test]
    fn idle_and_connecting_have_no_notification() {
        for lang in LANGS {
            assert_eq!(for_state(CallState::Idle, lang), "");
            assert_eq!(for_state(CallState::Connecting, lang), "");
        }
    }

    #[test]
    fn device_lines_differ_by_kind() {
        // A doorbell, a wall intercom and a desk phone read differently.
        for lang in LANGS {
            let door = incoming_from_device(DeviceKind::Doorbell, lang);
            let panel = incoming_from_device(DeviceKind::Intercom, lang);
            let phone = incoming_from_device(DeviceKind::DeskPhone, lang);
            assert_ne!(door, panel);
            assert_ne!(panel, phone);
            assert_ne!(door, phone);
        }
    }

    #[test]
    fn missed_from_embeds_the_caller_in_each_language() {
        assert_eq!(missed_from("the gate", Lang::En), "Missed call from the gate");
        assert!(missed_from("the gate", Lang::De).contains("the gate"));
        assert!(missed_from("the gate", Lang::Tr).contains("the gate"));
    }

    #[test]
    fn do_not_disturb_line_present_in_each_language() {
        for lang in LANGS {
            assert!(!do_not_disturb_on(lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-009: the call UI must never surface protocol,
        // transport, codec or controller terms.
        const BANNED: &[&str] = &[
            "SIP", "INVITE", "RTP", "RTCP", "VoIP", "codec", "G.711", "G.722",
            "WebRTC", "WebSocket", "REST", "MQTT", "entity_id", "entity id",
            "PSTN", "trunk", "Ubiquiti", "UniFi", "controller", "PBX",
            "pod", "kubelet", "namespace",
        ];
        let mut texts: Vec<String> = Vec::new();
        for lang in LANGS {
            texts.push(do_not_disturb_on(lang).to_owned());
            texts.push(missed_from("the gate", lang));
            for kind in KINDS {
                texts.push(incoming_from_device(kind, lang));
            }
            for state in SETTLED {
                texts.push(for_state(state, lang).to_owned());
            }
            texts.push(for_state(CallState::Voicemail, lang).to_owned());
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
