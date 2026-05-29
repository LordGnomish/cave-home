//! Smart-detect zones and line-crossings, and the "did this event count?"
//! decision.
//!
//! A UniFi Protect camera view is carved into named smart-detect *zones* and
//! *line-crossings*. Each one arms a set of [`SmartDetectType`]s: the driveway
//! zone might arm people and vehicles, while a perimeter line arms only people.
//! A detection that falls in a zone the household did *not* arm for that type
//! is noise — it should not record, ring or notify. This module is the
//! arm-set bookkeeping and the membership decision, as pure logic.
//!
//! Phase 1 keys on the *type* arming (which is what the public Protect zone
//! config exposes per zone). The pixel-polygon geometry that decides whether a
//! detection's box physically sits inside the drawn zone is the camera pillar's
//! job (`cave-home-camera::geometry`, point-in-polygon) and is referenced — not
//! duplicated — here.
//!
//! Modelled from the public Protect smart-detect-zone configuration and the HA
//! `unifiprotect` integration (Apache-2.0). No GPL source was read.

use std::collections::BTreeSet;

use crate::detect::{DetectionEvent, SmartDetectType};

/// A named smart-detect zone.
///
/// A region of a camera's view that arms a set of detect-types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    name: String,
    armed: BTreeSet<u8>,
}

/// A named smart-detect line-crossing.
///
/// Like a [`Zone`] but triggered by a thing crossing a drawn line rather than
/// entering a region. It arms detect-types the same way; the geometry of "did
/// the track cross the line" is the camera pillar's job (deferred, see crate
/// docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCrossing {
    name: String,
    armed: BTreeSet<u8>,
}

// Stable index for each detect-type so it can live in a `BTreeSet<u8>` without
// pulling in `Hash`-set ordering nondeterminism. Internal only.
fn type_index(t: SmartDetectType) -> u8 {
    match t {
        SmartDetectType::Person => 0,
        SmartDetectType::Vehicle => 1,
        SmartDetectType::Package => 2,
        SmartDetectType::Animal => 3,
        SmartDetectType::LicensePlate => 4,
        SmartDetectType::FaceKnown => 5,
        SmartDetectType::Smoke => 6,
        SmartDetectType::CoAlarm => 7,
    }
}

macro_rules! arm_set {
    ($name:ident) => {
        impl $name {
            /// A zone with the given household-facing name, arming nothing yet.
            #[must_use]
            pub fn new(name: impl Into<String>) -> Self {
                Self {
                    name: name.into(),
                    armed: BTreeSet::new(),
                }
            }

            /// Arm this zone for a detect-type (builder; idempotent).
            #[must_use]
            pub fn arming(mut self, t: SmartDetectType) -> Self {
                self.armed.insert(type_index(t));
                self
            }

            /// The zone's household-facing name.
            #[must_use]
            pub fn name(&self) -> &str {
                &self.name
            }

            /// Whether this zone is armed for the given detect-type.
            #[must_use]
            pub fn is_armed_for(&self, t: SmartDetectType) -> bool {
                self.armed.contains(&type_index(t))
            }

            /// How many detect-types this zone is armed for.
            #[must_use]
            pub fn armed_count(&self) -> usize {
                self.armed.len()
            }

            /// Whether the given detection should count for this zone: it counts
            /// if the zone is armed for *any* of the detection's types. A
            /// life-safety alarm (smoke / CO) always counts — the household is
            /// never asked to "arm" a smoke alarm.
            #[must_use]
            pub fn arms(&self, event: &DetectionEvent) -> bool {
                if event.is_safety_alarm() {
                    return true;
                }
                event.types.iter().any(|t| self.is_armed_for(*t))
            }
        }
    };
}

arm_set!(Zone);
arm_set!(LineCrossing);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zone_arms_nothing() {
        let z = Zone::new("driveway");
        assert_eq!(z.name(), "driveway");
        assert_eq!(z.armed_count(), 0);
        for t in SmartDetectType::ALL {
            assert!(!z.is_armed_for(t));
        }
    }

    #[test]
    fn arming_is_idempotent() {
        let z = Zone::new("drive")
            .arming(SmartDetectType::Person)
            .arming(SmartDetectType::Person)
            .arming(SmartDetectType::Vehicle);
        assert_eq!(z.armed_count(), 2);
        assert!(z.is_armed_for(SmartDetectType::Person));
        assert!(z.is_armed_for(SmartDetectType::Vehicle));
        assert!(!z.is_armed_for(SmartDetectType::Animal));
    }

    #[test]
    fn arms_an_event_only_for_an_armed_type() {
        let z = Zone::new("drive").arming(SmartDetectType::Vehicle);
        let car = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Vehicle);
        let dog = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Animal);
        assert!(z.arms(&car));
        assert!(!z.arms(&dog));
    }

    #[test]
    fn arms_if_any_type_matches() {
        let z = Zone::new("drive").arming(SmartDetectType::Person);
        let mixed = DetectionEvent::new("c", 90, 0)
            .with_type(SmartDetectType::Animal)
            .with_type(SmartDetectType::Person);
        assert!(z.arms(&mixed));
    }

    #[test]
    fn safety_alarm_always_arms_even_unconfigured() {
        let z = Zone::new("kitchen"); // arms nothing
        let smoke = DetectionEvent::new("c", 95, 0).with_type(SmartDetectType::Smoke);
        assert!(z.arms(&smoke));
    }

    #[test]
    fn empty_zone_ignores_a_plain_detection() {
        let z = Zone::new("attic");
        let person = DetectionEvent::new("c", 95, 0).with_type(SmartDetectType::Person);
        assert!(!z.arms(&person));
    }

    #[test]
    fn line_crossing_arms_like_a_zone() {
        let line = LineCrossing::new("perimeter").arming(SmartDetectType::Person);
        let person = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        let car = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Vehicle);
        assert!(line.arms(&person));
        assert!(!line.arms(&car));
        assert_eq!(line.name(), "perimeter");
    }
}
