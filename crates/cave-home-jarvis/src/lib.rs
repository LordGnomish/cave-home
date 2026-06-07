// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! `cave-home-jarvis` — the local-first **voice-assistant pipeline** (ADR-024).
//!
//! Where [`cave_home_voice`] is the *natural-language brain* (sentence-template
//! intent matching), this crate is the **assistant runtime** around it: it
//! captures audio, spots the wake word, runs speech-to-text, decides whether a
//! command is a simple intent or needs the local LLM, calls the matching
//! cave-home service through a tool, identifies which household member spoke,
//! resolves which room the microphone lives in, and speaks the reply back — all
//! on-device (Charter §9, no cloud STT/LLM/TTS).
//!
//! It is a clean-room implementation of the `OpenJarvis` pipeline shape and the
//! openWakeWord keyword-spotting approach; the LLM gateway speaks Ollama's
//! documented `/api/chat` tool-calling protocol to a model server the household
//! runs locally. No upstream source was copied (Charter §6.1, ADR-002).
//!
//! # Pipeline stages
//!
//! ```text
//!   AudioSource ─▶ VAD ─▶ WakeWordDetector ─▶ SpeechToText ─▶ SpeakerId
//!                                                                  │
//!                          ┌───────────────────────────────────────┘
//!                          ▼
//!     RoomContext ─▶ Dispatch ──(simple)──▶ cave-home-voice intent ─▶ Tool
//!                          └────(complex)──▶ LlmGateway tool-calling ─▶ Tool
//!                                                                  │
//!                                                                  ▼
//!                                                          TextToSpeech (reply)
//! ```
//!
//! # What is real here, and what is the seam
//!
//! Every *decision* — VAD gating, the DTW wake matcher, intent-vs-LLM routing,
//! tool-call validation, speaker cosine matching, room resolution, the Ollama
//! request/response codec — is first-party and unit-tested. The only Phase-1b
//! seams are the ML model *bindings*: [`audio::AudioSource`] (a real
//! microphone), [`stt::SpeechToText`] (whisper.cpp), [`tts::TextToSpeech`]
//! (piper), and [`llm::transport::HttpTransport`] (a socket to the local model
//! server). Each is a trait with an in-crate mock the whole pipeline is tested
//! against.

pub mod audio;
pub mod config;
pub mod dispatch;
pub mod error;
pub mod features;
pub mod llm;
pub mod metrics;
pub mod pipeline;
pub mod profile;
pub mod room;
pub mod stt;
pub mod tools;
pub mod tts;
pub mod wake;

pub use config::{DevicePlacement, JarvisConfig};
pub use dispatch::{DispatchContext, DispatchOutcome, DispatchPath, Dispatcher};
pub use error::{JarvisError, Result};
pub use metrics::Metrics;
pub use pipeline::{JarvisPipeline, PipelineEvent, Turn};
pub use profile::{SpeakerBook, SpeakerMatch};
pub use room::RoomRegistry;
pub use stt::{SpeechToText, Transcript};
pub use tools::{ToolExecutor, ToolRegistry, ToolResult};
pub use tts::{SpokenReply, TextToSpeech};
pub use wake::{WakeConfig, WakeDetection, WakeWordDetector};

/// Re-exported so callers configure one assistant in one language enum.
pub use cave_home_voice::Lang;

#[cfg(test)]
mod doctest_like {
    //! A compile-checked end-to-end example exercised as a normal test (kept out
    //! of the public docs to avoid leaking the mock types).
    use crate::audio::AudioFrame;
    use crate::dispatch::{DispatchConfig, Dispatcher};
    use crate::llm::MockLlm;
    use crate::pipeline::JarvisPipeline;
    use crate::profile::SpeakerBook;
    use crate::room::RoomRegistry;
    use crate::stt::MockStt;
    use crate::tools::{MockToolExecutor, ToolRegistry};
    use crate::tts::MockTts;
    use crate::wake::{WakeConfig, WakeWordDetector};

    #[tokio::test]
    async fn end_to_end_command_controls_a_light() {
        let stt = MockStt::new().say("turn the kitchen light on");
        let dispatcher = Dispatcher::new(
            cave_home_voice::intents::builtin_intents().unwrap(),
            MockLlm::new(),
            MockToolExecutor::new(),
            ToolRegistry::default(),
            DispatchConfig::default(),
        );
        let pipeline = JarvisPipeline::new(
            WakeWordDetector::new(WakeConfig::default()),
            SpeakerBook::with_defaults(),
            RoomRegistry::new().with_device("mic-kitchen", "kitchen"),
            stt,
            dispatcher,
            MockTts::new(),
        );
        let cmd = vec![AudioFrame::new("mic-kitchen", vec![0.2; 4096])];
        let turn = pipeline.handle_command("mic-kitchen", &cmd).await.unwrap();
        assert_eq!(turn.outcome.executed_tools(), vec!["set_light".to_string()]);
        assert_eq!(turn.room.as_deref(), Some("kitchen"));
    }
}
