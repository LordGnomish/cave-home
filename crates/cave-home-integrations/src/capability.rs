//! What a thing can *do* — the entity-platform set an integration provides.
//!
//! In Home Assistant terms these are the entity platforms an integration
//! forwards to (light, cover, climate…). cave-home keeps the same vocabulary
//! internally, but the household only ever sees the grandma-friendly label
//! (see [`crate::label`]); the [`Capability`] enum itself is the developer-side
//! name.

use crate::label::Lang;

/// A kind of thing an integration can control or sense — one entity platform.
///
/// Ordered & exhaustive so the registry can aggregate "what can this hub do"
/// deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// A light that can be switched (and possibly dimmed / coloured).
    Light,
    /// A switchable plug or relay.
    Switch,
    /// A blind, shade, curtain or garage door.
    Cover,
    /// Heating / cooling / a thermostat.
    Climate,
    /// A read-only measurement (temperature, humidity, air quality…).
    Sensor,
    /// A door / window / motion contact.
    BinarySensor,
    /// A camera feed.
    Camera,
    /// A door lock.
    Lock,
    /// A robot vacuum.
    Vacuum,
    /// A media player / speaker.
    MediaPlayer,
    /// A doorbell / chime button.
    Button,
}

impl Capability {
    /// Every capability, best-effort stable order. Useful for aggregation and
    /// exhaustiveness tests.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Light,
            Self::Switch,
            Self::Cover,
            Self::Climate,
            Self::Sensor,
            Self::BinarySensor,
            Self::Camera,
            Self::Lock,
            Self::Vacuum,
            Self::MediaPlayer,
            Self::Button,
        ]
    }

    /// The grandma-friendly singular noun for this capability, localised.
    ///
    /// This is the *only* word that reaches a household for a capability —
    /// never "platform", never "entity domain".
    #[must_use]
    pub const fn noun(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Light, Lang::En) => "light",
            (Self::Light, Lang::De) => "Lampe",
            (Self::Light, Lang::Tr) => "ışık",
            (Self::Switch, Lang::En) => "switch",
            (Self::Switch, Lang::De) => "Schalter",
            (Self::Switch, Lang::Tr) => "anahtar",
            (Self::Cover, Lang::En) => "blind",
            (Self::Cover, Lang::De) => "Rollladen",
            (Self::Cover, Lang::Tr) => "panjur",
            (Self::Climate, Lang::En) => "thermostat",
            (Self::Climate, Lang::De) => "Thermostat",
            (Self::Climate, Lang::Tr) => "termostat",
            (Self::Sensor, Lang::En) => "sensor",
            (Self::Sensor, Lang::De) => "Sensor",
            (Self::Sensor, Lang::Tr) => "sensör",
            (Self::BinarySensor, Lang::En) => "contact sensor",
            (Self::BinarySensor, Lang::De) => "Kontaktsensor",
            (Self::BinarySensor, Lang::Tr) => "kontak sensörü",
            (Self::Camera, Lang::En) => "camera",
            (Self::Camera, Lang::De) => "Kamera",
            (Self::Camera, Lang::Tr) => "kamera",
            (Self::Lock, Lang::En) => "lock",
            (Self::Lock, Lang::De) => "Schloss",
            (Self::Lock, Lang::Tr) => "kilit",
            (Self::Vacuum, Lang::En) => "vacuum",
            (Self::Vacuum, Lang::De) => "Staubsauger",
            (Self::Vacuum, Lang::Tr) => "süpürge",
            (Self::MediaPlayer, Lang::En) => "speaker",
            (Self::MediaPlayer, Lang::De) => "Lautsprecher",
            (Self::MediaPlayer, Lang::Tr) => "hoparlör",
            (Self::Button, Lang::En) => "button",
            (Self::Button, Lang::De) => "Taste",
            (Self::Button, Lang::Tr) => "düğme",
        }
    }
}

/// The aggregate of everything a set of loaded integrations can do — "what can
/// this hub do" for the Portal's overview.
///
/// Capabilities are de-duplicated and kept in [`Capability`] order so two hubs
/// with the same loaded set report the same thing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubCapabilities {
    present: Vec<Capability>,
}

impl HubCapabilities {
    /// An empty hub — nothing added yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { present: Vec::new() }
    }

    /// Add one capability, keeping the set sorted & de-duplicated.
    pub fn add(&mut self, cap: Capability) {
        if let Err(idx) = self.present.binary_search(&cap) {
            self.present.insert(idx, cap);
        }
    }

    /// Whether the hub can do this.
    #[must_use]
    pub fn has(&self, cap: Capability) -> bool {
        self.present.binary_search(&cap).is_ok()
    }

    /// How many distinct capabilities the hub has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.present.len()
    }

    /// Whether the hub can do nothing yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.present.is_empty()
    }

    /// The capabilities, in stable order.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        &self.present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_has_three_language_nouns() {
        for cap in Capability::all() {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                assert!(!cap.noun(lang).is_empty(), "{cap:?} missing noun for {lang:?}");
            }
        }
    }

    #[test]
    fn hub_capabilities_dedupe_and_sort() {
        let mut hub = HubCapabilities::new();
        assert!(hub.is_empty());
        hub.add(Capability::Sensor);
        hub.add(Capability::Light);
        hub.add(Capability::Light); // duplicate
        hub.add(Capability::Camera);
        assert_eq!(hub.len(), 3);
        // sorted in Capability order: Light < Sensor < Camera
        assert_eq!(
            hub.capabilities(),
            &[Capability::Light, Capability::Sensor, Capability::Camera]
        );
    }

    #[test]
    fn hub_has_reports_membership() {
        let mut hub = HubCapabilities::new();
        hub.add(Capability::Lock);
        assert!(hub.has(Capability::Lock));
        assert!(!hub.has(Capability::Vacuum));
    }

    #[test]
    fn capability_nouns_carry_no_jargon() {
        const BANNED: &[&str] = &["platform", "entity", "domain", "config"];
        for cap in Capability::all() {
            for lang in [Lang::En, Lang::De, Lang::Tr] {
                let n = cap.noun(lang).to_lowercase();
                for b in BANNED {
                    assert!(!n.contains(b), "{cap:?} noun leaks {b:?}: {n}");
                }
            }
        }
    }
}
