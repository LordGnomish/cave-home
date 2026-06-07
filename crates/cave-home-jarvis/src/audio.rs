// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Audio capture model: PCM frames, the injectable [`AudioSource`] seam, and an
//! energy-gated voice-activity detector (VAD).
//!
//! Microphones are hardware; the real ALSA/CPAL capture backend is the only
//! deferred piece (Phase-1b). Everything in this crate consumes the
//! [`AudioSource`] trait and is exercised through [`MockAudioSource`], which
//! replays a scripted sequence of frames exactly as a real microphone would.

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::{JarvisError, Result};

/// The canonical capture format: 16 kHz mono signed-16-bit PCM. This matches
/// what the wake-word matcher and whisper-class STT both expect, so the pipeline
/// never resamples mid-stream.
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// A block of mono PCM samples tagged with the device it came from.
///
/// Samples are normalised `f32` in `[-1.0, 1.0]` so the feature front-end never
/// has to know the source's bit depth.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// The logical capture device (a microphone in some room).
    pub device: String,
    /// Mono samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// The sample rate in Hz.
    pub sample_rate: u32,
}

impl AudioFrame {
    /// Build a frame from normalised `f32` samples at [`SAMPLE_RATE_HZ`].
    #[must_use]
    pub fn new(device: impl Into<String>, samples: Vec<f32>) -> Self {
        Self {
            device: device.into(),
            samples,
            sample_rate: SAMPLE_RATE_HZ,
        }
    }

    /// Build a frame from raw signed-16-bit PCM (what a sound card hands over),
    /// converting to the normalised `f32` the pipeline uses.
    #[must_use]
    pub fn from_pcm_i16(device: impl Into<String>, pcm: &[i16], sample_rate: u32) -> Self {
        let samples = pcm
            .iter()
            .map(|&s| f32::from(s) / f32::from(i16::MAX))
            .collect();
        Self {
            device: device.into(),
            samples,
            sample_rate,
        }
    }

    /// Number of samples in the frame.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the frame carries no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Root-mean-square energy of the frame in `[0.0, 1.0]` — the loudness
    /// signal the VAD and wake gate threshold against.
    #[must_use]
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = self.samples.iter().map(|s| s * s).sum();
        #[allow(clippy::cast_precision_loss)]
        let mean = sum_sq / self.samples.len() as f32;
        mean.sqrt()
    }

    /// Zero-crossing rate in `[0.0, 1.0]` — high for fricatives / noise, low for
    /// voiced speech; a cheap second feature the VAD uses to reject hiss.
    #[must_use]
    pub fn zero_crossing_rate(&self) -> f32 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let crossings = self
            .samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        #[allow(clippy::cast_precision_loss)]
        let rate = crossings as f32 / (self.samples.len() - 1) as f32;
        rate
    }
}

/// A pluggable source of audio frames. The production ALSA/CPAL capture backend
/// is Phase-1b; the crate is tested against [`MockAudioSource`].
#[async_trait]
pub trait AudioSource: Send + Sync {
    /// Pull the next frame, or [`JarvisError::AudioEnded`] when the stream is
    /// over (the mock empties, the device closes).
    ///
    /// # Errors
    /// [`JarvisError::AudioEnded`] at end-of-stream; format/hardware errors
    /// otherwise.
    async fn next_frame(&self) -> Result<AudioFrame>;
}

/// A scripted in-memory audio source for tests and the integration suite.
///
/// Frames are dequeued FIFO; once empty, `next_frame` returns
/// [`JarvisError::AudioEnded`], exactly mirroring a closed device.
#[derive(Debug, Default)]
pub struct MockAudioSource {
    frames: Mutex<std::collections::VecDeque<AudioFrame>>,
}

impl MockAudioSource {
    /// An empty source (immediately ended).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a source from a frame list.
    #[must_use]
    pub fn from_frames(frames: impl IntoIterator<Item = AudioFrame>) -> Self {
        Self {
            frames: Mutex::new(frames.into_iter().collect()),
        }
    }

    /// Append a frame to the back of the queue.
    pub fn push(&self, frame: AudioFrame) {
        self.frames.lock().push_back(frame);
    }

    /// How many frames remain.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.frames.lock().len()
    }
}

#[async_trait]
impl AudioSource for MockAudioSource {
    async fn next_frame(&self) -> Result<AudioFrame> {
        self.frames
            .lock()
            .pop_front()
            .ok_or(JarvisError::AudioEnded)
    }
}

/// Configuration for the energy-gated voice-activity detector.
#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// RMS above which a frame is "speech-loud".
    pub energy_threshold: f32,
    /// Maximum zero-crossing rate for a frame to count as voiced (rejects hiss).
    pub max_zcr: f32,
    /// How many consecutive non-speech frames end an utterance (hangover).
    pub hangover_frames: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            energy_threshold: 0.02,
            max_zcr: 0.35,
            hangover_frames: 5,
        }
    }
}

/// Where the VAD currently sits in an utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    /// No speech yet / between utterances.
    Silence,
    /// Inside an utterance.
    Speech,
}

/// The transition a single frame produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    /// Still silent.
    Idle,
    /// Speech just began on this frame.
    SpeechStart,
    /// Still inside speech.
    SpeechContinue,
    /// Speech just ended (hangover elapsed) on this frame.
    SpeechEnd,
}

