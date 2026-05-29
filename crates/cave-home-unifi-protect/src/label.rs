//! Grandma-friendly phrasing for Protect events (Charter §6.3, ADR-007).
//!
//! Everything the household reads about a camera comes through here, in EN / DE
//! / TR (the Charter §6.3 mandatory languages from M1). A smart detection
//! becomes "Person at the driveway camera"; a doorbell press becomes "Doorbell
//! rang at the front door"; a package becomes "Package detected". Nothing here
//! names a WebSocket packet, a smartDetectType wire string, an NVR, an RTSP
//! stream, or a device model — that is the whole point of the layer.

use crate::detect::SmartDetectType;

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

impl SmartDetectType {
    /// The plain noun for this detection, localised and lower-case so it can be
    /// embedded mid-sentence. [`detection_line`] capitalises it for a headline.
    #[must_use]
    pub const fn noun(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Person, Lang::En) => "person",
            (Self::Person, Lang::De) => "Person",
            (Self::Person, Lang::Tr) => "kişi",
            (Self::Vehicle, Lang::En) => "vehicle",
            (Self::Vehicle, Lang::De) => "Fahrzeug",
            (Self::Vehicle, Lang::Tr) => "araç",
            (Self::Package, Lang::En) => "package",
            (Self::Package, Lang::De) => "Paket",
            (Self::Package, Lang::Tr) => "paket",
            (Self::Animal, Lang::En) => "animal",
            (Self::Animal, Lang::De) => "Tier",
            (Self::Animal, Lang::Tr) => "hayvan",
            (Self::LicensePlate, Lang::En) => "number plate",
            (Self::LicensePlate, Lang::De) => "Kennzeichen",
            (Self::LicensePlate, Lang::Tr) => "plaka",
            (Self::FaceKnown, Lang::En) => "familiar face",
            (Self::FaceKnown, Lang::De) => "bekanntes Gesicht",
            (Self::FaceKnown, Lang::Tr) => "tanıdık yüz",
            (Self::Smoke, Lang::En) => "smoke alarm",
            (Self::Smoke, Lang::De) => "Rauchmelder",
            (Self::Smoke, Lang::Tr) => "duman alarmı",
            (Self::CoAlarm, Lang::En) => "carbon-monoxide alarm",
            (Self::CoAlarm, Lang::De) => "Kohlenmonoxid-Alarm",
            (Self::CoAlarm, Lang::Tr) => "karbonmonoksit alarmı",
        }
    }
}

/// A "thing at the place" line for a notification or tile.
///
/// For example "Person at the driveway camera", "Person an der Einfahrt-Kamera",
/// "Garaj yolu kamerasında bir kişi". `place` is the camera's already-localised
/// friendly name; this only joins the two with the right connective per
/// language.
#[must_use]
pub fn detection_line(detect: SmartDetectType, place: &str, lang: Lang) -> String {
    let thing = detect.noun(lang);
    match lang {
        Lang::En => format!("{} at the {place}", capitalise(thing)),
        Lang::De => format!("{thing} an der {place}"),
        Lang::Tr => format!("{place} bölgesinde bir {thing}"),
    }
}

/// The short "X detected" headline, with no place (e.g. a package on the step).
///
/// "Package detected", "Paket erkannt", "Paket algılandı".
#[must_use]
pub fn detected_headline(detect: SmartDetectType, lang: Lang) -> String {
    let thing = detect.noun(lang);
    match lang {
        Lang::En => format!("{} detected", capitalise(thing)),
        Lang::De => format!("{} erkannt", capitalise(thing)),
        Lang::Tr => format!("{} algılandı", capitalise(thing)),
    }
}

/// The doorbell-ring line.
///
/// "Doorbell rang at the front door", "Es hat an der Haustür geklingelt", "Ön
/// kapıda zil çaldı". `door` is the doorbell's already-localised friendly name.
#[must_use]
pub fn ring_line(door: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("Doorbell rang at the {door}"),
        Lang::De => format!("Es hat an der {door} geklingelt"),
        Lang::Tr => format!("{door} kapısında zil çaldı"),
    }
}

/// The reassuring "all quiet" line shown when nothing of interest is happening.
#[must_use]
pub const fn all_quiet(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "All quiet",
        Lang::De => "Alles ruhig",
        Lang::Tr => "Her şey sakin",
    }
}

/// Upper-case the first character of an ASCII word, leaving the rest untouched.
fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [Lang; 3] = [Lang::En, Lang::De, Lang::Tr];

    #[test]
    fn every_type_has_a_noun_in_every_language() {
        for t in SmartDetectType::ALL {
            for lang in LANGS {
                assert!(!t.noun(lang).is_empty(), "{t:?}/{lang:?}");
            }
        }
    }

    #[test]
    fn detection_line_reads_naturally_in_english() {
        assert_eq!(
            detection_line(SmartDetectType::Person, "driveway camera", Lang::En),
            "Person at the driveway camera"
        );
        assert_eq!(
            detection_line(SmartDetectType::Vehicle, "garage camera", Lang::En),
            "Vehicle at the garage camera"
        );
    }

    #[test]
    fn detection_line_embeds_the_place_in_every_language() {
        for lang in LANGS {
            let line = detection_line(SmartDetectType::Person, "Einfahrt", lang);
            assert!(line.contains("Einfahrt"), "{lang:?}: {line}");
        }
    }

    #[test]
    fn package_headline_is_grandma_plain() {
        assert_eq!(
            detected_headline(SmartDetectType::Package, Lang::En),
            "Package detected"
        );
        for lang in LANGS {
            assert!(!detected_headline(SmartDetectType::Package, lang).is_empty());
        }
    }

    #[test]
    fn ring_line_reads_naturally() {
        assert_eq!(
            ring_line("front door", Lang::En),
            "Doorbell rang at the front door"
        );
        for lang in LANGS {
            let line = ring_line("Haustür", lang);
            assert!(line.contains("Haustür"), "{lang:?}: {line}");
        }
    }

    #[test]
    fn all_quiet_present_in_every_language() {
        for lang in LANGS {
            assert!(!all_quiet(lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-007: a Protect notification must never surface a
        // transport, wire field, device model or NVR term.
        const BANNED: &[&str] = &[
            "WebSocket", "WS ", "bootstrap", "RTSP", "RTSPS", "smartDetect",
            "smartDetectType", "EventType", "NVR", "Ubiquiti", "UniFi", "G4",
            "G5", "Protect", "REST", "API", "entity_id", "entity id", "MQTT",
            "thumbnail_id", "codec", "H.264", "packet", "pod", "namespace",
        ];
        let mut texts: Vec<String> = Vec::new();
        for lang in LANGS {
            texts.push(all_quiet(lang).to_owned());
            texts.push(ring_line("front door", lang));
            for t in SmartDetectType::ALL {
                texts.push(t.noun(lang).to_owned());
                texts.push(detection_line(t, "driveway camera", lang));
                texts.push(detected_headline(t, lang));
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
