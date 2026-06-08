//! A detection zone: a named region of the picture with its own rules.
//!
//! A household draws a polygon over the camera view — "the driveway", "the
//! front door" — and says what they care about there: which things matter
//! (a person, a car) and how confident the detector must be. A [`Zone`] bundles
//! that polygon with those rules, and [`Zone::accepts`] answers the only
//! question the rest of the pillar asks: *does this detection count for this
//! zone?*
//!
//! This module also exposes [`filter`], the small pipeline that runs a frame's
//! worth of detections through a zone and hands back only the ones that matter.

use crate::detection::{Detection, ZoneAnchor};
use crate::geometry::Polygon;
use crate::label::{Lang, ObjectLabel};

/// A named, rule-bearing region of the camera view.
#[derive(Debug, Clone)]
pub struct Zone {
    name: String,
    polygon: Polygon,
    /// The labels this zone cares about. Empty means "any recognised thing".
    required_labels: Vec<ObjectLabel>,
    /// The minimum confidence a detection needs here, in `0.0..=1.0`.
    min_score: f64,
    /// Which point of a box is tested against the polygon.
    anchor: ZoneAnchor,
}

impl Zone {
    /// Build a zone.
    ///
    /// `name` is the stable identifier (`front_door`); the household-facing
    /// label comes from [`Zone::friendly_name`]. `required_labels` empty means
    /// the zone reacts to any recognised thing. `min_score` is clamped into
    /// `0.0..=1.0`.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        polygon: Polygon,
        required_labels: Vec<ObjectLabel>,
        min_score: f64,
    ) -> Self {
        Self {
            name: name.into(),
            polygon,
            required_labels,
            min_score: if min_score.is_nan() {
                0.0
            } else {
                min_score.clamp(0.0, 1.0)
            },
            anchor: ZoneAnchor::default(),
        }
    }

    /// Use a different box anchor for membership (default is bottom-centre).
    #[must_use]
    pub const fn with_anchor(mut self, anchor: ZoneAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// The zone's stable identifier.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The minimum confidence configured for this zone.
    #[must_use]
    pub const fn min_score(&self) -> f64 {
        self.min_score
    }

    /// The labels this zone reacts to (empty = any).
    #[must_use]
    pub fn required_labels(&self) -> &[ObjectLabel] {
        &self.required_labels
    }

    /// Whether `label` is one this zone reacts to. An empty required-label set
    /// means the zone reacts to anything recognised.
    #[must_use]
    pub fn wants_label(&self, label: ObjectLabel) -> bool {
        self.required_labels.is_empty() || self.required_labels.contains(&label)
    }

    /// Whether a detection's anchor point lies within the zone polygon.
    #[must_use]
    pub fn contains(&self, det: &Detection) -> bool {
        self.polygon.contains(det.anchor(self.anchor))
    }

    /// The single accept test the pillar asks: confident enough, a label we
    /// want, and physically inside the zone.
    #[must_use]
    pub fn accepts(&self, det: &Detection) -> bool {
        det.meets_score(self.min_score) && self.wants_label(det.label) && self.contains(det)
    }

    /// A grandma-friendly place name for this zone, derived from its
    /// identifier. Known zones get a hand-written localised name; an unknown
    /// identifier falls back to its readable form (underscores → spaces) so the
    /// UI still says something sensible rather than a raw token.
    #[must_use]
    pub fn friendly_name(&self, lang: Lang) -> String {
        match (self.name.as_str(), lang) {
            ("front_door", Lang::En) => "front door".into(),
            ("front_door", Lang::De) => "Haustür".into(),
            ("front_door", Lang::Tr) => "ön kapı".into(),
            ("driveway", Lang::En) => "driveway".into(),
            ("driveway", Lang::De) => "Einfahrt".into(),
            ("driveway", Lang::Tr) => "garaj yolu".into(),
            ("back_garden", Lang::En) => "back garden".into(),
            ("back_garden", Lang::De) => "Garten".into(),
            ("back_garden", Lang::Tr) => "arka bahçe".into(),
            _ => self.name.replace('_', " "),
        }
    }
}