/// A streaming energy + zero-crossing voice-activity detector with hangover.
///
/// Feed it frames in order; it emits [`VadEvent`]s so the pipeline knows when an
/// utterance starts and ends without buffering the whole stream.
#[derive(Debug)]
pub struct Vad {
    config: VadConfig,
    state: VadState,
    silence_run: u32,
}

impl Vad {
    /// A detector with the given thresholds.
    #[must_use]
    pub const fn new(config: VadConfig) -> Self {
        Self {
            config,
            state: VadState::Silence,
            silence_run: 0,
        }
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> VadState {
        self.state
    }

    /// Is a single frame loud-and-voiced enough to be speech?
    #[must_use]
    pub fn frame_is_speech(&self, frame: &AudioFrame) -> bool {
        frame.rms() >= self.config.energy_threshold
            && frame.zero_crossing_rate() <= self.config.max_zcr
    }

    /// Advance the detector by one frame, returning the transition it caused.
    pub fn observe(&mut self, frame: &AudioFrame) -> VadEvent {
        let voiced = self.frame_is_speech(frame);
        match self.state {
            VadState::Silence => {
                if voiced {
                    self.state = VadState::Speech;
                    self.silence_run = 0;
                    VadEvent::SpeechStart
                } else {
                    VadEvent::Idle
                }
            }
            VadState::Speech => {
                if voiced {
                    self.silence_run = 0;
                    VadEvent::SpeechContinue
                } else {
                    self.silence_run += 1;
                    if self.silence_run >= self.config.hangover_frames {
                        self.state = VadState::Silence;
                        self.silence_run = 0;
                        VadEvent::SpeechEnd
                    } else {
                        VadEvent::SpeechContinue
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine-ish voiced frame at a given amplitude (low zero-crossing rate).
    fn voiced_frame(device: &str, amp: f32, n: usize) -> AudioFrame {
        // A slow triangle wave: loud, but crosses zero only twice per period.
        let period = 64.0;
        let samples = (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let phase = (i as f32 % period) / period; // 0..1
                let tri = if phase < 0.5 {
                    4.0 * phase - 1.0
                } else {
                    3.0 - 4.0 * phase
                };
                tri * amp
            })
            .collect();
        AudioFrame::new(device, samples)
    }

    fn silent_frame(device: &str, n: usize) -> AudioFrame {
        AudioFrame::new(device, vec![0.0; n])
    }

    #[test]
    fn pcm_i16_round_trips_to_normalised() {
        let f = AudioFrame::from_pcm_i16("mic", &[i16::MAX, 0, i16::MIN], SAMPLE_RATE_HZ);
        assert!((f.samples[0] - 1.0).abs() < 1e-3);
        assert!(f.samples[1].abs() < 1e-6);
        assert!((f.samples[2] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(silent_frame("mic", 100).rms(), 0.0);
    }

    #[test]
    fn rms_of_loud_exceeds_quiet() {
        let loud = voiced_frame("mic", 0.5, 256).rms();
        let quiet = voiced_frame("mic", 0.05, 256).rms();
        assert!(loud > quiet);
    }

    #[test]
    fn zero_crossing_rate_high_for_alternating() {
        let alt: Vec<f32> = (0..100).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
        let f = AudioFrame::new("mic", alt);
        assert!(f.zero_crossing_rate() > 0.9);
    }

    #[tokio::test]
    async fn mock_source_replays_then_ends() {
        let src = MockAudioSource::from_frames([silent_frame("mic", 4), silent_frame("mic", 4)]);
        assert_eq!(src.remaining(), 2);
        assert!(src.next_frame().await.is_ok());
        assert!(src.next_frame().await.is_ok());
        assert_eq!(src.next_frame().await.unwrap_err(), JarvisError::AudioEnded);
    }

    #[test]
    fn vad_emits_start_continue_end_with_hangover() {
        let mut vad = Vad::new(VadConfig {
            energy_threshold: 0.05,
            max_zcr: 0.35,
            hangover_frames: 3,
        });
        // Silence -> idle.
        assert_eq!(vad.observe(&silent_frame("mic", 256)), VadEvent::Idle);
        // First loud frame starts speech.
        assert_eq!(vad.observe(&voiced_frame("mic", 0.6, 256)), VadEvent::SpeechStart);
        assert_eq!(vad.state(), VadState::Speech);
        // More speech continues.
        assert_eq!(vad.observe(&voiced_frame("mic", 0.6, 256)), VadEvent::SpeechContinue);
        // Two silent frames: still within hangover.
        assert_eq!(vad.observe(&silent_frame("mic", 256)), VadEvent::SpeechContinue);
        assert_eq!(vad.observe(&silent_frame("mic", 256)), VadEvent::SpeechContinue);
        // Third silent frame trips hangover -> end.
        assert_eq!(vad.observe(&silent_frame("mic", 256)), VadEvent::SpeechEnd);
        assert_eq!(vad.state(), VadState::Silence);
    }

    #[test]
    fn vad_rejects_loud_hiss_by_zcr() {
        let vad = Vad::new(VadConfig::default());
        // Loud but alternating every sample => very high ZCR => not speech.
        let hiss: Vec<f32> = (0..256).map(|i| if i % 2 == 0 { 0.6 } else { -0.6 }).collect();
        assert!(!vad.frame_is_speech(&AudioFrame::new("mic", hiss)));
    }
}
