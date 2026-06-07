// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Wake-word spotting by dynamic-time-warping a live log-mel feature sequence
//! against enrolled keyword templates.
//!
//! This is the openWakeWord *approach* — log-mel features + a learned template,
//! matched in a loudness- and tempo-robust way — implemented first-party with a
//! classic DTW matcher instead of a neural net, so it needs no model weights and
//! runs anywhere. A household enrolls a keyword once (one spoken clip → a
//! [`WakeTemplate`]); thereafter [`WakeWordDetector::detect`] reports whether a
//! buffered window contains it and how confident the match is.

use crate::features::{FeatureConfig, FeatureExtractor};

/// L2-normalise a feature vector in place-free fashion, returning a copy. A
/// zero vector is returned unchanged (no division by zero). Normalising makes
/// the DTW local cost loudness-invariant: a quiet "Jarvis" matches a loud one.
#[must_use]
fn l2_normalise(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// Squared Euclidean distance between two equal-length vectors.
#[must_use]
fn sq_euclidean(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y) * (x - y))
        .sum()
}

/// Length-normalised dynamic-time-warping distance between two feature
/// sequences.
///
/// Each frame is L2-normalised first; the local cost is Euclidean distance; the
/// accumulated optimal-path cost is divided by the template length so keywords
/// of different durations are comparable. Returns [`f32::INFINITY`] if either
/// sequence is empty.
#[must_use]
pub fn dtw_distance(query: &[Vec<f32>], template: &[Vec<f32>]) -> f32 {
    if query.is_empty() || template.is_empty() {
        return f32::INFINITY;
    }
    let q: Vec<Vec<f32>> = query.iter().map(|v| l2_normalise(v)).collect();
    let t: Vec<Vec<f32>> = template.iter().map(|v| l2_normalise(v)).collect();
    let n = q.len();
    let m = t.len();
    // Rolling two-row DP over the (n+1) x (m+1) accumulated-cost matrix.
    let mut prev = vec![f32::INFINITY; m + 1];
    let mut curr = vec![f32::INFINITY; m + 1];
    prev[0] = 0.0;
    for i in 1..=n {
        curr[0] = f32::INFINITY;
        for j in 1..=m {
            let cost = sq_euclidean(&q[i - 1], &t[j - 1]).sqrt();
            let best = prev[j].min(curr[j - 1]).min(prev[j - 1]);
            curr[j] = cost + best;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    #[allow(clippy::cast_precision_loss)]
    let norm = m as f32;
    prev[m] / norm
}

/// An enrolled keyword: a label plus the log-mel feature sequence of one spoken
/// reference clip.
#[derive(Debug, Clone)]
pub struct WakeTemplate {
    /// The wake word, e.g. "jarvis".
    pub keyword: String,
    /// The reference feature sequence.
    pub frames: Vec<Vec<f32>>,
}

/// A positive wake-word match.
#[derive(Debug, Clone, PartialEq)]
pub struct WakeDetection {
    /// Which keyword fired.
    pub keyword: String,
    /// The DTW distance (lower = better).
    pub distance: f32,
    /// Confidence in `[0,1]`, derived from how far under threshold the match is.
    pub confidence: f32,
}

/// Wake-detector tuning.
#[derive(Debug, Clone)]
pub struct WakeConfig {
    /// Maximum DTW distance to accept a match.
    pub threshold: f32,
    /// Feature front-end configuration (must match enrollment).
    pub features: FeatureConfig,
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            threshold: 0.55,
            features: FeatureConfig::default(),
        }
    }
}

/// A multi-keyword DTW wake-word detector.
#[derive(Debug)]
pub struct WakeWordDetector {
    extractor: FeatureExtractor,
    templates: Vec<WakeTemplate>,
    threshold: f32,
}

impl WakeWordDetector {
    /// Build a detector for the given configuration with no keywords yet.
    #[must_use]
    pub fn new(config: WakeConfig) -> Self {
        Self {
            extractor: FeatureExtractor::new(config.features),
            templates: Vec::new(),
            threshold: config.threshold,
        }
    }

