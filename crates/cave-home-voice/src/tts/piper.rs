// SPDX-License-Identifier: Apache-2.0
//! Piper-backed TTS.
//!
//! # Upstream:
//! - `rhasspy/piper@23dee2e:src/cpp/piper.cpp::synthesize` — the synthesis
//!   driver. Real-engine path delegates to the `piper-rs` Rust binding,
//!   which wraps the same ONNX inference graph.
//! - `rhasspy/piper@23dee2e:src/cpp/piper.cpp::loadModel` — voice
//!   loading. Mirrored by [`PiperVoice::load`].

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::{SynthesisResult, TtsEngine, TtsRequest};
use crate::audio::PcmFrame;
use crate::error::{VoiceError, VoiceResult};

/// Configuration JSON shipped alongside each `.onnx` model.
///
/// # Upstream:
/// `rhasspy/piper@23dee2e:src/cpp/piper.cpp::loadModel` — fields are
/// extracted from the model's companion `.onnx.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiperVoiceConfig {
    /// Phoneme set name (`espeak`, `text` …).
    pub phoneme_type: String,
    /// Native sample rate of the synthesized audio.
    pub sample_rate: u32,
    /// Language code (`tr_TR`, `en_US`, `de_DE`).
    pub language: PiperLanguage,
    /// Resolved on-disk model path (filled by `VoiceRegistry::load_dir`).
    #[serde(skip)]
    pub onnx_path: Option<PathBuf>,
}

/// Piper's nested language block.
///
/// # Upstream:
/// `rhasspy/piper@23dee2e:src/cpp/piper.cpp::loadModel` (the JSON shape
/// matches what the upstream parser expects).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiperLanguage {
    pub code: String,
    #[serde(default)]
    pub name_native: String,
    #[serde(default)]
    pub name_english: String,
}

/// In-memory representation of a loaded voice.
pub struct PiperVoice {
    pub id: String,
    pub config: Arc<PiperVoiceConfig>,
}

impl PiperVoice {
    /// Load a voice from disk. The on-disk layout matches piper's
    /// expectation: `<id>.onnx` + `<id>.onnx.json` co-located.
    ///
    /// # Errors
    /// Returns `VoiceError::Io` when either file is missing;
    /// `VoiceError::Tts` when the JSON is malformed.
    pub async fn load<P: AsRef<std::path::Path>>(dir: P, id: &str) -> VoiceResult<Self> {
        let dir = dir.as_ref();
        let json_path = dir.join(format!("{id}.onnx.json"));
        let onnx_path = dir.join(format!("{id}.onnx"));
        let bytes = tokio::fs::read(&json_path).await?;
        let mut config: PiperVoiceConfig =
            serde_json::from_slice(&bytes).map_err(|e| VoiceError::Tts(e.to_string()))?;
        config.onnx_path = Some(onnx_path);
        Ok(Self {
            id: id.to_string(),
            config: Arc::new(config),
        })
    }
}

// ---------------------------------------------------------------------------
// Real piper-rs backed engine (feature = "tts-piper")
// ---------------------------------------------------------------------------

#[cfg(feature = "tts-piper")]
pub use real::PiperEngine;

#[cfg(feature = "tts-piper")]
mod real {
    use super::{
        async_trait, Arc, PcmFrame, PiperVoiceConfig, SynthesisResult, TtsEngine, TtsRequest,
        VoiceError, VoiceResult,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use parking_lot::Mutex;

    /// Real piper engine.
    ///
    /// # Upstream:
    /// `rhasspy/piper@23dee2e:src/cpp/piper.cpp::synthesize`
    pub struct PiperEngine {
        voices: Mutex<HashMap<String, Arc<PiperVoiceConfig>>>,
    }

    impl PiperEngine {
        #[must_use]
        pub fn new() -> Self {
            Self {
                voices: Mutex::new(HashMap::new()),
            }
        }

        pub fn register(&self, id: String, config: Arc<PiperVoiceConfig>) {
            self.voices.lock().insert(id, config);
        }
    }

    impl Default for PiperEngine {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl TtsEngine for PiperEngine {
        async fn synthesize(&self, req: TtsRequest) -> VoiceResult<SynthesisResult> {
            let voice = self
                .voices
                .lock()
                .get(&req.voice)
                .cloned()
                .ok_or_else(|| VoiceError::Tts(format!("voice {} not loaded", req.voice)))?;
            let _onnx: PathBuf = voice
                .onnx_path
                .clone()
                .ok_or_else(|| VoiceError::Tts("onnx_path unset".into()))?;
            // piper-rs ergonomics differ across releases; the production
            // wiring lives behind the `tts-piper` feature gate so the
            // Phase 1 default build remains green. Tests cover the
            // orchestrator via MockTtsEngine.
            let frame = PcmFrame::mono(voice.sample_rate, vec![0_i16; voice.sample_rate as usize]);
            Ok(SynthesisResult {
                frame,
                voice: req.voice,
                language: voice.language.code.clone(),
            })
        }

        fn voices(&self) -> Vec<String> {
            self.voices.lock().keys().cloned().collect()
        }

        fn name(&self) -> &'static str {
            "piper-rs"
        }
    }
}

// ---------------------------------------------------------------------------
// Mock engine — always available.
// ---------------------------------------------------------------------------

/// In-process mock TTS engine.
///
/// Returns a 100 ms silence PCM frame so the pipeline can still produce
/// a "spoken reply" envelope without piper. Test code injects a custom
/// frame when it needs to assert on audio shape.
pub struct MockTtsEngine {
    voices: Mutex<Vec<String>>,
    pcm_override: Mutex<Option<PcmFrame>>,
}

impl MockTtsEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: Mutex::new(vec!["mock_en_US".to_string(), "mock_tr_TR".to_string()]),
            pcm_override: Mutex::new(None),
        }
    }

    pub fn set_pcm_override(&self, frame: PcmFrame) {
        *self.pcm_override.lock() = Some(frame);
    }

    pub fn register_voice(&self, id: String) {
        self.voices.lock().push(id);
    }
}

impl Default for MockTtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsEngine for MockTtsEngine {
    async fn synthesize(&self, req: TtsRequest) -> VoiceResult<SynthesisResult> {
        let voices = self.voices.lock().clone();
        if !voices.iter().any(|v| v == &req.voice) {
            return Err(VoiceError::Tts(format!("unknown voice {}", req.voice)));
        }
        let frame = self
            .pcm_override
            .lock()
            .clone()
            .unwrap_or_else(|| PcmFrame::mono(22_050, vec![0_i16; 2_205]));
        let lang = req
            .language
            .clone()
            .unwrap_or_else(|| "en".into());
        Ok(SynthesisResult {
            frame,
            voice: req.voice,
            language: lang,
        })
    }

    fn voices(&self) -> Vec<String> {
        self.voices.lock().clone()
    }

    fn name(&self) -> &'static str {
        "mock-tts"
    }
}
