// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The acoustic feature front-end: pre-emphasis → framing → window → DFT power
//! spectrum → mel filterbank → log-mel feature vectors.
//!
//! These are the same log-mel features wake-word spotters (openWakeWord-class)
//! and speaker-identification embeddings are built on. The implementation is
//! first-party and `std`-only — a direct DFT (no FFT crate), triangular mel
//! filters, natural-log compression — small enough that the per-frame `O(N²)`
//! transform is irrelevant for 256-sample frames yet exact enough for matching.

use std::f32::consts::PI;

/// Convert a linear frequency in Hz to the mel scale (O'Shaughnessy 1987).
#[must_use]
pub fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Convert a mel value back to linear Hz.
#[must_use]
pub fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

/// First-difference pre-emphasis: `y[n] = x[n] - coeff*x[n-1]`. Boosts the high
/// frequencies speech energy concentrates in. `coeff` is typically `0.97`.
#[must_use]
pub fn pre_emphasis(samples: &[f32], coeff: f32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(samples.len());
    out.push(samples[0]);
    for i in 1..samples.len() {
        out.push(coeff.mul_add(-samples[i - 1], samples[i]));
    }
    out
}

/// A periodic Hann window of length `n`.
#[must_use]
pub fn hann_window(n: usize) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }
    #[allow(clippy::cast_precision_loss)]
    let denom = n as f32;
    (0..n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let x = i as f32;
            0.5f32.mul_add(-(2.0 * PI * x / denom).cos(), 0.5)
        })
        .collect()
}

/// Split a signal into overlapping frames advancing by `hop`.
///
/// Each frame is `frame_len` samples. Trailing samples that cannot fill a frame
/// are dropped (standard short-time analysis). Returns an empty vec if the
/// signal is too short.
#[must_use]
pub fn frame_signal(samples: &[f32], frame_len: usize, hop: usize) -> Vec<Vec<f32>> {
    if frame_len == 0 || hop == 0 || samples.len() < frame_len {
        return Vec::new();
    }
    let mut frames = Vec::new();
    let mut start = 0;
    while start + frame_len <= samples.len() {
        frames.push(samples[start..start + frame_len].to_vec());
        start += hop;
    }
    frames
}

/// The single-sided power spectrum of a frame via a direct DFT.
///
/// Returns `frame.len()/2 + 1` bins of `|X(k)|²`. A direct transform is `O(N²)`
/// but for the 256-sample frames used here that is a few thousand operations —
/// negligible, and it avoids pulling in an FFT dependency (Charter §9).
#[must_use]
pub fn power_spectrum(frame: &[f32]) -> Vec<f32> {
    let n = frame.len();
    if n == 0 {
        return Vec::new();
    }
    let bins = n / 2 + 1;
    let mut out = Vec::with_capacity(bins);
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f32;
    for k in 0..bins {
        let mut re = 0.0_f32;
        let mut im = 0.0_f32;
        #[allow(clippy::cast_precision_loss)]
        let kf = k as f32;
        for (t, &x) in frame.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let tf = t as f32;
            let angle = -2.0 * PI * kf * tf / nf;
            re = x.mul_add(angle.cos(), re);
            im = x.mul_add(angle.sin(), im);
        }
        out.push(im.mul_add(im, re * re));
    }
    out
}

/// A bank of triangular mel filters mapping a linear power spectrum to mel-band
/// energies.
#[derive(Debug, Clone)]
pub struct MelFilterBank {
    /// One weight row per mel band, each `n_fft/2 + 1` long.
    filters: Vec<Vec<f32>>,
}

impl MelFilterBank {
    /// Build `n_mels` triangular filters spanning `[fmin, fmax]` over an
    /// `n_fft`-point spectrum at `sample_rate`.
    #[must_use]
    pub fn new(n_fft: usize, n_mels: usize, sample_rate: u32, fmin: f32, fmax: f32) -> Self {
        let bins = n_fft / 2 + 1;
        if n_mels == 0 || bins == 0 {
            return Self { filters: Vec::new() };
        }
        #[allow(clippy::cast_precision_loss)]
        let sr = sample_rate as f32;
        let mel_min = hz_to_mel(fmin);
        let mel_max = hz_to_mel(fmax);
        // n_mels+2 equally spaced mel points -> n_mels triangles.
        let points: Vec<f32> = (0..n_mels + 2)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let frac = i as f32 / (n_mels + 1) as f32;
                let mel = mel_min + frac * (mel_max - mel_min);
                mel_to_hz(mel)
            })
            .collect();
        // Map each hz point to the nearest FFT bin index.
        let bin_of = |hz: f32| -> f32 {
            #[allow(clippy::cast_precision_loss)]
            let b = (n_fft as f32) * hz / sr;
            b
        };
        let mut filters = Vec::with_capacity(n_mels);
        for m in 1..=n_mels {
            let left = bin_of(points[m - 1]);
            let center = bin_of(points[m]);
            let right = bin_of(points[m + 1]);
            let mut row = vec![0.0_f32; bins];
            for (k, w) in row.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let kf = k as f32;
                if kf >= left && kf <= center && (center - left) > 0.0 {
                    *w = (kf - left) / (center - left);
                } else if kf > center && kf <= right && (right - center) > 0.0 {
                    *w = (right - kf) / (right - center);
                }
            }
            filters.push(row);
        }
        Self { filters }
    }

    /// Number of mel bands.
    #[must_use]
    pub fn n_mels(&self) -> usize {
        self.filters.len()
    }

    /// Apply the bank to a power spectrum, yielding one energy per mel band.
    #[must_use]
    pub fn apply(&self, power: &[f32]) -> Vec<f32> {
        self.filters
            .iter()
            .map(|row| {
                row.iter()
                    .zip(power.iter())
                    .map(|(w, p)| w * p)
                    .sum::<f32>()
            })
            .collect()
    }
}

