//! Object-tracking-lite: stitch per-frame detections into things that persist.
//!
//! A detector looks at each frame fresh — it has no idea that the person it sees
//! now is the same person it saw a moment ago. To say "a person walked up the
//! path" (one event) instead of "person, person, person" (thirty events) the
//! brain has to associate this frame's boxes with the things it is already
//! following. The cheap, well-understood way to do that is overlap: a box that
//! sits where a tracked thing was almost certainly *is* that thing.
//!
//! [`Tracker`] keeps a list of [`TrackedObject`]s and, each frame, greedily
//! matches incoming detections to them by highest `IoU` above a threshold.
//! Unmatched detections start new tracks; tracks that go unseen for too long
//! are dropped. This is a deliberately simple matcher — no Kalman filter, no
//! re-identification — which is exactly the slice that needs no model or video.

use crate::detection::{Detection, Tick};
use crate::geometry::{iou, BBox};
use crate::label::ObjectLabel;

/// A stable identifier for a tracked thing, unique within one [`Tracker`].
pub type TrackId = u64;

/// One thing the tracker is following across frames.
#[derive(Debug, Clone)]
pub struct TrackedObject {
    /// Stable id for the life of this track.
    pub id: TrackId,
    /// What it is.
    pub label: ObjectLabel,
    /// Its box as of the most recent frame it was seen.
    pub bbox: BBox,
    /// The most recent confidence.
    pub score: f64,
    /// The frame it was first seen.
    pub first_seen: Tick,
    /// The frame it was most recently seen.
    pub last_seen: Tick,
    /// How many frames it has been seen in.
    pub hits: u32,
}

impl TrackedObject {
    /// How many ticks since this track was last updated, relative to `now`
    /// (saturating, so a non-monotonic `now` reads as zero rather than wrapping).
    #[must_use]
    pub const fn age(&self, now: Tick) -> Tick {
        now.saturating_sub(self.last_seen)
    }
}

/// A greedy, IoU-based multi-object tracker.
#[derive(Debug, Clone)]
pub struct Tracker {
    tracks: Vec<TrackedObject>,
    next_id: TrackId,
    /// Minimum `IoU` for a detection to be considered the same thing as a track.
    min_iou: f64,
    /// Drop a track once it has gone unseen this many ticks.
    max_age: Tick,
}

impl Tracker {
    /// A tracker that associates boxes overlapping by at least `min_iou`, and
    /// forgets a thing it has not seen for `max_age` ticks. `min_iou` is clamped
    /// into `0.0..=1.0`.
    #[must_use]
    pub const fn new(min_iou: f64, max_age: Tick) -> Self {
        Self {
            tracks: Vec::new(),
            next_id: 0,
            min_iou: if min_iou.is_nan() {
                0.0
            } else {
                min_iou.clamp(0.0, 1.0)
            },
            max_age,
        }
    }

    /// The things currently being followed.
    #[must_use]
    pub fn tracks(&self) -> &[TrackedObject] {
        &self.tracks
    }

    /// Feed one frame's detections (all at the same `now` tick), associate them
    /// with existing tracks by greedy best-IoU matching, start tracks for the
    /// unmatched, and retire tracks that have aged out. Returns the ids that
    /// were updated or created this frame, in the input detection order.
    ///
    /// Matching is greedy: every (detection, track) pair whose label agrees and
    /// whose `IoU` clears the threshold is scored, the pairs are taken highest-IoU
    /// first, and each detection and each track is used at most once. Greedy `IoU`
    /// is the standard lightweight association and is enough for the household
    /// "is this the same visitor" question; a globally optimal assignment is a
    /// later refinement.
    pub fn update(&mut self, detections: &[Detection], now: Tick) -> Vec<TrackId> {
        // Score every compatible (detection, track) pair.
        let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
        for (di, det) in detections.iter().enumerate() {
            for (ti, track) in self.tracks.iter().enumerate() {
                if track.label != det.label {
                    continue;
                }
                let score = iou(&det.bbox, &track.bbox);
                if score >= self.min_iou && score > 0.0 {
                    candidates.push((score, di, ti));
                }
            }
        }
        // Greedy: take the strongest overlaps first.
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(core::cmp::Ordering::Equal));

        let mut det_used = vec![false; detections.len()];
        let mut track_used = vec![false; self.tracks.len()];
        // Per-detection assigned id (None = will start a new track).
        let mut assigned: Vec<Option<TrackId>> = vec![None; detections.len()];

        for (_score, di, ti) in candidates {
            if det_used[di] || track_used[ti] {
                continue;
            }
            det_used[di] = true;
            track_used[ti] = true;
            let det = &detections[di];
            let track = &mut self.tracks[ti];
            track.bbox = det.bbox;
            track.score = det.score;
            track.last_seen = now;
            track.hits = track.hits.saturating_add(1);
            assigned[di] = Some(track.id);
        }

