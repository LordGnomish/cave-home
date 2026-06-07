// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The speech-to-text seam: a [`Transcript`] model and the injectable
//! [`SpeechToText`] trait.
//!
//! The production engine is a whisper.cpp-class local model (ggml weights, no
//! cloud — Charter §9); binding it is the Phase-1b seam. The rest of the
//! pipeline consumes the trait and is tested against [`MockStt`], which replays
//! scripted transcripts for scripted audio exactly as a real recogniser would.

use async_trait::async_trait;
use parking_lot::Mutex;

use cave_home_voice::Lang;

use crate::audio::AudioFrame;
use crate::error::{JarvisError, Result};

/// One recognised span of speech with its timing and per-segment confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// The recognised text of this span.
    pub text: String,
    /// Start offset from the utterance start, milliseconds.
    pub start_ms: u32,
    /// End offset from the utterance start, milliseconds.
    pub end_ms: u32,
    /// Confidence in `[0,1]` (whisper's avg-logprob, mapped).
    pub confidence: f32,
}

/// A full recognition result: the joined text, the detected language, the
/// per-segment breakdown, and an overall confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// The full recognised utterance.
    pub text: String,
    /// The language the recogniser detected.
    pub language: Lang,
    /// The individual segments (may be a single span).
    pub segments: Vec<Segment>,
    /// Mean of the segment confidences in `[0,1]`.
    pub confidence: f32,
}

impl Transcript {
    /// A single-segment transcript spanning `0..duration_ms`.
    #[must_use]
    pub fn single(
        text: impl Into<String>,
        language: Lang,
        duration_ms: u32,
        confidence: f32,
    ) -> Self {
        let text = text.into();
        Self {
            segments: vec![Segment {
                text: text.clone(),
                start_ms: 0,
                end_ms: duration_ms,
                confidence,
            }],
            text,
            language,
            confidence,
        }
    }

    /// Whether the recogniser returned nothing usable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// The pluggable speech-to-text engine. The production whisper.cpp binding is
/// Phase-1b; the crate is tested against [`MockStt`].
#[async_trait]
pub trait SpeechToText: Send + Sync {
    /// Transcribe a captured utterance (the frames the VAD bracketed).
    ///
    /// # Errors
    /// [`JarvisError::Stt`] if the engine fails or the audio is unusable.
    async fn transcribe(&self, audio: &[AudioFrame]) -> Result<Transcript>;
}

/// A scripted recogniser for tests and the integration suite.
///
/// Pushed transcripts are returned FIFO; every call records the total sample
/// count it received so tests can assert the right audio reached the engine.
/// When the script empties it errors with [`JarvisError::Stt`], mirroring a real
/// engine that cannot recognise silence.
#[derive(Debug, Default)]
pub struct MockStt {
    scripted: Mutex<std::collections::VecDeque<Transcript>>,
    /// Sample counts seen, in order.
    pub received_samples: Mutex<Vec<usize>>,
}

impl MockStt {
    /// An empty mock (errors until scripted).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Script the next transcript as a single English span.
    #[must_use]
    pub fn say(self, text: impl Into<String>) -> Self {
        self.push(Transcript::single(text, Lang::En, 1000, 0.95));
        self
    }

    /// Script the next transcript verbatim.
    pub fn push(&self, t: Transcript) {
        self.scripted.lock().push_back(t);
    }
}

#[async_trait]
impl SpeechToText for MockStt {
    async fn transcribe(&self, audio: &[AudioFrame]) -> Result<Transcript> {
        let total: usize = audio.iter().map(AudioFrame::len).sum();
        self.received_samples.lock().push(total);
        self.scripted
            .lock()
            .pop_front()
            .ok_or_else(|| JarvisError::Stt("no scripted transcript".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(n: usize) -> AudioFrame {
        AudioFrame::new("mic", vec![0.1; n])
    }

    #[test]
    fn single_transcript_has_one_segment() {
        let t = Transcript::single("hello", Lang::En, 500, 0.9);
        assert_eq!(t.segments.len(), 1);
        assert_eq!(t.segments[0].end_ms, 500);
        assert!(!t.is_empty());
    }

    #[test]
    fn empty_text_is_empty() {
        assert!(Transcript::single("   ", Lang::De, 0, 0.0).is_empty());
    }

    #[tokio::test]
    async fn mock_returns_scripted_text_and_records_audio() {
        let stt = MockStt::new().say("turn the kitchen light on");
        let t = stt.transcribe(&[frame(160), frame(160)]).await.unwrap();
        assert_eq!(t.text, "turn the kitchen light on");
        assert_eq!(t.language, Lang::En);
        assert_eq!(*stt.received_samples.lock(), vec![320]);
    }

    #[tokio::test]
    async fn mock_errors_when_script_empty() {
        let stt = MockStt::new();
        let err = stt.transcribe(&[frame(10)]).await.unwrap_err();
        assert!(matches!(err, JarvisError::Stt(_)));
    }

    #[tokio::test]
    async fn mock_replays_in_order() {
        let stt = MockStt::new().say("first").say("second");
        assert_eq!(stt.transcribe(&[frame(1)]).await.unwrap().text, "first");
        assert_eq!(stt.transcribe(&[frame(1)]).await.unwrap().text, "second");
    }
}
