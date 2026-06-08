//! What a single camera is set up to do.
//!
//! A [`CameraConfig`] is the household's settings for one camera: its id and
//! friendly name, which things it should bother watching for, the zones drawn
//! over its view, when it should record, and how long clips and events are kept.
//! It carries no live state and reads no clock — it is the static description a
//! running pipeline (Phase 1b) would consume.

use crate::label::ObjectLabel;
use crate::zone::Zone;

/// When a camera writes recordings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordMode {
    /// Never record (live view only).
    Off,
    /// Record only around detection / motion events. The privacy-friendly
    /// default: most of the day nothing is written.
    #[default]
    MotionOnly,
    /// Record continuously, 24/7.
    Continuous,
}

impl RecordMode {
    /// Whether this mode ever records at all.
    #[must_use]
    pub const fn records_anything(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Settings for one camera.
#[derive(Debug, Clone)]
pub struct CameraConfig {
    id: String,
    name: String,
    enabled_labels: Vec<ObjectLabel>,
    zones: Vec<Zone>,
    record_mode: RecordMode,
    retention_days: u32,
}

impl CameraConfig {
    /// A camera with the given stable `id` and friendly `name`, recording in
    /// the privacy-friendly [`RecordMode::MotionOnly`] default, watching for
    /// nothing yet, with no zones and a `retention_days` of zero. Use the
    /// builder methods to fill it in.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled_labels: Vec::new(),
            zones: Vec::new(),
            record_mode: RecordMode::default(),
            retention_days: 0,
        }
    }

    /// Set the things this camera watches for.
    #[must_use]
    pub fn with_labels(mut self, labels: Vec<ObjectLabel>) -> Self {
        self.enabled_labels = labels;
        self
    }

    /// Add a zone.
    #[must_use]
    pub fn with_zone(mut self, zone: Zone) -> Self {
        self.zones.push(zone);
        self
    }

    /// Set the record mode.
    #[must_use]
    pub const fn with_record_mode(mut self, mode: RecordMode) -> Self {
        self.record_mode = mode;
        self
    }

    /// Set how many days clips / events are kept.
    #[must_use]
    pub const fn with_retention_days(mut self, days: u32) -> Self {
        self.retention_days = days;
        self
    }

    /// The stable identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The friendly name shown to the household.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configured zones.
    #[must_use]
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// The record mode.
    #[must_use]
    pub const fn record_mode(&self) -> RecordMode {
        self.record_mode
    }

    /// How many days clips / events are kept.
    #[must_use]
    pub const fn retention_days(&self) -> u32 {
        self.retention_days
    }

    /// Whether this camera is watching for `label`. An empty enabled set means
    /// the camera is not watching for anything specific yet.
    #[must_use]
    pub fn watches(&self, label: ObjectLabel) -> bool {
        self.enabled_labels.contains(&label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Polygon};

    fn a_zone() -> Zone {
        Zone::new(
            "driveway",
            Polygon::new(vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
            ])
            .expect("triangle"),
            vec![],
            0.5,
        )
    }

    #[test]
    fn default_record_mode_is_motion_only() {
        assert_eq!(RecordMode::default(), RecordMode::MotionOnly);
        assert!(RecordMode::MotionOnly.records_anything());
        assert!(RecordMode::Continuous.records_anything());
        assert!(!RecordMode::Off.records_anything());
    }

    #[test]
    fn builder_assembles_a_camera() {
        let cam = CameraConfig::new("front", "Front door camera")
            .with_labels(vec![ObjectLabel::Person, ObjectLabel::DeliveryVan])
            .with_zone(a_zone())
            .with_record_mode(RecordMode::Continuous)
            .with_retention_days(14);
        assert_eq!(cam.id(), "front");
        assert_eq!(cam.name(), "Front door camera");
        assert_eq!(cam.zones().len(), 1);
        assert_eq!(cam.record_mode(), RecordMode::Continuous);
        assert_eq!(cam.retention_days(), 14);
        assert!(cam.watches(ObjectLabel::Person));
        assert!(!cam.watches(ObjectLabel::Cat));
    }

    #[test]
    fn fresh_camera_has_safe_defaults() {
        let cam = CameraConfig::new("x", "X");
        assert_eq!(cam.record_mode(), RecordMode::MotionOnly);
        assert_eq!(cam.retention_days(), 0);
        assert!(cam.zones().is_empty());
        assert!(!cam.watches(ObjectLabel::Person));
    }
}
