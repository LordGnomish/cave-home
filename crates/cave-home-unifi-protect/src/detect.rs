//! Smart-detection taxonomy and the detection-event model.
//!
//! UniFi Protect cameras run on-device smart detection: rather than raw motion,
//! they classify *what* moved — a person, a vehicle, a package on the step, an
//! animal, a licence plate, a recognised face — and the newer models also raise
//! safety alarms (smoke, CO). This module is the vendor-neutral shape of those
//! detections: the [`SmartDetectType`] taxonomy and a [`DetectionEvent`] that
//! the (deferred, Phase-1b) WebSocket transport fills in and the recording /
//! zone / privacy / phrasing layers reason about.
//!
//! The crate reads no clock: a tick is a caller-supplied whole-second (or
//! millisecond) counter, exactly like the sibling camera pillar.
//!
//! Modelled from the public Protect smart-detect type list and the HA
//! `unifiprotect` integration (Apache-2.0). No GPL source was read.

/// A caller-supplied monotonic counter (whole seconds or milliseconds — the
/// crate never interprets the unit, it only compares and subtracts). The crate
/// never reads a real clock.
pub type Tick = u64;

/// What a UniFi Protect camera's smart detection recognised.
///
/// This is the household-meaningful set the public Protect API exposes as
/// `smartDetectTypes`. The string forms match the wire vocabulary so the
/// deferred transport can map them directly; the household never sees these
/// strings (Charter §6.3 — see [`crate::label`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartDetectType {
    /// A person.
    Person,
    /// A vehicle (car / van / motorbike).
    Vehicle,
    /// A package left in view.
    Package,
    /// An animal / pet.
    Animal,
    /// A licence plate was read.
    LicensePlate,
    /// A recognised (enrolled) face.
    FaceKnown,
    /// Smoke-alarm sound was heard.
    Smoke,
    /// Carbon-monoxide-alarm sound was heard.
    CoAlarm,
}

impl SmartDetectType {
    /// Every smart-detect type, for iteration in tests and config UIs.
    pub const ALL: [Self; 8] = [
        Self::Person,
        Self::Vehicle,
        Self::Package,
        Self::Animal,
        Self::LicensePlate,
        Self::FaceKnown,
        Self::Smoke,
        Self::CoAlarm,
    ];

    /// The wire string the NVR uses (Protect `smartDetectTypes`). Internal —
    /// never surfaced to the household.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Vehicle => "vehicle",
            Self::Package => "package",
            Self::Animal => "animal",
            Self::LicensePlate => "licensePlate",
            Self::FaceKnown => "face",
            Self::Smoke => "smoke",
            Self::CoAlarm => "co",
        }
    }

    /// Parse a Protect `smartDetectTypes` wire string back into a type.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "person" => Self::Person,
            "vehicle" => Self::Vehicle,
            "package" => Self::Package,
            "animal" => Self::Animal,
            "licensePlate" => Self::LicensePlate,
            "face" => Self::FaceKnown,
            "smoke" => Self::Smoke,
            "co" => Self::CoAlarm,
            _ => return None,
        })
    }

    /// Whether this detection is a life-safety alarm (smoke or CO). Those are
    /// treated as urgent everywhere downstream — they record regardless of mode
    /// and notify even inside a privacy window.
    #[must_use]
    pub const fn is_safety_alarm(self) -> bool {
        matches!(self, Self::Smoke | Self::CoAlarm)
    }
}

/// One smart-detection the NVR raised on one camera.
///
/// A single event can carry more than one type (the camera saw a person *and*
/// read their licence plate in the same moment). `score` is the camera's
/// confidence, clamped to `0..=100` on construction so a downstream threshold
/// never compares against garbage. `start`/`end` are caller ticks; `end` is
/// `None` while the event is still firing. `thumbnail_id` is the opaque handle
/// the deferred transport would use to fetch the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionEvent {
    /// Camera that raised the event.
    pub camera: crate::device::CameraId,
    /// The detected types (one event may carry several).
    pub types: Vec<SmartDetectType>,
    /// Confidence 0..=100.
    pub score: u8,
    /// Start tick (caller clock).
    pub start: Tick,
    /// End tick, or `None` while still firing.
    pub end: Option<Tick>,
    /// Opaque thumbnail / snapshot handle, if the NVR attached one.
    pub thumbnail_id: Option<String>,
}

