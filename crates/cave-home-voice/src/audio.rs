// SPDX-License-Identifier: Apache-2.0
//! Audio buffer + PCM helpers shared by STT / TTS / wake-word.
//!
//! # Upstream:
//! - `ggerganov/whisper.cpp@6ad0bb0:whisper.cpp::WHISPER_SAMPLE_RATE` —
//!   whisper's fixed 16 kHz mono assumption.
//! - `rhasspy/piper@23dee2e:src/cpp/piper.cpp::AudioConfig` — piper's
//!   per-voice sample rate (configured by the model JSON).
//! - `dscripka/openWakeWord@ed7f5b9:openwakeword/utils.py::AudioFeatures` —
//!   openWakeWord's chunking expectations (80 ms @ 16 kHz).

use std::io::{Cursor, Seek, Write};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

use crate::error::{VoiceError, VoiceResult};

/// Whisper's hard-coded sample rate.
///
/// # Upstream: `ggerganov/whisper.cpp@6ad0bb0:whisper.h::WHISPER_SAMPLE_RATE`
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Wake-word frame size (80 ms @ 16 kHz = 1280 samples).
///
/// # Upstream:
/// `dscripka/openWakeWord@ed7f5b9:openwakeword/utils.py::AudioFeatures.__init__`
/// — `self.melspec_window_size = 1280`.
pub const WAKE_FRAME_SAMPLES: usize = 1_280;

/// Mono 16-bit PCM frame.
///
/// All three engines (whisper, piper, openWakeWord) speak this shape.
/// piper emits it at the voice's native rate; whisper + openWakeWord
/// always require 16 kHz mono. The `sample_rate` field is part of the
/// struct so a re-sampler can verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmFrame {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl PcmFrame {
    /// Construct an empty mono frame at the given sample rate.
    #[must_use]
    pub const fn empty(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 1,
            samples: Vec::new(),
        }
    }

    /// Construct a mono frame from owned samples.
    #[must_use]
    pub const fn mono(sample_rate: u32, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            channels: 1,
            samples,
        }
    }

    /// Duration of the frame in seconds.
    #[must_use]
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }
        let total = self.samples.len() as f32;
        total / (f32::from(self.channels) * self.sample_rate as f32)
    }

    /// Convert to `f32` samples normalised to `[-1.0, 1.0]`.
    ///
    /// # Upstream:
    /// `ggerganov/whisper.cpp@6ad0bb0:examples/common.cpp::read_wav` — the
    /// example reader divides each `i16` sample by 32768 before feeding
    /// it to `whisper_full`.
    #[must_use]
    pub fn to_f32(&self) -> Vec<f32> {
        self.samples
            .iter()
            .map(|s| f32::from(*s) / 32_768.0)
            .collect()
    }

    /// Encode as a RIFF/WAVE byte vector.
    ///
    /// # Errors
    /// Returns [`VoiceError::Wav`] on hound encode failure.
    ///
    /// # Upstream:
    /// `rhasspy/piper@23dee2e:src/cpp/piper.cpp::write_wav` — piper's
    /// example writer emits exactly this header layout.
    pub fn to_wav_bytes(&self) -> VoiceResult<Vec<u8>> {
        let spec = WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec)?;
            for s in &self.samples {
                writer.write_sample(*s)?;
            }
            writer.finalize()?;
        }
        Ok(cursor.into_inner())
    }

    /// Decode a RIFF/WAVE byte slice into a [`PcmFrame`].
    ///
    /// # Errors
    /// Returns [`VoiceError::Wav`] when the WAV is not 16-bit PCM or
    /// is otherwise malformed.
    pub fn from_wav_bytes(bytes: &[u8]) -> VoiceResult<Self> {
        let cursor = Cursor::new(bytes);
        let mut reader = WavReader::new(cursor)?;
        let spec = reader.spec();
        if spec.bits_per_sample != 16 {
            return Err(VoiceError::Wav(format!(
                "expected 16-bit PCM, got {} bits",
                spec.bits_per_sample
            )));
        }
        let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
        let samples = samples?;
        Ok(Self {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            samples,
        })
    }

    /// Append the samples from `other` to `self`, panic-free.
    ///
    /// # Errors
    /// Returns [`VoiceError::Audio`] when sample rates or channel
    /// counts disagree — callers must re-sample first.
    pub fn extend(&mut self, other: &Self) -> VoiceResult<()> {
        if self.sample_rate != other.sample_rate || self.channels != other.channels {
            return Err(VoiceError::Audio(format!(
                "cannot concatenate {} Hz/{}ch onto {} Hz/{}ch",
                other.sample_rate, other.channels, self.sample_rate, self.channels
            )));
        }
        self.samples.extend_from_slice(&other.samples);
        Ok(())
    }
}

