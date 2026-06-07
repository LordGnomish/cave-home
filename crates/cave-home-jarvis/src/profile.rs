// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Per-speaker voice profiles: recognise *who* is talking (Burak vs. Sanja) so
//! the assistant can personalise replies and gate household-only commands.
//!
//! This is the d-vector idea reduced to its essence: a profile is the mean of
//! the L2-normalised log-mel feature frames of an enrolled clip — a fixed-length
//! embedding of that voice's average spectral signature. Identification embeds
//! the live utterance the same way and picks the enrolled profile with the
//! highest cosine similarity (above a margin). First-party, `std`-only; the
//! production system would swap in a trained speaker encoder behind the same
//! embed/compare seam.

use crate::features::{FeatureConfig, FeatureExtractor};

/// L2-normalise a vector (a zero vector is returned unchanged).
fn l2_normalise(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// Cosine similarity of two equal-length, already-normalised-ish vectors.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na <= f32::EPSILON || nb <= f32::EPSILON {
        return 0.0;
    }
    dot / (na * nb)
}

/// An enrolled household member's voice signature.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceProfile {
    /// The member's name (e.g. "Burak").
    pub name: String,
    /// The mean L2-normalised log-mel embedding.
    pub embedding: Vec<f32>,
}

/// A speaker-identification result.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerMatch {
    /// The recognised member.
    pub name: String,
    /// Cosine similarity to that member's profile, in `[-1, 1]`.
    pub similarity: f32,
}

/// The household's enrolled voices and the identifier.
#[derive(Debug, Clone)]
pub struct SpeakerBook {
    extractor: FeatureExtractor,
    profiles: Vec<VoiceProfile>,
    /// Minimum cosine similarity to accept an identification.
    threshold: f32,
}

impl SpeakerBook {
    /// A book using the given feature front-end and acceptance threshold.
    #[must_use]
    pub fn new(features: FeatureConfig, threshold: f32) -> Self {
        Self {
            extractor: FeatureExtractor::new(features),
            profiles: Vec::new(),
            threshold,
        }
    }

    /// A book with sensible defaults (default features, 0.6 cosine threshold).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(FeatureConfig::default(), 0.6)
    }

    /// Embed a clip into a fixed-length speaker vector: the mean of its
    /// per-frame L2-normalised features, itself L2-normalised. Returns an empty
    /// vector if the clip is too short to frame.
    #[must_use]
    pub fn embed(&self, audio: &[f32]) -> Vec<f32> {
        let frames = self.extractor.extract(audio);
        if frames.is_empty() {
            return Vec::new();
        }
        let dim = frames[0].len();
        let mut acc = vec![0.0_f32; dim];
        for frame in &frames {
            let nf = l2_normalise(frame);
            for (a, x) in acc.iter_mut().zip(nf.iter()) {
                *a += x;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let count = frames.len() as f32;
        for a in &mut acc {
            *a /= count;
        }
        l2_normalise(&acc)
    }

    /// Enroll a member from a reference clip.
    pub fn enroll(&mut self, name: impl Into<String>, reference: &[f32]) {
        let embedding = self.embed(reference);
        self.profiles.push(VoiceProfile {
            name: name.into(),
            embedding,
        });
    }

    /// Number of enrolled members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether no one is enrolled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Score a clip against every enrolled profile (sorted best-first).
    #[must_use]
    pub fn score(&self, audio: &[f32]) -> Vec<SpeakerMatch> {
        let emb = self.embed(audio);
        let mut scores: Vec<SpeakerMatch> = self
            .profiles
            .iter()
            .map(|p| SpeakerMatch {
                name: p.name.clone(),
                similarity: cosine_similarity(&emb, &p.embedding),
            })
            .collect();
        scores.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scores
    }

    /// Identify the speaker of a clip: the best-scoring profile, if it clears
    /// the acceptance threshold.
    #[must_use]
    pub fn identify(&self, audio: &[f32]) -> Option<SpeakerMatch> {
        self.score(audio)
            .into_iter()
            .next()
            .filter(|m| m.similarity >= self.threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16_000;

    /// A multi-tone "voice" — a fixed set of formant-like tones, scaled by amp.
    fn voice(formants: &[f32], n: usize, amp: f32) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        let srf = SR as f32;
        (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / srf;
                let s: f32 = formants.iter().map(|f| (2.0 * PI * f * t).sin()).sum();
                amp * s / formants.len() as f32
            })
            .collect()
    }

    fn burak(n: usize, amp: f32) -> Vec<f32> {
        voice(&[150.0, 800.0, 2400.0], n, amp) // lower, darker
    }

    fn sanja(n: usize, amp: f32) -> Vec<f32> {
        voice(&[320.0, 1300.0, 3100.0], n, amp) // higher, brighter
    }

    #[test]
    fn cosine_of_identical_is_one() {
        let v = vec![0.3, 0.4, 0.5];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn embed_is_unit_length() {
        let book = SpeakerBook::with_defaults();
        let emb = book.embed(&burak(4096, 0.7));
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "embedding should be unit length");
    }

    #[test]
    fn identifies_the_right_household_member() {
        let mut book = SpeakerBook::with_defaults();
        book.enroll("Burak", &burak(8192, 0.8));
        book.enroll("Sanja", &sanja(8192, 0.8));
        assert_eq!(book.len(), 2);

        // A fresh Burak sample (different length + loudness) -> Burak.
        let hit = book.identify(&burak(6000, 0.4)).expect("identified");
        assert_eq!(hit.name, "Burak");

        // And Sanja's sample -> Sanja.
        assert_eq!(book.identify(&sanja(6000, 0.5)).unwrap().name, "Sanja");
    }

    #[test]
    fn correct_member_outscores_the_other() {
        let mut book = SpeakerBook::with_defaults();
        book.enroll("Burak", &burak(8192, 0.8));
        book.enroll("Sanja", &sanja(8192, 0.8));
        let scores = book.score(&burak(6000, 0.6));
        // Sorted best-first: Burak must beat Sanja.
        assert_eq!(scores[0].name, "Burak");
        assert!(scores[0].similarity > scores[1].similarity);
    }

    #[test]
    fn silence_identifies_no_one() {
        let mut book = SpeakerBook::with_defaults();
        book.enroll("Burak", &burak(8192, 0.8));
        assert!(book.identify(&vec![0.0; 6000]).is_none());
    }
}
