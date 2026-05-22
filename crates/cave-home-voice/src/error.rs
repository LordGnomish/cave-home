// SPDX-License-Identifier: Apache-2.0
//! Crate-wide error type.
//!
//! # Upstream:
//! OVOS uses Python exceptions throughout (`ovos_core.exceptions`); we
//! collapse those into a single `thiserror`-derived enum because the
//! Rust analogue is a per-call `Result<T, VoiceError>` rather than a
//! propagating exception.

use std::io;

use thiserror::Error;

/// Convenience alias used across the crate.
pub type VoiceResult<T> = Result<T, VoiceError>;

/// Crate-wide error type.
#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("audio: {0}")]
    Audio(String),

    #[error("stt: {0}")]
    Stt(String),

    #[error("tts: {0}")]
    Tts(String),

    #[error("wake-word: {0}")]
    Wake(String),

    #[error("intent: {0}")]
    Intent(String),

    #[error("dialog: {0}")]
    Dialog(String),

    #[error("skill `{skill}`: {reason}")]
    Skill { skill: String, reason: String },

    #[error("bus: {0}")]
    Bus(String),

    #[error("config: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("wav: {0}")]
    Wav(String),
}

impl From<hound::Error> for VoiceError {
    fn from(value: hound::Error) -> Self {
        Self::Wav(value.to_string())
    }
}
