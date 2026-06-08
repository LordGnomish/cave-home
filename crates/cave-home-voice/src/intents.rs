//! Built-in intents — the commands cave-home understands out of the box, in
//! EN / DE / TR (Charter §6.3 multilingual mandate).
//!
//! Each intent has a stable id (`light.on`, `climate.set`, …) and one or more
//! *sentence templates* per language describing how a household phrases it. A
//! shared slot vocabulary (`name`, `level`) is supplied so every variant
//! resolves room names and numbers the same way. [`builtin_intents`] compiles
//! the whole set into [`CompiledIntent`]s ready for [`crate::matcher`].
//!
//! The room/cover/scene vocabulary here is a worked example for tests and the
//! demo; in production the household's real device list (from cave-home-core,
//! Phase-1b) is injected instead — the engine itself is vocabulary-agnostic.

use crate::label::Lang;
use crate::matcher::CompiledIntent;
use crate::slot::{SlotKind, ValueList};
use crate::template::TemplateError;
use std::collections::BTreeMap;

/// Build the example room vocabulary (canonical names + a few synonyms).
#[must_use]
pub fn example_rooms() -> ValueList {
    ValueList::new(["living room", "kitchen", "bedroom", "bathroom"])
        .with_synonym("lounge", "living room")
        .with_synonym("sitting room", "living room")
        .with_synonym("wohnzimmer", "living room")
        .with_synonym("küche", "kitchen")
        .with_synonym("schlafzimmer", "bedroom")
        .with_synonym("oturma odası", "living room")
        .with_synonym("mutfak", "kitchen")
        .with_synonym("yatak odası", "bedroom")
}

/// Example cover vocabulary.
#[must_use]
pub fn example_covers() -> ValueList {
    ValueList::new(["blinds", "curtains", "garage door"])
        .with_synonym("shades", "blinds")
        .with_synonym("rollladen", "blinds")
        .with_synonym("perde", "curtains")
}

/// Example scene vocabulary.
#[must_use]
pub fn example_scenes() -> ValueList {
    ValueList::new(["movie night", "good morning", "bedtime"])
        .with_synonym("film akşamı", "movie night")
        .with_synonym("filmabend", "movie night")
}

/// Slot kinds keyed by slot name, shared across all built-in templates.
fn slot_defs(name_list: ValueList, scene_list: ValueList, cover_list: ValueList) -> SlotTable {
    SlotTable {
        rooms: name_list,
        scenes: scene_list,
        covers: cover_list,
    }
}

struct SlotTable {
    rooms: ValueList,
    scenes: ValueList,
    covers: ValueList,
}

impl SlotTable {
    fn name(&self, list: &ValueList) -> BTreeMap<String, SlotKind> {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), SlotKind::List(list.clone()));
        m
    }

    fn name_and_level(&self, max: u32) -> BTreeMap<String, SlotKind> {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), SlotKind::List(self.rooms.clone()));
        m.insert("level".to_string(), SlotKind::Number { min: 0, max });
        m
    }
}

/// One built-in intent's sentence templates, grouped by language.
struct IntentSpec {
    id: &'static str,
    /// (lang, template-src) pairs.
    sentences: &'static [(Lang, &'static str)],
    /// How to build this intent's slot table.
    slots: SlotSelector,
}

#[derive(Clone, Copy)]
enum SlotSelector {
    Room,
    RoomAndPercent,
    RoomAndDegrees,
    Cover,
    Scene,
}

