//! What a detector can recognise, and how the house talks about it.
//!
//! Two things live here. [`ObjectLabel`] is the small set of things the camera
//! pillar cares about — a person, a car, a delivery van, a pet. [`Lang`] plus
//! the phrasing helpers turn "a person detected in the zone named `front_door`"
//! into the plain line a household reads: "Person at the front door" / "Person
//! an der Haustür" / "Ön kapıda bir kişi" (Charter §6.3, ADR-007).
//!
//! Nothing here mentions a model class index, a tensor, a transport or a device
//! model — that is the whole point of the layer.

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// The things the camera pillar can recognise and reason about. This is a
/// deliberately small, household-meaningful set — not the full COCO class list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectLabel {
    /// A person.
    Person,
    /// A car or similar passenger vehicle.
    Car,
    /// A delivery van / truck.
    DeliveryVan,
    /// A bicycle.
    Bicycle,
    /// A dog.
    Dog,
    /// A cat.
    Cat,
    /// A package left in view.
    Package,
}

impl ObjectLabel {
    /// All recognised labels, for iteration in tests and config UIs.
    pub const ALL: [Self; 7] = [
        Self::Person,
        Self::Car,
        Self::DeliveryVan,
        Self::Bicycle,
        Self::Dog,
        Self::Cat,
        Self::Package,
    ];

    /// The plain noun for this thing, localised. Lower-case so it can be
    /// embedded mid-sentence; [`ObjectLabel::headline`] capitalises it.
    #[must_use]
    pub const fn noun(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Person, Lang::En) => "person",
            (Self::Person, Lang::De) => "Person",
            (Self::Person, Lang::Tr) => "kişi",
            (Self::Car, Lang::En) => "car",
            (Self::Car, Lang::De) => "Auto",
            (Self::Car, Lang::Tr) => "araba",
            (Self::DeliveryVan, Lang::En) => "delivery van",
            (Self::DeliveryVan, Lang::De) => "Lieferwagen",
            (Self::DeliveryVan, Lang::Tr) => "teslimat aracı",
            (Self::Bicycle, Lang::En) => "bicycle",
            (Self::Bicycle, Lang::De) => "Fahrrad",
            (Self::Bicycle, Lang::Tr) => "bisiklet",
            (Self::Dog, Lang::En) => "dog",
            (Self::Dog, Lang::De) => "Hund",
            (Self::Dog, Lang::Tr) => "köpek",
            (Self::Cat, Lang::En) => "cat",
            (Self::Cat, Lang::De) => "Katze",
            (Self::Cat, Lang::Tr) => "kedi",
            (Self::Package, Lang::En) => "package",
            (Self::Package, Lang::De) => "Paket",
            (Self::Package, Lang::Tr) => "paket",
        }
    }
}

/// A grandma-friendly "thing at the place" line for a notification or tile.
///
/// For example "Person at the front door", "Auto in der Einfahrt", "Araba garaj
/// yolunda". `place` is the zone's already-localised friendly name (see
/// [`crate::zone::Zone::friendly_name`]); this only joins the two with the right
/// connective per language.
#[must_use]
pub fn seen_at(label: ObjectLabel, place: &str, lang: Lang) -> String {
    let thing = label.noun(lang);
    match lang {
        // Capitalise the first letter of the English noun for a headline.
        Lang::En => format!("{} at the {place}", capitalise(thing)),
        Lang::De => format!("{thing} an der {place}"),
        Lang::Tr => format!("{place} bölgesinde bir {thing}"),
    }
}

/// The reassuring "all clear" line shown when nothing of interest is in view.
#[must_use]
pub const fn nothing_unusual(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Nothing unusual",
        Lang::De => "Nichts Ungewöhnliches",
        Lang::Tr => "Olağandışı bir şey yok",
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
    fn every_label_has_a_noun_in_every_language() {
        for label in ObjectLabel::ALL {
            for lang in LANGS {
                assert!(
                    !label.noun(lang).is_empty(),
                    "{label:?}/{lang:?} has no noun"
                );
            }
        }
    }

    #[test]
    fn seen_at_reads_naturally_in_english() {
        assert_eq!(
            seen_at(ObjectLabel::Person, "front door", Lang::En),
            "Person at the front door"
        );
        assert_eq!(
            seen_at(ObjectLabel::Car, "driveway", Lang::En),
            "Car at the driveway"
        );
    }

    #[test]
    fn seen_at_embeds_the_place_in_every_language() {
        for lang in LANGS {
            let line = seen_at(ObjectLabel::Person, "Einfahrt", lang);
            assert!(line.contains("Einfahrt"), "{lang:?}: {line}");
        }
    }

    #[test]
    fn nothing_unusual_present_in_every_language() {
        for lang in LANGS {
            assert!(!nothing_unusual(lang).is_empty());
        }
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-007: a camera notification must never surface a
        // codec, transport, model or device-model term.
        const BANNED: &[&str] = &[
            "RTSP", "ONVIF", "H.264", "H264", "NAL", "tensor", "inference",
            "YOLO", "ONNX", "Coral", "TensorRT", "GPU", "MQTT", "entity_id",
            "entity id", "bounding box", "bbox", "IoU", "Frigate", "Protect",
            "ffmpeg", "codec", "pod", "namespace", "webhook",
        ];
        let mut texts: Vec<String> = Vec::new();
        for lang in LANGS {
            texts.push(nothing_unusual(lang).to_owned());
            for label in ObjectLabel::ALL {
                texts.push(label.noun(lang).to_owned());
                texts.push(seen_at(label, "front door", lang));
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
