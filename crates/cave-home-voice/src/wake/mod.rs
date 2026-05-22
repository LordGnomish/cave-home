// SPDX-License-Identifier: Apache-2.0
//! Wake-word detection.
//!
//! # Upstream:
//! `dscripka/openWakeWord@ed7f5b9:openwakeword/model.py::Model.predict`
//! — the runtime loop that turns raw PCM into per-window activation
//! scores. The Rust port lives in [`open_wake_word`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::audio::PcmFrame;
use crate::error::VoiceResult;

pub mod open_wake_word;

pub use open_wake_word::{MockWakeEngine, OpenWakeWordConfig};

/// One activation event emitted by a wake-word engine.
///
/// # Upstream:
/// `dscripka/openWakeWord@ed7f5b9:openwakeword/model.py::Model.predict`
/// returns a dict `{wakeword_id: score}`; we lift the highest-scoring
/// entry above the threshold into a discrete event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeEvent {
    /// Wake-word identifier (e.g. `hey_cave_home`).
    pub wake_word: String,
    /// Activation score in `[0.0, 1.0]`.
    pub score: f32,
    /// Frame offset within the streaming buffer (samples since start).
    pub at_sample: u64,
}

/// Wake-word engine trait.
///
/// `feed` is called with a PCM chunk (any size; engine internally
/// re-windows to 80 ms @ 16 kHz). `Ok(Some(event))` means the activation
/// score crossed the configured threshold during this chunk.
#[async_trait]
pub trait WakeEngine: Send + Sync {
    async fn feed(&self, frame: &PcmFrame) -> VoiceResult<Option<WakeEvent>>;
    fn name(&self) -> &'static str;
    /// Reset any internal state (called after a successful activation
    /// to avoid retriggering on the same audio).
    fn reset(&self);
}
