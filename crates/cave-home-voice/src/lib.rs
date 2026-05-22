// SPDX-License-Identifier: Apache-2.0
#![allow(
    // Voice DSP routines do f32 arithmetic on usize/u32/u64 counters
    // (sample indices, scores, durations). These casts are inherent
    // to the problem domain; allow them at the crate level rather
    // than annotating every call site.
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
)]
//! cave-home-voice — local voice pipeline.
//!
//! Phase 1 MVP (per ADR-024 + ROADMAP M5) is a line-by-line port of:
//!
//! - **whisper.cpp** (MIT) — speech-to-text driver flow (`stt`).
//! - **piper** (MIT) — text-to-speech inference loop (`tts`).
//! - **openWakeWord** (Apache-2.0) — wake-word runtime (`wake`).
//! - **OVOS / Adapt / Padatious** (Apache-2.0) — intent parsing (`intent`),
//!   dialog management (`dialog`), skill framework (`skill`), and the
//!   in-process message bus (`bus`).
//!
//! Upstream SHAs are pinned in `parity.manifest.toml`. Every line-by-line
//! port lands a `# Upstream:` doc-comment naming the file + symbol it came
//! from.
//!
//! The end-to-end pipeline (`pipeline::VoicePipeline`) wires
//! microphone PCM → wake-word → STT → intent → skill handler → TTS reply
//! in the same control flow as upstream `ovos_core/intent_services/...`.
//!
//! ML bindings are feature-gated so unit tests can run without native
//! whisper.cpp / piper / ONNX libraries:
//!
//! | feature       | enables                                   |
//! |---------------|-------------------------------------------|
//! | `stt-whisper` | real whisper.cpp via `whisper-rs`         |
//! | `tts-piper`   | real piper inference via `piper-rs`       |
//! | `wake-ort`    | real openWakeWord ONNX via `ort`          |
//!
//! Tests use the in-crate `Mock*Engine` impls of the engine traits; they
//! exercise exactly the same orchestrator code path as the production
//! binary.

pub mod audio;
pub mod bus;
pub mod dialog;
pub mod error;
pub mod intent;
pub mod pipeline;
pub mod prelude;
pub mod skill;
pub mod stt;
pub mod tts;
pub mod wake;

pub use error::{VoiceError, VoiceResult};
