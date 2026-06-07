// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The text-to-speech seam: a [`SpokenReply`] model and the injectable
//! [`TextToSpeech`] trait.
//!
//! The production engine is a piper-class local synthesiser (ONNX voice, no
//! cloud — Charter §9); binding it is the Phase-1b seam. The pipeline consumes
//! the trait and is tested against [`MockTts`], which renders deterministic
//! placeholder PCM so tests can assert "something was spoken" without a model.

use async_trait::async_trait;
use parking_lot::Mutex;

use cave_home_voice::Lang;

use crate::audio::SAMPLE_RATE_HZ;
use crate::error::Result;

/// Synthesised speech: the source text, its language, and the rendered mono PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct SpokenReply {
    /// The text that was spoken.
    pub text: String,
    /// The language it was spoken in.
    pub language: Lang,
    /// Mono samples in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// Sample rate, Hz.
    pub sample_rate: u32,
}

impl SpokenReply {
    /// Approximate spoken duration in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> u32 {
        if self.sample_rate == 0 {
            return 0;
        }
        #[allow(clippy::cast_possible_truncation)]
        let ms = (self.samples.len() as u64 * 1000 / u64::from(self.sample_rate)) as u32;
        ms
    }
}

/// The pluggable text-to-speech engine. The production piper binding is
/// Phase-1b; the crate is tested against [`MockTts`].
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    /// Render `text` to speech in `language`.
    ///
    /// # Errors
    /// [`JarvisError::Tts`](crate::error::JarvisError::Tts) on engine failure.
    async fn synthesize(&self, text: &str, language: Lang) -> Result<SpokenReply>;
}

/// A deterministic placeholder synthesiser for tests.
///
/// Renders a fixed number of samples per character (so longer replies produce
/// longer audio) and records every phrase it was asked to speak.
#[derive(Debug)]
pub struct MockTts {
    samples_per_char: usize,
    /// Every phrase spoken, in order.
    pub spoken: Mutex<Vec<String>>,
}

impl Default for MockTts {
    fn default() -> Self {
        Self {
            samples_per_char: 160, // ~10 ms per character at 16 kHz
            spoken: Mutex::new(Vec::new()),
        }
    }
}

impl MockTts {
    /// A mock with the default rendering rate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TextToSpeech for MockTts {
    async fn synthesize(&self, text: &str, language: Lang) -> Result<SpokenReply> {
        self.spoken.lock().push(text.to_string());
        let n = text.chars().count() * self.samples_per_char;
        // A faint 220 Hz placeholder tone so the buffer is real audio, not zeros.
        #[allow(clippy::cast_precision_loss)]
        let samples = (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE_HZ as f32;
                0.1 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
            })
            .collect();
        Ok(SpokenReply {
            text: text.to_string(),
            language,
            samples,
            sample_rate: SAMPLE_RATE_HZ,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn synthesizes_non_empty_audio_and_records_text() {
        let tts = MockTts::new();
        let r = tts.synthesize("Turning on the kitchen light.", Lang::En).await.unwrap();
        assert_eq!(r.text, "Turning on the kitchen light.");
        assert_eq!(r.language, Lang::En);
        assert!(!r.samples.is_empty());
        assert_eq!(*tts.spoken.lock(), vec!["Turning on the kitchen light.".to_string()]);
    }

    #[tokio::test]
    async fn longer_text_speaks_longer() {
        let tts = MockTts::new();
        let short = tts.synthesize("ok", Lang::En).await.unwrap();
        let long = tts.synthesize("okay then, here we go", Lang::En).await.unwrap();
        assert!(long.duration_ms() > short.duration_ms());
    }
}
