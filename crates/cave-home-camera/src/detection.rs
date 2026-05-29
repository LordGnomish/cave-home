//! A single detection and the filters that decide whether it matters.
//!
//! A [`Detection`] is what the (Phase-1b) inference step hands the brain: a
//! label, a confidence in `0..=1`, a bounding box and the frame's tick. This
//! module does the cheap, deterministic reasoning the household actually cares
//! about: drop low-confidence guesses, keep only the labels a camera is
//! watching for, decide whether a detection sits inside a zone, and decide
//! whether a thing is standing still or moving between two frames.
//!
//! The crate reads no clock: a tick is a caller-supplied whole-second (or
//! frame) counter.

use crate::geometry::{iou, BBox, Point};
use crate::label::ObjectLabel;

/// A whole-second (or frame) counter supplied by the caller. The crate never
/// reads a real clock.
pub type Tick = u64;

/// One thing a detector found in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    /// What was found.
    pub label: ObjectLabel,
    /// How sure the detector was, clamped to `0.0..=1.0` by [`Detection::new`].
    pub score: f64,
    /// Where it was, in normalised picture space.
    pub bbox: BBox,
    /// The frame this was found in.
    pub tick: Tick,
}

impl Detection {
    /// A detection. `score` is clamped into `0.0..=1.0` so a miscalibrated or
    /// out-of-range confidence can never make a later threshold compare against
    /// garbage.
    #[must_use]
    pub const fn new(label: ObjectLabel, score: f64, bbox: BBox, tick: Tick) -> Self {
        let score = if score.is_nan() { 0.0 } else { score.clamp(0.0, 1.0) };
        Self {
            label,
            score,
            bbox,
            tick,
        }
    }

    /// Whether the detection is confident enough, i.e. `score >= min_score`.
    #[must_use]
    pub fn meets_score(&self, min_score: f64) -> bool {
        self.score >= min_score
    }

    /// Where this detection counts as "being" for zone tests.
    #[must_use]
    pub fn anchor(&self, anchor: ZoneAnchor) -> Point {
        match anchor {
            ZoneAnchor::Center => self.bbox.center(),
            ZoneAnchor::BottomCenter => self.bbox.bottom_center(),
        }
    }
}

/// Which point of a detection's box is tested for zone membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum ZoneAnchor {
    /// The geometric centre of the box.
    Center,
    /// The middle of the bottom edge — where a standing person / a car meets
    /// the ground. The natural default for "is it in the driveway".
    #[default]
    BottomCenter,
}


/// Whether a thing is holding still or moving, judged by how much its box moved
/// between two frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// The box barely moved — within the threshold.
    Stationary,
    /// The box moved more than the threshold.
    Moving,
}

/// Classify movement between two boxes of the *same* thing across frames.
///
/// "Moved" is measured as the distance its centre travelled. If that distance
/// is at most `threshold` (in the same normalised units as the boxes) the thing
/// is [`Motion::Stationary`], otherwise [`Motion::Moving`]. A parked car or a
/// person waiting at the door reads as stationary; someone walking up the path
/// reads as moving. The boundary is inclusive: a move of exactly `threshold`
/// still counts as stationary.
#[must_use]
pub fn classify_motion(previous: &BBox, current: &BBox, threshold: f64) -> Motion {
    let a = previous.center();
    let b = current.center();
    let dist = (b.x - a.x).hypot(b.y - a.y);
    if dist <= threshold {
        Motion::Stationary
    } else {
        Motion::Moving
    }
}

/// An alternative movement test using box overlap rather than centre distance.
///
/// A thing whose box still overlaps its previous box by at least `min_iou` is
/// treated as holding its ground. Useful when a thing changes size (steps
/// toward the camera) without its centre moving far.
#[must_use]
pub fn is_stationary_by_overlap(previous: &BBox, current: &BBox, min_iou: f64) -> bool {
    iou(previous, current) >= min_iou
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_is_clamped_into_unit_range() {
        let b = BBox::new(0.0, 0.0, 1.0, 1.0);
        assert_eq!(Detection::new(ObjectLabel::Person, 1.5, b, 0).score, 1.0);
        assert_eq!(Detection::new(ObjectLabel::Person, -0.2, b, 0).score, 0.0);
        assert_eq!(Detection::new(ObjectLabel::Person, f64::NAN, b, 0).score, 0.0);
        assert!((Detection::new(ObjectLabel::Person, 0.73, b, 0).score - 0.73).abs() < 1e-9);
    }

    #[test]
    fn meets_score_is_inclusive_at_threshold() {
        let d = Detection::new(ObjectLabel::Person, 0.50, BBox::new(0.0, 0.0, 1.0, 1.0), 0);
        assert!(d.meets_score(0.50));
        assert!(d.meets_score(0.40));
        assert!(!d.meets_score(0.51));
    }

    #[test]
    fn anchor_picks_the_right_point() {
        let d = Detection::new(ObjectLabel::Car, 0.9, BBox::new(0.0, 0.0, 10.0, 20.0), 0);
        assert_eq!(d.anchor(ZoneAnchor::Center), Point::new(5.0, 10.0));
        assert_eq!(d.anchor(ZoneAnchor::BottomCenter), Point::new(5.0, 20.0));
    }

    #[test]
    fn default_anchor_is_bottom_center() {
        assert_eq!(ZoneAnchor::default(), ZoneAnchor::BottomCenter);
    }

    #[test]
    fn stationary_box_reads_as_stationary() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(0.5, 0.5, 10.0, 10.0); // centre moved ~0.707
        assert_eq!(classify_motion(&a, &b, 1.0), Motion::Stationary);
    }

    #[test]
    fn moved_box_reads_as_moving() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(5.0, 0.0, 10.0, 10.0); // centre moved 5.0
        assert_eq!(classify_motion(&a, &b, 1.0), Motion::Moving);
    }

    #[test]
    fn motion_threshold_boundary_is_stationary() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(3.0, 0.0, 10.0, 10.0); // centre moved exactly 3.0
        assert_eq!(classify_motion(&a, &b, 3.0), Motion::Stationary);
    }

    #[test]
    fn overlap_stationarity() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let same = BBox::new(0.0, 0.0, 10.0, 10.0);
        let shifted = BBox::new(8.0, 0.0, 10.0, 10.0);
        assert!(is_stationary_by_overlap(&a, &same, 0.5));
        assert!(!is_stationary_by_overlap(&a, &shifted, 0.5));
    }
}
