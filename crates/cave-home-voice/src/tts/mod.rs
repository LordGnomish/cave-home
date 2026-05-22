// SPDX-License-Identifier: Apache-2.0
//! Text-to-speech engines.
//!
//! # Upstream:
//! - `rhasspy/piper@23dee2e:src/cpp/piper.cpp::synthesize` — the
//!   reference synthesis driver.
//! - `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/dialog/dialog.py` — the
//!   dialog manager calls the TTS engine with a rendered string + voice
//!   id; we keep that two-stage shape.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::audio::PcmFrame;
use crate::error::{VoiceError, VoiceResult};

pub mod piper;

pub use piper::{MockTtsEngine, PiperVoice, PiperVoiceConfig};

/// One TTS synthesis request.
///
/// # Upstream:
/// `rhasspy/piper@23dee2e:src/cpp/piper.hpp::PiperConfig`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsRequest {
    pub text: String,
    /// Voice id (filename of the `.onnx` + `.onnx.json` pair).
    pub voice: String,
    /// Language tag override; falls through to the voice's default.
    #[serde(default)]
    pub language: Option<String>,
}

impl TtsRequest {
    #[must_use]
    pub fn new<S: Into<String>, V: Into<String>>(text: S, voice: V) -> Self {
        Self {
            text: text.into(),
            voice: voice.into(),
            language: None,
        }
    }
}

/// Output of a synthesis call.
#[derive(Debug, Clone)]
pub struct SynthesisResult {
    pub frame: PcmFrame,
    pub voice: String,
    pub language: String,
}

/// TTS engine trait.
#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn synthesize(&self, req: TtsRequest) -> VoiceResult<SynthesisResult>;
    /// List the voice ids the engine knows about.
    fn voices(&self) -> Vec<String>;
    fn name(&self) -> &'static str;
}

/// A registry of loaded voices, keyed by id.
///
/// # Upstream:
/// `rhasspy/piper@23dee2e:src/cpp/main.cpp::loadVoices` — upstream
/// scans a directory for `.onnx`/`.onnx.json` pairs; we replicate that
/// in [`VoiceRegistry::load_dir`].
#[derive(Debug, Default)]
pub struct VoiceRegistry {
    voices: Mutex<HashMap<String, Arc<PiperVoiceConfig>>>,
}

impl VoiceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: String, config: PiperVoiceConfig) {
        self.voices.lock().insert(id, Arc::new(config));
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<PiperVoiceConfig>> {
        self.voices.lock().get(id).cloned()
    }

    #[must_use]
    pub fn list(&self) -> Vec<String> {
        self.voices.lock().keys().cloned().collect()
    }

    /// Scan `dir` for `*.onnx.json` files and register each one as a
    /// voice. Mirrors `loadVoices`.
    ///
    /// # Errors
    /// Returns `VoiceError::Io` on directory read failure;
    /// `VoiceError::Tts` on a malformed JSON config.
    pub async fn load_dir(&self, dir: &std::path::Path) -> VoiceResult<usize> {
        let mut count = 0_usize;
        let mut read = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read.next_entry().await? {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.ends_with(".onnx.json") {
                continue;
            }
            let id = name.trim_end_matches(".onnx.json").to_string();
            let bytes = tokio::fs::read(&path).await?;
            let raw: PiperVoiceConfig =
                serde_json::from_slice(&bytes).map_err(|e| VoiceError::Tts(e.to_string()))?;
            let onnx = path.with_extension("");
            let onnx = onnx.with_file_name(format!("{id}.onnx"));
            let mut cfg = raw;
            cfg.onnx_path = Some(onnx);
            self.insert(id, cfg);
            count += 1;
        }
        Ok(count)
    }
}
