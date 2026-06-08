//! The doorbell ring event and rapid-detection de-dupe / grouping.
//!
//! Two things live here. A [`RingEvent`] is the doorbell-press event — distinct
//! from a smart detection because a household reacts to it differently ("someone
//! is at the door" rather than "a person was seen"). And the [`EventGrouper`]
//! collapses a burst of identical detections — a UniFi Protect camera will
//! happily raise the same "person" detection many times a second while the
//! person is in view — into one logical event, so the household gets one
//! notification, not forty.
//!
//! De-dupe is pure logic over caller-supplied ticks and a caller-supplied
//! cooldown; the crate reads no clock.

use std::collections::HashMap;

use crate::detect::{DetectionEvent, SmartDetectType, Tick};
use crate::device::CameraId;

/// A doorbell button press.
///
/// `at` is the caller tick of the press. A ring is instantaneous, so there is
/// no end tick; the household reaction is "go look now".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingEvent {
    /// Doorbell camera that was pressed.
    pub camera: CameraId,
    /// Tick of the press.
    pub at: Tick,
}

impl RingEvent {
    /// A ring on `camera` at `at`.
    #[must_use]
    pub fn new(camera: impl Into<String>, at: Tick) -> Self {
        Self {
            camera: CameraId::new(camera),
            at,
        }
    }
}

/// Collapses rapid repeat detections of the same type on the same camera into
/// one logical event.
///
/// The household configures a `cooldown` (in the same tick unit the detections
/// carry). The grouper remembers, per `(camera, type)` pair, the tick of the
/// last detection it let through. A new detection of that pair is **accepted**
/// (a fresh logical event, worth notifying) only if it arrives more than
/// `cooldown` ticks after the last accepted one; otherwise it is **absorbed**
/// into the ongoing event and the cooldown window is extended.
///
/// A life-safety alarm (smoke / CO) is never absorbed — every one is accepted —
/// because suppressing a repeat smoke alarm would be unsafe.
#[derive(Debug, Clone)]
pub struct EventGrouper {
    cooldown: Tick,
    last_seen: HashMap<(String, u8), Tick>,
}

fn type_key(t: SmartDetectType) -> u8 {
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

impl EventGrouper {
    /// A grouper with the given `cooldown` window (in caller tick units).
    #[must_use]
    pub fn new(cooldown: Tick) -> Self {
        Self {
            cooldown,
            last_seen: HashMap::new(),
        }
    }

    /// Offer a detection to the grouper at tick `now`.
    ///
    /// Returns `true` if this should surface as a **fresh** logical event (the
    /// household is notified), `false` if it was absorbed into an event already
    /// in flight. A detection carrying several types is a fresh event if *any*
    /// of its types is fresh; in all cases every present type's window is
    /// (re)started so the whole detection is debounced together.
    ///
    /// `now` is taken from the detection's `start` if you prefer; here it is
    /// explicit so the caller controls the clock.
    pub fn observe(&mut self, event: &DetectionEvent, now: Tick) -> bool {
        // Safety alarms always surface.
        if event.is_safety_alarm() {
            for t in &event.types {
                self.last_seen.insert((event.camera.as_str().to_owned(), type_key(*t)), now);
            }
            return true;
        }

        let mut fresh = false;
        for t in &event.types {
            let key = (event.camera.as_str().to_owned(), type_key(*t));
            let is_fresh = match self.last_seen.get(&key) {
                // A detection that arrives strictly more than `cooldown` after
                // the last accepted one is a new event.
                Some(&last) => now.saturating_sub(last) > self.cooldown,
                None => true,
            };
            if is_fresh {
                fresh = true;
            }
            // Always extend the window so a steady stream keeps absorbing.
            self.last_seen.insert(key, now);
        }
        fresh
    }

    /// Forget all remembered windows (e.g. when the NVR reconnects).
    pub fn reset(&mut self) {
        self.last_seen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_event_carries_camera_and_tick() {
        let r = RingEvent::new("front-door", 500);
        assert_eq!(r.camera.as_str(), "front-door");
        assert_eq!(r.at, 500);
    }

    #[test]
    fn first_detection_is_always_fresh() {
        let mut g = EventGrouper::new(10);
        let e = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&e, 0));
    }

    #[test]
    fn rapid_repeats_within_cooldown_are_absorbed() {
        let mut g = EventGrouper::new(10);
        let e = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&e, 0)); // fresh
        assert!(!g.observe(&e, 3)); // absorbed
        assert!(!g.observe(&e, 9)); // absorbed
        assert!(!g.observe(&e, 19)); // still within 10 of the last (9)
    }

    #[test]
    fn a_gap_longer_than_cooldown_is_a_new_event() {
        let mut g = EventGrouper::new(10);
        let e = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&e, 0)); // fresh
        assert!(!g.observe(&e, 5)); // absorbed, window now at 5
        assert!(g.observe(&e, 16)); // 16 - 5 = 11 > 10 -> fresh again
    }

    #[test]
    fn boundary_equal_to_cooldown_is_absorbed_not_fresh() {
        let mut g = EventGrouper::new(10);
        let e = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&e, 0));
        // exactly cooldown later: > is strict, so 10 - 0 = 10 is NOT > 10.
        assert!(!g.observe(&e, 10));
    }

    #[test]
    fn different_types_group_independently() {
        let mut g = EventGrouper::new(10);
        let person = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        let car = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Vehicle);
        assert!(g.observe(&person, 0));
        assert!(g.observe(&car, 1)); // different type -> fresh despite being 1 tick later
        assert!(!g.observe(&person, 2)); // person absorbed
    }

    #[test]
    fn different_cameras_group_independently() {
        let mut g = EventGrouper::new(10);
        let a = DetectionEvent::new("cam-a", 90, 0).with_type(SmartDetectType::Person);
        let b = DetectionEvent::new("cam-b", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&a, 0));
        assert!(g.observe(&b, 1)); // other camera -> fresh
    }

    #[test]
    fn multi_type_event_is_fresh_if_any_type_is_fresh() {
        let mut g = EventGrouper::new(10);
        let person = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&person, 0));
        // person still cooling down, but vehicle is new -> whole event fresh.
        let mixed = DetectionEvent::new("c", 90, 0)
            .with_type(SmartDetectType::Person)
            .with_type(SmartDetectType::Vehicle);
        assert!(g.observe(&mixed, 2));
    }

    #[test]
    fn safety_alarms_are_never_absorbed() {
        let mut g = EventGrouper::new(1000);
        let smoke = DetectionEvent::new("kitchen", 95, 0).with_type(SmartDetectType::Smoke);
        assert!(g.observe(&smoke, 0));
        assert!(g.observe(&smoke, 1)); // every smoke detection surfaces
        assert!(g.observe(&smoke, 2));
    }

    #[test]
    fn reset_forgets_windows() {
        let mut g = EventGrouper::new(100);
        let e = DetectionEvent::new("c", 90, 0).with_type(SmartDetectType::Person);
        assert!(g.observe(&e, 0));
        assert!(!g.observe(&e, 1));
        g.reset();
        assert!(g.observe(&e, 2)); // fresh again after reset
    }
}