        // Unmatched detections become new tracks.
        for (di, det) in detections.iter().enumerate() {
            if det_used[di] {
                continue;
            }
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.tracks.push(TrackedObject {
                id,
                label: det.label,
                bbox: det.bbox,
                score: det.score,
                first_seen: now,
                last_seen: now,
                hits: 1,
            });
            assigned[di] = Some(id);
        }

        // Retire stale tracks.
        let max_age = self.max_age;
        self.tracks.retain(|t| t.age(now) <= max_age);

        assigned.into_iter().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(label: ObjectLabel, bbox: BBox, tick: Tick) -> Detection {
        Detection::new(label, 0.9, bbox, tick)
    }

    #[test]
    fn min_iou_is_clamped() {
        let t = Tracker::new(5.0, 30);
        // No public getter; exercise via behaviour: with clamp to 1.0, a
        // partial overlap should NOT match, so a shifted box starts a new track.
        let mut t = t;
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(2.0, 0.0, 10.0, 10.0); // IoU < 1.0
        t.update(&[det(ObjectLabel::Person, a, 0)], 0);
        t.update(&[det(ObjectLabel::Person, b, 1)], 1);
        assert_eq!(t.tracks().len(), 2, "perfect-overlap-only tracker splits");
    }

    #[test]
    fn first_frame_starts_a_track_per_detection() {
        let mut t = Tracker::new(0.3, 30);
        let ids = t.update(
            &[
                det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0),
                det(ObjectLabel::Car, BBox::new(50.0, 50.0, 20.0, 20.0), 0),
            ],
            0,
        );
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(t.tracks().len(), 2);
    }

    #[test]
    fn overlapping_detection_continues_the_same_track() {
        let mut t = Tracker::new(0.3, 30);
        let id0 = t.update(&[det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0)], 0)[0];
        // Next frame, box drifted a little but still overlaps strongly.
        let id1 = t.update(&[det(ObjectLabel::Person, BBox::new(1.0, 0.0, 10.0, 10.0), 1)], 1)[0];
        assert_eq!(id0, id1, "same thing keeps its id");
        assert_eq!(t.tracks().len(), 1);
        assert_eq!(t.tracks()[0].hits, 2);
        assert_eq!(t.tracks()[0].first_seen, 0);
        assert_eq!(t.tracks()[0].last_seen, 1);
    }

    #[test]
    fn label_mismatch_does_not_associate() {
        let mut t = Tracker::new(0.1, 30);
        let id0 = t.update(&[det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0)], 0)[0];
        // A car sitting exactly where the person was is NOT the person.
        let id1 = t.update(&[det(ObjectLabel::Car, BBox::new(0.0, 0.0, 10.0, 10.0), 1)], 1)[0];
        assert_ne!(id0, id1);
        // The person track ages but is still alive at age 1.
        assert_eq!(t.tracks().len(), 2);
    }

    #[test]
    fn greedy_matcher_pairs_best_overlaps_first() {
        let mut t = Tracker::new(0.1, 30);
        // Two tracks side by side.
        t.update(
            &[
                det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0),
                det(ObjectLabel::Person, BBox::new(100.0, 0.0, 10.0, 10.0), 0),
            ],
            0,
        );
        let left = t.tracks()[0].id;
        let right = t.tracks()[1].id;
        // Next frame: two detections, each clearly closest to one track.
        let ids = t.update(
            &[
                det(ObjectLabel::Person, BBox::new(101.0, 0.0, 10.0, 10.0), 1), // near right
                det(ObjectLabel::Person, BBox::new(1.0, 0.0, 10.0, 10.0), 1),   // near left
            ],
            1,
        );
        assert_eq!(ids[0], right);
        assert_eq!(ids[1], left);
        assert_eq!(t.tracks().len(), 2, "no spurious new tracks");
    }

    #[test]
    fn stale_track_is_retired_after_max_age() {
        let mut t = Tracker::new(0.3, 5);
        t.update(&[det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0)], 0);
        assert_eq!(t.tracks().len(), 1);
        // Six ticks later with no detections: age 6 > max_age 5 -> retired.
        let ids = t.update(&[], 6);
        assert!(ids.is_empty());
        assert!(t.tracks().is_empty());
    }

    #[test]
    fn track_within_max_age_survives_a_gap() {
        let mut t = Tracker::new(0.3, 5);
        let id0 = t.update(&[det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 0)], 0)[0];
        // Empty frame at tick 3 (age 3 <= 5): survives.
        t.update(&[], 3);
        assert_eq!(t.tracks().len(), 1);
        // Reappears at the same place at tick 4 -> same id.
        let id1 = t.update(&[det(ObjectLabel::Person, BBox::new(0.0, 0.0, 10.0, 10.0), 4)], 4)[0];
        assert_eq!(id0, id1);
    }
}