/// Keep only the detections a zone accepts, in their original frame order.
///
/// Each detection in `detections` is kept when the zone accepts it (confident
/// enough, a wanted label, inside the polygon). The pipeline filters; it does
/// not reorder.
#[must_use]
pub fn filter(zone: &Zone, detections: &[Detection]) -> Vec<Detection> {
    detections
        .iter()
        .copied()
        .filter(|d| zone.accepts(d))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{BBox, Point};

    fn driveway() -> Polygon {
        Polygon::new(vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
        ])
        .expect("valid polygon")
    }

    fn det(label: ObjectLabel, score: f64, bbox: BBox) -> Detection {
        Detection::new(label, score, bbox, 0)
    }

    #[test]
    fn min_score_is_clamped() {
        let z = Zone::new("driveway", driveway(), vec![], 5.0);
        assert!((z.min_score() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_required_labels_means_any() {
        let z = Zone::new("driveway", driveway(), vec![], 0.0);
        assert!(z.wants_label(ObjectLabel::Person));
        assert!(z.wants_label(ObjectLabel::Cat));
    }

    #[test]
    fn required_labels_are_enforced() {
        let z = Zone::new("driveway", driveway(), vec![ObjectLabel::Car], 0.0);
        assert!(z.wants_label(ObjectLabel::Car));
        assert!(!z.wants_label(ObjectLabel::Person));
    }

    #[test]
    fn bottom_center_inside_zone_is_member() {
        let z = Zone::new("driveway", driveway(), vec![], 0.0);
        // Box bottom-centre at (50, 80): inside.
        let d = det(ObjectLabel::Car, 0.9, BBox::new(40.0, 60.0, 20.0, 20.0));
        assert!(z.contains(&d));
    }

    #[test]
    fn bottom_center_outside_zone_is_not_member() {
        let z = Zone::new("driveway", driveway(), vec![], 0.0);
        // Box bottom-centre at (150, 80): outside on x.
        let d = det(ObjectLabel::Car, 0.9, BBox::new(140.0, 60.0, 20.0, 20.0));
        assert!(!z.contains(&d));
    }

    #[test]
    fn anchor_choice_changes_membership() {
        // A box whose centre is inside but whose bottom edge falls below the
        // zone: anchor choice flips membership.
        let z_center = Zone::new("driveway", driveway(), vec![], 0.0)
            .with_anchor(ZoneAnchor::Center);
        let z_bottom = Zone::new("driveway", driveway(), vec![], 0.0)
            .with_anchor(ZoneAnchor::BottomCenter);
        // centre (50,90) inside; bottom-centre (50,120) outside.
        let d = det(ObjectLabel::Person, 0.9, BBox::new(40.0, 60.0, 20.0, 60.0));
        assert!(z_center.contains(&d));
        assert!(!z_bottom.contains(&d));
    }

    #[test]
    fn accepts_requires_score_label_and_location() {
        let z = Zone::new("driveway", driveway(), vec![ObjectLabel::Car], 0.6);
        let good = det(ObjectLabel::Car, 0.7, BBox::new(40.0, 60.0, 20.0, 20.0));
        let low_score = det(ObjectLabel::Car, 0.5, BBox::new(40.0, 60.0, 20.0, 20.0));
        let wrong_label = det(ObjectLabel::Person, 0.9, BBox::new(40.0, 60.0, 20.0, 20.0));
        let outside = det(ObjectLabel::Car, 0.9, BBox::new(200.0, 200.0, 20.0, 20.0));
        assert!(z.accepts(&good));
        assert!(!z.accepts(&low_score));
        assert!(!z.accepts(&wrong_label));
        assert!(!z.accepts(&outside));
    }

    #[test]
    fn filter_keeps_only_accepted_detections() {
        let z = Zone::new("driveway", driveway(), vec![ObjectLabel::Car], 0.6);
        let frame = vec![
            det(ObjectLabel::Car, 0.7, BBox::new(40.0, 60.0, 20.0, 20.0)), // keep
            det(ObjectLabel::Person, 0.9, BBox::new(40.0, 60.0, 20.0, 20.0)), // wrong label
            det(ObjectLabel::Car, 0.4, BBox::new(40.0, 60.0, 20.0, 20.0)), // low score
            det(ObjectLabel::Car, 0.95, BBox::new(300.0, 300.0, 20.0, 20.0)), // outside
        ];
        let kept = filter(&z, &frame);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].label, ObjectLabel::Car);
    }

    #[test]
    fn friendly_name_known_and_fallback() {
        let known = Zone::new("front_door", driveway(), vec![], 0.0);
        assert_eq!(known.friendly_name(Lang::En), "front door");
        assert_eq!(known.friendly_name(Lang::De), "Haustür");
        let unknown = Zone::new("side_gate", driveway(), vec![], 0.0);
        assert_eq!(unknown.friendly_name(Lang::En), "side gate");
    }
}
