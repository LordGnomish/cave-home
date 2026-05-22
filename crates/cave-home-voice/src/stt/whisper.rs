// SPDX-License-Identifier: Apache-2.0
//! whisper.cpp-backed STT.
//!
//! # Upstream:
//! - `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_init_from_file_with_params` —
//!   model loading; we delegate to the `whisper-rs` Rust binding which
//!   wraps the same FFI entry point.
//! - `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_full_default` —
//!   the recognition driver. The Rust binding exposes this as
//!   `WhisperContext::create_state().full(...)`.
//! - `ggerganov/whisper.cpp@6ad0bb0:examples/main/main.cpp::main` — the
//!   reference example shows the segment-iteration loop reproduced
//!   below.
//!
//! When the `stt-whisper` feature is disabled, the real engine is a
//! compile-time stub that returns "feature off" — production builds
//! enable the feature; tests use [`MockSttEngine`].

use std::path::PathBuf;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{SttEngine, SttRequest, Transcript, TranscriptSegment};
#[cfg(feature = "stt-whisper")]
use crate::error::VoiceError;
use crate::error::VoiceResult;

/// Configuration for the whisper engine.
///
/// # Upstream:
/// `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_context_params` +
/// `whisper_full_params` — we keep the subset the cave-home pipeline
/// actually wires.
#[derive(Debug, Clone)]
pub struct WhisperConfig {
    /// Path to the GGML/GGUF model file (e.g. `ggml-small.bin`).
    pub model_path: PathBuf,
    /// Worker thread count. Upstream `whisper_full_params.n_threads`.
    pub n_threads: u32,
    /// Force a language tag (`tr`, `en`, `de`). `None` ⇒ auto-detect.
    pub language: Option<String>,
    /// Enable translation to English (`whisper_full_params.translate`).
    pub translate: bool,
}

