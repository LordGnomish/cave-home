// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The crate-wide error type and `Result` alias.
//!
//! Every fallible decision in the pipeline funnels through [`JarvisError`] so the
//! orchestrator can branch on *why* a stage failed (no audio, the model timed
//! out, a tool refused) rather than on opaque strings.

use thiserror::Error;

/// The crate result alias.
pub type Result<T> = std::result::Result<T, JarvisError>;

/// Everything that can go wrong inside the voice-assistant pipeline.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum JarvisError {
    /// The audio source ran dry / the device hung up.
    #[error("audio source ended")]
    AudioEnded,

    /// An audio frame did not satisfy a stage's format contract.
    #[error("audio format: {0}")]
    AudioFormat(String),

    /// The speech-to-text engine failed to transcribe.
    #[error("speech-to-text failed: {0}")]
    Stt(String),

    /// The text-to-speech engine failed to synthesise.
    #[error("text-to-speech failed: {0}")]
    Tts(String),

    /// The underlying transport (HTTP socket to the local model server) failed.
    #[error("transport: {0}")]
    Transport(String),

    /// The LLM server returned a non-2xx status.
    #[error("llm http {status}: {body}")]
    LlmHttp {
        /// The HTTP status code.
        status: u16,
        /// The (truncated) response body.
        body: String,
    },

    /// The LLM response could not be decoded into the expected shape.
    #[error("llm decode: {0}")]
    LlmDecode(String),

    /// The model asked for a tool the registry does not know.
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    /// A tool's arguments did not satisfy its schema.
    #[error("tool '{tool}' arguments: {reason}")]
    ToolArguments {
        /// The tool name.
        tool: String,
        /// Why the arguments were rejected.
        reason: String,
    },

    /// A tool ran but reported a failure.
    #[error("tool '{tool}' failed: {reason}")]
    ToolFailed {
        /// The tool name.
        tool: String,
        /// The failure reported by the executor.
        reason: String,
    },

    /// The utterance could not be routed to any device because the room is
    /// unknown and the command did not name one.
    #[error("no room context for device '{0}'")]
    UnknownRoom(String),

    /// A configuration value was invalid.
    #[error("config: {0}")]
    Config(String),
}
