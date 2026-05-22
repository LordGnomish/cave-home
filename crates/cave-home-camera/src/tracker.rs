// SPDX-License-Identifier: Apache-2.0
//! IOU-based multi-object tracker — link detections across frames into
//! stable track ids.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/track/norfair_tracker.py + frigate/track/object_processing.py.
//!
//! Frigate uses Norfair (Kalman-filtered) by default; Phase 1 ports the
//! simpler IOU-matching tracker that Frigate fell back to in v0.13 and
//! earlier (`frigate.track.iou_tracker`). The contract is identical
//! from the outside: `update(detections) -> Vec<TrackedObject>` with
//! stable `object_id`s.

use std::collections::BTreeMap;

use crate::detectors::Detection;

/// One tracked object across frames.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackedObject {
    /// Tracker-assigned, stable across frames while the object is alive.
    pub object_id: u64,
    /// Latest matched detection.
    pub detection: Detection,
    /// Number of consecutive frames the object has been seen.
    pub hits: u32,
    /// Number of consecutive frames the object has been missed
    /// (resets to 0 on every match).
    pub misses: u32,
}

/// IoU-based tracker state.
#[derive(Debug)]
pub struct IouTracker {
    /// IoU floor required to consider a current-frame detection as the
    /// same object as a tracked one. Frigate default: 0.2.
    iou_thresh: f32,
    /// Number of consecutive missed frames after which a track is
    /// retired. Frigate default: 5.
    max_misses: u32,
    /// Tracks indexed by object_id for O(1) updates.
    tracks: BTreeMap<u64, TrackedObject>,
    /// Monotonically-increasing object_id source.
    next_id: u64,
}

impl Default for IouTracker {
    fn default() -> Self {
        Self::new(0.2, 5)
    }
}

/// Transition that happened to a single object during `update()`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackTransition {
    /// Object first appeared this frame.
    New(u64),
    /// Object matched an existing detection this frame.
    Updated(u64),
    /// Object's miss-counter exceeded `max_misses`; track retired.
    Ended(u64),
}