    /// Enroll a keyword from a raw reference clip (normalised `f32` samples).
    pub fn enroll(&mut self, keyword: impl Into<String>, reference: &[f32]) {
        let frames = self.extractor.extract(reference);
        self.templates.push(WakeTemplate {
            keyword: keyword.into(),
            frames,
        });
    }

    /// Number of enrolled keywords.
    #[must_use]
    pub fn keyword_count(&self) -> usize {
        self.templates.len()
    }

    /// Test a buffered audio window against every keyword, returning the best
    /// match under threshold (if any).
    #[must_use]
    pub fn detect(&self, window: &[f32]) -> Option<WakeDetection> {
        let frames = self.extractor.extract(window);
        if frames.is_empty() {
            return None;
        }
        let mut best: Option<WakeDetection> = None;
        for tpl in &self.templates {
            let distance = dtw_distance(&frames, &tpl.frames);
            if distance <= self.threshold {
                let confidence = (1.0 - distance / self.threshold).clamp(0.0, 1.0);
                let candidate = WakeDetection {
                    keyword: tpl.keyword.clone(),
                    distance,
                    confidence,
                };
                if best.as_ref().is_none_or(|b| distance < b.distance) {
                    best = Some(candidate);
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16_000;

    /// A tone burst of `freq` Hz for `n` samples at amplitude `amp`.
    fn tone(freq: f32, n: usize, amp: f32) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        let srf = SR as f32;
        (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / srf;
                amp * (2.0 * PI * freq * t).sin()
            })
            .collect()
    }

    /// A synthetic "keyword": three distinct tone segments in a fixed order —
    /// a stand-in for the formant trajectory of a spoken word.
    fn keyword_signal(amp: f32) -> Vec<f32> {
        let mut s = Vec::new();
        s.extend(tone(450.0, 1600, amp));
        s.extend(tone(900.0, 1600, amp));
        s.extend(tone(1600.0, 1600, amp));
        s
    }

    /// A different word: the same tones in a different order.
    fn other_signal(amp: f32) -> Vec<f32> {
        let mut s = Vec::new();
        s.extend(tone(1600.0, 1600, amp));
        s.extend(tone(300.0, 1600, amp));
        s.extend(tone(1600.0, 1600, amp));
        s
    }

    #[test]
    fn dtw_distance_is_zero_for_identical_sequences() {
        let a = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        assert!(dtw_distance(&a, &a) < 1e-5);
    }

    #[test]
    fn dtw_distance_empty_is_infinite() {
        assert_eq!(dtw_distance(&[], &[vec![1.0]]), f32::INFINITY);
    }

    #[test]
    fn dtw_is_loudness_invariant() {
        // Same direction vectors, different magnitude -> zero after L2 norm.
        let a = vec![vec![1.0, 2.0, 3.0]];
        let b = vec![vec![10.0, 20.0, 30.0]];
        assert!(dtw_distance(&a, &b) < 1e-4);
    }

    #[test]
    fn detector_fires_on_enrolled_keyword() {
        let mut det = WakeWordDetector::new(WakeConfig::default());
        det.enroll("jarvis", &keyword_signal(0.8));
        assert_eq!(det.keyword_count(), 1);
        // The same word, spoken more quietly, should still fire.
        let hit = det.detect(&keyword_signal(0.3));
        assert!(hit.is_some(), "expected a wake detection");
        let d = hit.unwrap();
        assert_eq!(d.keyword, "jarvis");
        assert!(d.confidence > 0.0 && d.confidence <= 1.0);
    }

    #[test]
    fn detector_rejects_a_different_word() {
        let mut det = WakeWordDetector::new(WakeConfig::default());
        det.enroll("jarvis", &keyword_signal(0.8));
        assert!(det.detect(&other_signal(0.8)).is_none(), "false wake");
    }

    #[test]
    fn detector_picks_closest_of_several_keywords() {
        let mut det = WakeWordDetector::new(WakeConfig::default());
        det.enroll("jarvis", &keyword_signal(0.8));
        det.enroll("computer", &other_signal(0.8));
        let hit = det.detect(&other_signal(0.5)).expect("a match");
        assert_eq!(hit.keyword, "computer");
    }
}