/// The built-in intent specifications. EN + DE + TR sentence sets per intent.
const SPECS: &[IntentSpec] = &[
    IntentSpec {
        id: "light.on",
        slots: SlotSelector::Room,
        sentences: &[
            (Lang::En, "turn [the] {name} [light] on"),
            (Lang::En, "switch [the] {name} [light] on"),
            (Lang::De, "schalte [das] [licht] [im] {name} ein"),
            (Lang::Tr, "{name} ışığını aç"),
        ],
    },
    IntentSpec {
        id: "light.off",
        slots: SlotSelector::Room,
        sentences: &[
            (Lang::En, "turn [the] {name} [light] off"),
            (Lang::En, "switch [the] {name} [light] off"),
            (Lang::De, "schalte [das] [licht] [im] {name} aus"),
            (Lang::Tr, "{name} ışığını kapat"),
        ],
    },
    IntentSpec {
        id: "light.brightness",
        slots: SlotSelector::RoomAndPercent,
        sentences: &[
            (Lang::En, "set [the] {name} [light] to {level} percent"),
            (Lang::En, "dim [the] {name} [light] to {level} percent"),
            (Lang::De, "stelle [das] [licht] [im] {name} auf {level} prozent"),
            (Lang::Tr, "{name} ışığını yüzde {level} yap"),
        ],
    },
    IntentSpec {
        id: "climate.set",
        slots: SlotSelector::RoomAndDegrees,
        sentences: &[
            (Lang::En, "set [the] {name} to {level} degrees"),
            (Lang::En, "make [the] {name} {level} degrees"),
            (Lang::De, "stelle [das] {name} auf {level} grad"),
            (Lang::Tr, "{name} sıcaklığını {level} dereceye ayarla"),
        ],
    },
    IntentSpec {
        id: "cover.open",
        slots: SlotSelector::Cover,
        sentences: &[
            (Lang::En, "open [the] {name}"),
            (Lang::De, "öffne [die] {name}"),
            (Lang::Tr, "{name} aç"),
        ],
    },
    IntentSpec {
        id: "cover.close",
        slots: SlotSelector::Cover,
        sentences: &[
            (Lang::En, "close [the] {name}"),
            (Lang::De, "schließe [die] {name}"),
            (Lang::Tr, "{name} kapat"),
        ],
    },
    IntentSpec {
        id: "scene.activate",
        slots: SlotSelector::Scene,
        sentences: &[
            (Lang::En, "[(start|activate)] [the] {name} [scene]"),
            (Lang::En, "set the scene to {name}"),
            (Lang::De, "starte [die] [szene] {name}"),
            (Lang::Tr, "{name} sahnesini başlat"),
        ],
    },
    IntentSpec {
        id: "query.temperature",
        slots: SlotSelector::Room,
        sentences: &[
            (Lang::En, "(what is|whats|what's) the {name} temperature"),
            (Lang::En, "how warm is [the] {name}"),
            (Lang::De, "wie warm ist [es] [im] {name}"),
            (Lang::Tr, "{name} kaç derece"),
        ],
    },
    IntentSpec {
        id: "query.on_state",
        slots: SlotSelector::Room,
        sentences: &[
            (Lang::En, "is [the] {name} [light] on"),
            (Lang::De, "ist [das] [licht] [im] {name} an"),
            (Lang::Tr, "{name} ışığı açık mı"),
        ],
    },
];

/// Compile the full built-in intent set using the example vocabularies.
///
/// # Errors
///
/// Returns a [`TemplateError`] if any built-in template is malformed (a
/// programming error caught by the test suite, not at runtime).
pub fn builtin_intents() -> Result<Vec<CompiledIntent>, TemplateError> {
    builtin_intents_with(example_rooms(), example_scenes(), example_covers())
}