impl IouTracker {
    /// New tracker.
    #[must_use]
    pub fn new(iou_thresh: f32, max_misses: u32) -> Self {
        Self {
            iou_thresh,
            max_misses,
            tracks: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// All currently-alive tracks, ordered by object_id.
    #[must_use]
    pub fn live(&self) -> Vec<TrackedObject> {
        self.tracks.values().cloned().collect()
    }

    /// Update with the detections from the next frame; returns the list
    /// of transitions (new / updated / ended) in stable order:
    /// `New` first, then `Updated`, then `Ended`.
    pub fn update(&mut self, mut detections: Vec<Detection>) -> Vec<TrackTransition> {
        // 1) Greedy match: for each existing track, pick the
        //    highest-IoU same-label detection above the threshold.
        let mut transitions: Vec<TrackTransition> = Vec::new();
        let mut matched_detection: Vec<bool> = vec![false; detections.len()];
        let mut matched_track_id: Vec<u64> = Vec::new();

        // Sort tracks by descending hits so the most stable ones bind
        // first — mirrors Frigate's `sorted(tracks, key=hits)` ordering.
        let mut track_ids: Vec<u64> = self.tracks.keys().copied().collect();
        track_ids.sort_by_key(|id| {
            std::cmp::Reverse(self.tracks.get(id).map(|t| t.hits).unwrap_or(0))
        });

        for tid in track_ids {
            let prev = match self.tracks.get(&tid) {
                Some(t) => t.detection.clone(),
                None => continue,
            };
            let mut best_i: Option<usize> = None;
            let mut best_iou = self.iou_thresh;
            for (i, d) in detections.iter().enumerate() {
                if matched_detection[i] || d.label != prev.label {
                    continue;
                }
                let iou = d.iou(&prev);
                if iou >= best_iou {
                    best_iou = iou;
                    best_i = Some(i);
                }
            }
            if let Some(i) = best_i {
                matched_detection[i] = true;
                matched_track_id.push(tid);
                let det = detections[i].clone();
                if let Some(t) = self.tracks.get_mut(&tid) {
                    t.hits = t.hits.saturating_add(1);
                    t.misses = 0;
                    t.detection = det;
                }
                transitions.push(TrackTransition::Updated(tid));
            }
        }

        // 2) Unmatched tracks bump their miss counter.
        let mut to_retire: Vec<u64> = Vec::new();
        for (tid, t) in self.tracks.iter_mut() {
            if matched_track_id.contains(tid) {
                continue;
            }
            t.misses = t.misses.saturating_add(1);
            if t.misses > self.max_misses {
                to_retire.push(*tid);
            }
        }
        for tid in to_retire {
            self.tracks.remove(&tid);
            transitions.push(TrackTransition::Ended(tid));
        }

        // 3) Unmatched detections spawn new tracks. Drain in order so the
        //    object_id assignment is deterministic relative to detection
        //    input order.
        for i in 0..detections.len() {
            if matched_detection[i] {
                continue;
            }
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            let det = std::mem::replace(
                &mut detections[i],
                Detection {
                    label: String::new(),
                    score: 0.0,
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
            );
            self.tracks.insert(
                id,
                TrackedObject {
                    object_id: id,
                    detection: det,
                    hits: 1,
                    misses: 0,
                },
            );
            transitions.insert(0, TrackTransition::New(id));
        }

        // Stabilise the transition order: New, then Updated, then Ended.
        transitions.sort_by_key(|t| match t {
            TrackTransition::New(_) => 0,
            TrackTransition::Updated(_) => 1,
            TrackTransition::Ended(_) => 2,
        });

        transitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(label: &str, score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        Detection {
            label: label.into(),
            score,
            x,
            y,
            w,
            h,
        }
    }

    #[test]
    fn first_frame_creates_one_track_per_detection() {
        let mut t = IouTracker::default();
        let trans = t.update(vec![det("person", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        assert!(matches!(trans.as_slice(), [TrackTransition::New(_)]));
        assert_eq!(t.live().len(), 1);
    }

    #[test]
    fn overlapping_same_label_detection_keeps_object_id() {
        let mut t = IouTracker::default();
        let trans1 = t.update(vec![det("person", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        let id = match trans1.first() {
            Some(TrackTransition::New(id)) => *id,
            other => panic!("expected New, got {other:?}"),
        };
        let trans2 = t.update(vec![det("person", 0.92, 0.11, 0.1, 0.2, 0.2)]);
        assert!(matches!(trans2.as_slice(), [TrackTransition::Updated(updated_id)] if *updated_id == id));
        assert_eq!(t.live().first().expect("alive").hits, 2);
    }

    #[test]
    fn label_mismatch_does_not_match() {
        let mut t = IouTracker::default();
        t.update(vec![det("person", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        let trans = t.update(vec![det("car", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        // person track wasn't matched -> +1 miss. car -> new.
        assert!(matches!(trans.as_slice(), [TrackTransition::New(_)]));
        // both alive (person is at miss=1, car at miss=0).
        assert_eq!(t.live().len(), 2);
    }

    #[test]
    fn missing_more_than_max_misses_retires_track() {
        let mut t = IouTracker::new(0.2, 2);
        t.update(vec![det("person", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        // 3 empty frames in a row -> retire after the 3rd (miss = 3 > max=2).
        t.update(vec![]);
        t.update(vec![]);
        let trans = t.update(vec![]);
        assert!(trans.iter().any(|x| matches!(x, TrackTransition::Ended(_))));
        assert!(t.live().is_empty());
    }

    #[test]
    fn low_iou_overlap_does_not_match() {
        let mut t = IouTracker::new(0.5, 5);
        t.update(vec![det("person", 0.9, 0.0, 0.0, 0.1, 0.1)]);
        let trans = t.update(vec![det("person", 0.9, 0.5, 0.5, 0.1, 0.1)]);
        // No match -> the second detection spawns a new track; the old
        // one is missed.
        let news = trans
            .iter()
            .filter(|x| matches!(x, TrackTransition::New(_)))
            .count();
        let updates = trans
            .iter()
            .filter(|x| matches!(x, TrackTransition::Updated(_)))
            .count();
        assert_eq!(news, 1);
        assert_eq!(updates, 0);
    }

    #[test]
    fn object_id_is_stable_across_many_frames() {
        let mut t = IouTracker::default();
        let trans1 = t.update(vec![det("person", 0.9, 0.1, 0.1, 0.2, 0.2)]);
        let id = match trans1.first() {
            Some(TrackTransition::New(id)) => *id,
            other => panic!("expected New, got {other:?}"),
        };
        for k in 0..10 {
            let x = 0.1 + 0.005 * f32::from(u8::try_from(k).unwrap_or(0));
            t.update(vec![det("person", 0.9, x, 0.1, 0.2, 0.2)]);
        }
        assert_eq!(t.live().first().expect("alive").object_id, id);
    }
}