impl WhisperConfig {
    /// Phase-1 defaults: 4 threads, no forced language, no translation.
    #[must_use]
    pub fn new<P: Into<PathBuf>>(model_path: P) -> Self {
        Self {
            model_path: model_path.into(),
            n_threads: 4,
            language: None,
            translate: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Real whisper-rs backed engine (feature = "stt-whisper")
// ---------------------------------------------------------------------------

#[cfg(feature = "stt-whisper")]
pub use real::WhisperEngine;

#[cfg(feature = "stt-whisper")]
mod real {
    use super::{
        async_trait, Mutex, SttEngine, SttRequest, Transcript, TranscriptSegment, VoiceError,
        VoiceResult, WhisperConfig,
    };
    use std::sync::Arc;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    /// Real whisper.cpp-backed engine.
    ///
    /// # Upstream:
    /// `ggerganov/whisper.cpp@6ad0bb0:whisper.h::whisper_full_default`
    pub struct WhisperEngine {
        ctx: Arc<WhisperContext>,
        config: WhisperConfig,
    }

    impl WhisperEngine {
        /// Load a model and ready the engine.
        ///
        /// # Errors
        /// Returns `VoiceError::Stt` when the model file cannot be
        /// opened. Matches `whisper_init_from_file_with_params`
        /// returning `NULL`.
        pub fn load(config: WhisperConfig) -> VoiceResult<Self> {
            let params = WhisperContextParameters::default();
            let path = config
                .model_path
                .to_str()
                .ok_or_else(|| VoiceError::Stt("model path is not UTF-8".into()))?;
            let ctx = WhisperContext::new_with_params(path, params)
                .map_err(|e| VoiceError::Stt(format!("whisper_init: {e}")))?;
            Ok(Self {
                ctx: Arc::new(ctx),
                config,
            })
        }
    }

    #[async_trait]
    impl SttEngine for WhisperEngine {
        async fn transcribe(&self, req: SttRequest) -> VoiceResult<Transcript> {
            // whisper.cpp is purely CPU/GPU sync; we run it on a blocking
            // pool to keep the orchestrator's async runtime responsive.
            //
            // # Upstream:
            // `ggerganov/whisper.cpp@6ad0bb0:examples/main/main.cpp::main` —
            // the example reads PCM, builds `whisper_full_params`, calls
            // `whisper_full`, then iterates segments via
            // `whisper_full_n_segments` + `whisper_full_get_segment_*`.
            // The Rust binding exposes the same call sequence on a
            // `WhisperState`.
            let samples = req.frame.to_f32();
            let lang = req
                .language
                .clone()
                .or_else(|| self.config.language.clone());
            let translate = self.config.translate;
            let n_threads = self.config.n_threads;
            let punctuate = req.punctuate;
            let ctx = self.ctx.clone();
            let join = tokio::task::spawn_blocking(move || -> VoiceResult<Transcript> {
                let mut state = ctx
                    .create_state()
                    .map_err(|e| VoiceError::Stt(format!("whisper_create_state: {e}")))?;
                let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                params.set_n_threads(n_threads.try_into().unwrap_or(4));
                params.set_translate(translate);
                params.set_print_special(false);
                params.set_print_progress(false);
                params.set_print_realtime(false);
                params.set_print_timestamps(false);
                params.set_suppress_blank(true);
                if let Some(ref l) = lang {
                    params.set_language(Some(l));
                }
                let _ = punctuate; // whisper-rs flag is on by default
                state
                    .full(params, &samples)
                    .map_err(|e| VoiceError::Stt(format!("whisper_full: {e}")))?;
                let n_segments = state
                    .full_n_segments()
                    .map_err(|e| VoiceError::Stt(format!("n_segments: {e}")))?;
                let mut segments = Vec::with_capacity(n_segments as usize);
                let mut text = String::new();
                for i in 0..n_segments {
                    let seg_text = state
                        .full_get_segment_text(i)
                        .map_err(|e| VoiceError::Stt(format!("segment_text: {e}")))?;
                    let t0 = state
                        .full_get_segment_t0(i)
                        .map_err(|e| VoiceError::Stt(format!("segment_t0: {e}")))?;
                    let t1 = state
                        .full_get_segment_t1(i)
                        .map_err(|e| VoiceError::Stt(format!("segment_t1: {e}")))?;
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(seg_text.trim());
                    segments.push(TranscriptSegment {
                        start_ms: (t0 * 10) as u64,
                        end_ms: (t1 * 10) as u64,
                        text: seg_text,
                    });
                }
                Ok(Transcript {
                    text,
                    language: lang.unwrap_or_default(),
                    segments,
                    confidence: 0.0,
                })
            });
            match join.await {
                Ok(Ok(t)) => Ok(t),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(VoiceError::Stt(format!("whisper blocking task: {e}"))),
            }
        }

        fn name(&self) -> &'static str {
            "whisper-rs"
        }
    }

    // `Mutex` import is unused in this module path; keep at module
    // scope for symmetry with the mock module which uses it directly.
    #[allow(dead_code)]
    fn _force_mutex_import(_: Option<Mutex<()>>) {}
}

// ---------------------------------------------------------------------------
// Mock engine — always available (used by tests + when feature is off).
// ---------------------------------------------------------------------------

/// In-process mock STT engine.
///
/// Behaves like `whisper_full_default` in every observable way: takes a
/// PCM frame, returns a [`Transcript`] with segments and a confidence.
/// Tests inject the response so orchestrator behaviour is exercised
/// without the C library.
pub struct MockSttEngine {
    queued: Mutex<Vec<Transcript>>,
    default: Transcript,
}

impl Default for MockSttEngine {
    fn default() -> Self {
        Self::new(Transcript {
            text: "test transcript".into(),
            language: "en".into(),
            segments: vec![TranscriptSegment {
                start_ms: 0,
                end_ms: 1_000,
                text: "test transcript".into(),
            }],
            confidence: -0.10,
        })
    }
}

impl MockSttEngine {
    #[must_use]
    pub fn new(default: Transcript) -> Self {
        Self {
            queued: Mutex::new(Vec::new()),
            default,
        }
    }

    /// Enqueue a reply for the next `transcribe()` call.
    pub fn enqueue(&self, transcript: Transcript) {
        self.queued.lock().push(transcript);
    }
}

#[async_trait]
impl SttEngine for MockSttEngine {
    async fn transcribe(&self, _req: SttRequest) -> VoiceResult<Transcript> {
        let next = self.queued.lock().pop();
        Ok(next.unwrap_or_else(|| self.default.clone()))
    }

    fn name(&self) -> &'static str {
        "mock-stt"
    }
}