/// Compile the built-in intent set against caller-supplied vocabularies (the
/// household's real device list in production).
///
/// # Errors
///
/// Returns a [`TemplateError`] if any built-in template is malformed.
pub fn builtin_intents_with(
    rooms: ValueList,
    scenes: ValueList,
    covers: ValueList,
) -> Result<Vec<CompiledIntent>, TemplateError> {
    let table = slot_defs(rooms, scenes, covers);
    let mut out = Vec::new();
    for spec in SPECS {
        for (lang, src) in spec.sentences {
            let slots = match spec.slots {
                SlotSelector::Room => table.name(&table.rooms),
                SlotSelector::RoomAndPercent => table.name_and_level(100),
                SlotSelector::RoomAndDegrees => table.name_and_level(35),
                SlotSelector::Cover => table.name(&table.covers),
                SlotSelector::Scene => table.name(&table.scenes),
            };
            out.push(CompiledIntent::new(spec.id, *lang, src, slots)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{match_intent, MatchOutcome};
    use crate::slot::SlotValue;

    fn matched(utt: &str) -> (String, BTreeMap<String, SlotValue>) {
        let intents = builtin_intents().expect("built-ins compile");
        match match_intent(utt, &intents) {
            MatchOutcome::Matched(m) => (m.intent, m.slots),
            other => panic!("{utt:?} -> {other:?}"),
        }
    }

    #[test]
    fn all_builtins_compile() {
        let intents = builtin_intents().expect("compile");
        assert!(intents.len() >= 25, "expected a full set, got {}", intents.len());
    }

    #[test]
    fn english_light_on_off() {
        let (id, slots) = matched("turn the kitchen light on");
        assert_eq!(id, "light.on");
        assert_eq!(slots["name"], SlotValue::Text("kitchen".into()));
        let (id, _) = matched("switch the bedroom off");
        assert_eq!(id, "light.off");
    }

    #[test]
    fn german_light_command() {
        let (id, slots) = matched("schalte das licht im wohnzimmer ein");
        assert_eq!(id, "light.on");
        assert_eq!(slots["name"], SlotValue::Text("living room".into()));
    }

    #[test]
    fn turkish_light_command() {
        let (id, slots) = matched("mutfak ışığını aç");
        assert_eq!(id, "light.on");
        assert_eq!(slots["name"], SlotValue::Text("kitchen".into()));
    }

    #[test]
    fn english_brightness_with_number_word() {
        let (id, slots) = matched("dim the bedroom light to fifty percent");
        assert_eq!(id, "light.brightness");
        assert_eq!(slots["level"], SlotValue::Number(50));
    }

    #[test]
    fn turkish_brightness_with_number_word() {
        let (id, slots) = matched("yatak odası ışığını yüzde elli yap");
        assert_eq!(id, "light.brightness");
        assert_eq!(slots["level"], SlotValue::Number(50));
        assert_eq!(slots["name"], SlotValue::Text("bedroom".into()));
    }

    #[test]
    fn climate_set_clamps_to_household_range() {
        let (id, slots) = matched("set the living room to 21 degrees");
        assert_eq!(id, "climate.set");
        assert_eq!(slots["level"], SlotValue::Number(21));
        // 50 degrees is outside the household range (max 35) -> no match.
        let intents = builtin_intents().expect("c");
        assert_eq!(
            match_intent("set the living room to 50 degrees", &intents),
            MatchOutcome::NoMatch
        );
    }

    #[test]
    fn cover_open_close_three_languages() {
        assert_eq!(matched("open the blinds").0, "cover.open");
        assert_eq!(matched("öffne die rollladen").0, "cover.open");
        assert_eq!(matched("perde kapat").0, "cover.close");
    }

    #[test]
    fn scene_activation() {
        assert_eq!(matched("start the movie night scene").0, "scene.activate");
        assert_eq!(matched("filmabend").1.len(), 1); // captured the name slot
    }

    #[test]
    fn query_temperature_english_and_german() {
        let (id, slots) = matched("what's the bedroom temperature");
        assert_eq!(id, "query.temperature");
        assert_eq!(slots["name"], SlotValue::Text("bedroom".into()));
        assert_eq!(matched("wie warm ist es im wohnzimmer").0, "query.temperature");
    }

    #[test]
    fn query_on_state() {
        assert_eq!(matched("is the kitchen light on").0, "query.on_state");
        assert_eq!(matched("mutfak ışığı açık mı").0, "query.on_state");
    }

    #[test]
    fn unknown_room_rejected_across_set() {
        let intents = builtin_intents().expect("c");
        assert_eq!(
            match_intent("turn the garage light on", &intents),
            MatchOutcome::NoMatch
        );
    }

    #[test]
    fn gibberish_is_no_match() {
        let intents = builtin_intents().expect("c");
        assert_eq!(
            match_intent("flibber the wobbledygook", &intents),
            MatchOutcome::NoMatch
        );
    }
}