impl DetectionEvent {
    /// A detection on `camera` with confidence `score` (clamped to `0..=100`)
    /// starting at `start`, no types yet, still active, no thumbnail. Builder
    /// methods layer on the detected types, an end tick and a thumbnail.
    #[must_use]
    pub fn new(camera: impl Into<String>, score: u8, start: Tick) -> Self {
        Self {
            camera: crate::device::CameraId::new(camera),
            types: Vec::new(),
            score: score.min(100),
            start,
            end: None,
            thumbnail_id: None,
        }
    }

    /// Add a detected type (de-duplicated).
    #[must_use]
    pub fn with_type(mut self, t: SmartDetectType) -> Self {
        if !self.types.contains(&t) {
            self.types.push(t);
        }
        self
    }

    /// Close the event at `end`.
    #[must_use]
    pub fn ended_at(mut self, end: Tick) -> Self {
        self.end = Some(end);
        self
    }

    /// Attach a thumbnail handle.
    #[must_use]
    pub fn with_thumbnail(mut self, id: impl Into<String>) -> Self {
        self.thumbnail_id = Some(id.into());
        self
    }

    /// Whether the event carries the given type.
    #[must_use]
    pub fn has_type(&self, t: SmartDetectType) -> bool {
        self.types.contains(&t)
    }

    /// Whether the event is still firing (no end tick).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.end.is_none()
    }

    /// Whether any detected type is a life-safety alarm.
    #[must_use]
    pub fn is_safety_alarm(&self) -> bool {
        self.types.iter().any(|t| t.is_safety_alarm())
    }

    /// The single most important type for phrasing a one-line headline. Safety
    /// alarms win; otherwise a person, then whatever was detected first.
    #[must_use]
    pub fn primary_type(&self) -> Option<SmartDetectType> {
        if let Some(t) = self.types.iter().copied().find(|t| t.is_safety_alarm()) {
            return Some(t);
        }
        if self.types.contains(&SmartDetectType::Person) {
            return Some(SmartDetectType::Person);
        }
        self.types.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips_for_every_type() {
        for t in SmartDetectType::ALL {
            assert_eq!(SmartDetectType::parse(t.wire()), Some(t), "{t:?}");
        }
        assert_eq!(SmartDetectType::parse("teleporter"), None);
    }

    #[test]
    fn score_is_clamped_to_0_100() {
        assert_eq!(DetectionEvent::new("c", 250, 0).score, 100);
        assert_eq!(DetectionEvent::new("c", 73, 0).score, 73);
    }

    #[test]
    fn builder_adds_types_without_duplicates() {
        let e = DetectionEvent::new("c", 90, 10)
            .with_type(SmartDetectType::Person)
            .with_type(SmartDetectType::Person)
            .with_type(SmartDetectType::LicensePlate);
        assert_eq!(e.types.len(), 2);
        assert!(e.has_type(SmartDetectType::Person));
        assert!(e.has_type(SmartDetectType::LicensePlate));
        assert!(!e.has_type(SmartDetectType::Animal));
    }

    #[test]
    fn active_until_ended() {
        let e = DetectionEvent::new("c", 90, 10);
        assert!(e.is_active());
        assert!(!e.ended_at(20).is_active());
    }

    #[test]
    fn thumbnail_is_optional() {
        assert!(DetectionEvent::new("c", 1, 0).thumbnail_id.is_none());
        let e = DetectionEvent::new("c", 1, 0).with_thumbnail("thumb-7");
        assert_eq!(e.thumbnail_id.as_deref(), Some("thumb-7"));
    }

    #[test]
    fn safety_alarms_are_flagged() {
        assert!(SmartDetectType::Smoke.is_safety_alarm());
        assert!(SmartDetectType::CoAlarm.is_safety_alarm());
        assert!(!SmartDetectType::Person.is_safety_alarm());
        let e = DetectionEvent::new("kitchen", 80, 0).with_type(SmartDetectType::Smoke);
        assert!(e.is_safety_alarm());
    }

    #[test]
    fn primary_type_prefers_safety_then_person() {
        let e = DetectionEvent::new("c", 80, 0)
            .with_type(SmartDetectType::Vehicle)
            .with_type(SmartDetectType::Person)
            .with_type(SmartDetectType::Smoke);
        assert_eq!(e.primary_type(), Some(SmartDetectType::Smoke));

        let e = DetectionEvent::new("c", 80, 0)
            .with_type(SmartDetectType::Vehicle)
            .with_type(SmartDetectType::Person);
        assert_eq!(e.primary_type(), Some(SmartDetectType::Person));

        let e = DetectionEvent::new("c", 80, 0).with_type(SmartDetectType::Vehicle);
        assert_eq!(e.primary_type(), Some(SmartDetectType::Vehicle));

        assert_eq!(DetectionEvent::new("c", 80, 0).primary_type(), None);
    }
}
