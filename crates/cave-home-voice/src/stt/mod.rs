// SPDX-License-Identifier: Apache-2.0
//! Speech-to-text engines.
//!
//! # Upstream:
//! `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_full_default` is the
//! canonical "do the whole transcription" entry point; the trait below
//! is the Rust analogue, with feature-gated real bindings in
//! [`whisper`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::audio::PcmFrame;
use crate::error::VoiceResult;

pub mod whisper;

pub use whisper::{MockSttEngine, WhisperConfig};

/// Per-utterance recognition result.
///
/// # Upstream:
/// `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_get_segment_text` —
/// upstream walks segments and concatenates; we keep both the joined
/// text and the per-segment list because OVOS's intent service uses
/// segment offsets for slot timing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    pub text: String,
    /// Detected (or configured) language tag (`tr`, `en`, `de`, …).
    pub language: String,
    /// Per-segment breakdown — `whisper_full_n_segments`.
    pub segments: Vec<TranscriptSegment>,
    /// Average logprob across segments (0.0..-inf; -1.0 ≈ uncertain).
    pub confidence: f32,
}

/// One whisper segment.
///
/// # Upstream:
/// `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_full_get_segment_t0`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Configuration knobs handed to a recognition call.
///
/// # Upstream:
/// `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_full_params`
#[derive(Debug, Clone)]
pub struct SttRequest {
    pub frame: PcmFrame,
    /// `Some("tr")` to force a language; `None` lets the model
    /// auto-detect — parity with `whisper_full_params::detect_language`.
    pub language: Option<String>,
    /// Whether to emit punctuation. Parity with
    /// `whisper_full_params::token_timestamps + ...`.
    pub punctuate: bool,
}

impl SttRequest {
    #[must_use]
    pub const fn new(frame: PcmFrame) -> Self {
        Self {
            frame,
            language: None,
            punctuate: true,
        }
    }
}

/// STT engine interface.
///
/// All implementations are `Send + Sync` so the orchestrator can hold
/// `Arc<dyn SttEngine>` and call from multiple tasks.
#[async_trait]
pub trait SttEngine: Send + Sync {
    /// Run a one-shot recognition over a full utterance.
    async fn transcribe(&self, req: SttRequest) -> VoiceResult<Transcript>;

    /// Human-readable engine name (for `/admin/voice` status).
    fn name(&self) -> &'static str;
}