/// Configuration for the log-mel feature extractor.
#[derive(Debug, Clone)]
pub struct FeatureConfig {
    /// Samples per analysis frame.
    pub frame_len: usize,
    /// Samples between frame starts.
    pub hop: usize,
    /// Number of mel bands.
    pub n_mels: usize,
    /// Pre-emphasis coefficient.
    pub pre_emphasis: f32,
    /// Sample rate of the incoming signal.
    pub sample_rate: u32,
    /// Low edge of the mel range, Hz.
    pub fmin: f32,
    /// High edge of the mel range, Hz.
    pub fmax: f32,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            frame_len: 256,
            hop: 128,
            n_mels: 16,
            pre_emphasis: 0.97,
            sample_rate: crate::audio::SAMPLE_RATE_HZ,
            fmin: 80.0,
            fmax: 7600.0,
        }
    }
}

/// Turns a raw signal into a sequence of log-mel feature vectors — the common
/// representation the wake matcher and speaker embedder both consume.
#[derive(Debug, Clone)]
pub struct FeatureExtractor {
    config: FeatureConfig,
    window: Vec<f32>,
    bank: MelFilterBank,
}

impl FeatureExtractor {
    /// Build an extractor for the given configuration.
    #[must_use]
    pub fn new(config: FeatureConfig) -> Self {
        let window = hann_window(config.frame_len);
        let bank = MelFilterBank::new(
            config.frame_len,
            config.n_mels,
            config.sample_rate,
            config.fmin,
            config.fmax,
        );
        Self {
            config,
            window,
            bank,
        }
    }

    /// The number of mel bands each frame produces.
    #[must_use]
    pub fn n_mels(&self) -> usize {
        self.bank.n_mels()
    }

    /// Extract a log-mel feature vector for every frame in `samples`.
    #[must_use]
    pub fn extract(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        let emph = pre_emphasis(samples, self.config.pre_emphasis);
        frame_signal(&emph, self.config.frame_len, self.config.hop)
            .into_iter()
            .map(|mut frame| {
                for (s, w) in frame.iter_mut().zip(self.window.iter()) {
                    *s *= w;
                }
                let power = power_spectrum(&frame);
                self.bank
                    .apply(&power)
                    .into_iter()
                    // log(1+x): compresses dynamic range, never returns -inf.
                    .map(f32::ln_1p)
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: u32, n: usize, amp: f32) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        let srf = sr as f32;
        (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / srf;
                amp * (2.0 * PI * freq * t).sin()
            })
            .collect()
    }

    #[test]
    fn mel_round_trips() {
        for hz in [100.0, 1000.0, 4000.0] {
            let back = mel_to_hz(hz_to_mel(hz));
            assert!((back - hz).abs() < 1.0, "{hz} -> {back}");
        }
    }

    #[test]
    fn hann_window_starts_at_zero_peaks_mid_and_mirrors() {
        let w = hann_window(8);
        assert_eq!(w.len(), 8);
        assert!(w[0] < 0.01);
        // Periodic Hann mirrors about n/2: w[i] == w[n-i] for i in 1..n.
        for i in 1..8 {
            assert!((w[i] - w[8 - i]).abs() < 1e-5, "w[{i}] != w[{}]", 8 - i);
        }
        let max = w.iter().cloned().fold(0.0_f32, f32::max);
        assert!(max > 0.9);
    }

    #[test]
    fn framing_overlaps_correctly() {
        let sig: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let frames = frame_signal(&sig, 4, 2);
        assert_eq!(frames.len(), 4); // starts at 0,2,4,6
        assert_eq!(frames[0], vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(frames[1], vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn framing_too_short_is_empty() {
        assert!(frame_signal(&[1.0, 2.0], 4, 2).is_empty());
    }

    #[test]
    fn power_spectrum_peaks_at_input_frequency() {
        let n = 256;
        let sr = 16_000;
        // A pure tone at bin 20 -> freq = 20*sr/n.
        #[allow(clippy::cast_precision_loss)]
        let freq = 20.0 * sr as f32 / n as f32;
        let frame = sine(freq, sr, n, 1.0);
        let ps = power_spectrum(&frame);
        let (peak_bin, _) = ps
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!((peak_bin as i32 - 20).abs() <= 1, "peak at {peak_bin}");
    }

    #[test]
    fn pre_emphasis_removes_dc() {
        // A constant signal has all energy at DC; pre-emphasis should flatten it.
        let dc = vec![1.0_f32; 64];
        let emph = pre_emphasis(&dc, 0.97);
        // After the first sample, every value is 1 - 0.97 = 0.03.
        assert!((emph[10] - 0.03).abs() < 1e-5);
    }

    #[test]
    fn mel_bank_has_requested_band_count() {
        let bank = MelFilterBank::new(256, 16, 16_000, 80.0, 7600.0);
        assert_eq!(bank.n_mels(), 16);
        // Each filter row spans the half-spectrum + 1.
        let energies = bank.apply(&vec![1.0; 129]);
        assert_eq!(energies.len(), 16);
    }

    #[test]
    fn extractor_produces_one_vector_per_frame() {
        let cfg = FeatureConfig::default();
        let extractor = FeatureExtractor::new(cfg.clone());
        let sig = sine(440.0, cfg.sample_rate, 1024, 0.5);
        let feats = extractor.extract(&sig);
        // (1024 - 256)/128 + 1 = 7 frames.
        assert_eq!(feats.len(), 7);
        assert_eq!(feats[0].len(), cfg.n_mels);
        // Real speech-band energy => some band is non-trivially positive.
        assert!(feats[0].iter().cloned().fold(0.0_f32, f32::max) > 0.0);
    }
}
