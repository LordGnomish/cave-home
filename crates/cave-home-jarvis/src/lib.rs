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
pub mod dispatch;
pub mod error;
pub mod features;
pub mod llm;
pub mod profile;
pub mod room;
pub mod stt;
pub mod tools;
pub mod tts;
pub mod wake;

pub use error::{JarvisError, Result};