/// A streaming ring buffer of mono i16 samples — the FIFO the wake-word
/// detector reads 80 ms windows out of.
///
/// # Upstream:
/// `dscripka/openWakeWord@ed7f5b9:openwakeword/utils.py::AudioFeatures._streaming_features`
/// — the upstream class maintains a rolling buffer; we replicate that
/// here in Rust.
#[derive(Debug)]
pub struct AudioRing {
    sample_rate: u32,
    capacity: usize,
    buffer: Vec<i16>,
}

impl AudioRing {
    #[must_use]
    pub fn new(sample_rate: u32, capacity: usize) -> Self {
        Self {
            sample_rate,
            capacity,
            buffer: Vec::with_capacity(capacity),
        }
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Push samples into the ring; the oldest samples drop when capacity
    /// is exceeded.
    pub fn push(&mut self, samples: &[i16]) {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() > self.capacity {
            let drop = self.buffer.len() - self.capacity;
            self.buffer.drain(..drop);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Snapshot the current contents as a fresh [`PcmFrame`].
    #[must_use]
    pub fn snapshot(&self) -> PcmFrame {
        PcmFrame::mono(self.sample_rate, self.buffer.clone())
    }

    /// Drain at most `n` samples from the front of the ring.
    pub fn take(&mut self, n: usize) -> Vec<i16> {
        let n = n.min(self.buffer.len());
        self.buffer.drain(..n).collect()
    }
}

/// Convenience helper — turn an i16 sample slice into a writable WAV
/// container. Used by tests and the `cavehomectl voice speak` command.
///
/// # Errors
/// Returns [`VoiceError::Wav`] on hound encode failure.
pub fn pcm_to_wav<W: Write + Seek>(writer: W, frame: &PcmFrame) -> VoiceResult<()> {
    let spec = WavSpec {
        channels: frame.channels,
        sample_rate: frame.sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut wav = WavWriter::new(writer, spec)?;
    for s in &frame.samples {
        wav.write_sample(*s)?;
    }
    wav.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_mono_round_trips_through_wav() {
        let frame = PcmFrame::mono(WHISPER_SAMPLE_RATE, vec![0, 1, -1, 16_000, -16_000]);
        let bytes = frame.to_wav_bytes().expect("encode");
        let decoded = PcmFrame::from_wav_bytes(&bytes).expect("decode");
        assert_eq!(frame, decoded);
    }

    #[test]
    fn pcm_duration_is_samples_over_rate() {
        let frame = PcmFrame::mono(16_000, vec![0; 16_000]);
        assert!((frame.duration_secs() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pcm_to_f32_normalises_to_unit_range() {
        let frame = PcmFrame::mono(16_000, vec![0, i16::MAX, i16::MIN]);
        let f = frame.to_f32();
        assert_eq!(f[0], 0.0);
        assert!(f[1] > 0.99 && f[1] <= 1.0);
        assert!(f[2] < -0.99 && f[2] >= -1.0);
    }

    #[test]
    fn pcm_extend_rejects_mismatched_rates() {
        let mut a = PcmFrame::mono(16_000, vec![1, 2, 3]);
        let b = PcmFrame::mono(8_000, vec![4, 5, 6]);
        assert!(a.extend(&b).is_err());
    }

    #[test]
    fn pcm_extend_concatenates_compatible_frames() {
        let mut a = PcmFrame::mono(16_000, vec![1, 2, 3]);
        let b = PcmFrame::mono(16_000, vec![4, 5, 6]);
        a.extend(&b).expect("compat");
        assert_eq!(a.samples, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn audio_ring_drops_oldest_when_full() {
        let mut ring = AudioRing::new(WHISPER_SAMPLE_RATE, 4);
        ring.push(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(ring.len(), 4);
        assert_eq!(ring.snapshot().samples, vec![3, 4, 5, 6]);
    }

    #[test]
    fn audio_ring_take_drains_from_front() {
        let mut ring = AudioRing::new(WHISPER_SAMPLE_RATE, 8);
        ring.push(&[1, 2, 3, 4]);
        assert_eq!(ring.take(2), vec![1, 2]);
        assert_eq!(ring.len(), 2);
    }

    #[test]
    fn wake_frame_constant_matches_80ms_at_16khz() {
        // 80 ms * 16 000 / 1000 = 1280 samples.
        assert_eq!(WAKE_FRAME_SAMPLES, 1_280);
    }
}
