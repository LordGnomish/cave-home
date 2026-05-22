// SPDX-License-Identifier: Apache-2.0
//! End-to-end voice pipeline.
//!
//! # Upstream:
//! `OpenVoiceOS/ovos-core@5a8f64a:ovos_core/__main__.py::main` — the
//! OVOS entrypoint wires wake-word → STT → intent → skill → TTS. The
//! cave-home pipeline reproduces the same five-stage flow in one
//! `VoicePipeline::process_audio` call.
//!
//! State machine:
//! ```text
//!     PCM frame
//!         │
//!         ▼
//!     wake.feed   ──── no detection ────► return Idle
//!         │
//!         ▼
//!     stt.transcribe
//!         │
//!         ▼
//!     intent.resolve  ──── no match ────► return NoIntent
//!         │
//!         ▼
//!     skill.dispatch ───── no handler ──► return NoSkill
//!         │
//!         ▼
//!     tts.synthesize
//!         │
//!         ▼
//!     return Spoken { transcript, intent, reply_audio }
//! ```

use std::sync::Arc;

use serde_json::json;

use crate::bus::{VoiceBus, VoiceMessage};
use crate::dialog::DialogRenderer;
use crate::error::{VoiceError, VoiceResult};
use crate::intent::{IntentMatch, IntentRouter};
use crate::skill::{SkillContext, SkillLoader, SkillResponse};
use crate::stt::{SttEngine, SttRequest, Transcript};
use crate::tts::{SynthesisResult, TtsEngine, TtsRequest};
use crate::wake::{WakeEngine, WakeEvent};

/// Result of one `process_audio` call.
#[derive(Debug)]
pub enum PipelineOutcome {
    /// Wake-word did not fire.
    Idle,
    /// Wake fired but transcription produced no parsable text.
    NoIntent {
        wake: WakeEvent,
        transcript: Transcript,
    },
    /// Intent matched but no skill claims it.
    NoSkill {
        wake: WakeEvent,
        transcript: Transcript,
        intent: IntentMatch,
    },
    /// Full pipeline ran end-to-end.
    Spoken {
        wake: WakeEvent,
        transcript: Transcript,
        intent: IntentMatch,
        response: SkillResponse,
        reply_audio: SynthesisResult,
    },
}

/// Pipeline configuration knobs.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Default language tag if STT auto-detection fails.
    pub default_language: String,
    /// Default voice id for TTS.
    pub default_voice: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            default_language: "en".into(),
            default_voice: "mock_en_US".into(),
        }
    }
}

/// The end-to-end voice pipeline.
pub struct VoicePipeline {
    wake: Arc<dyn WakeEngine>,
    stt: Arc<dyn SttEngine>,
    router: Arc<IntentRouter>,
    skills: Arc<SkillLoader>,
    dialog: Arc<DialogRenderer>,
    tts: Arc<dyn TtsEngine>,
    bus: VoiceBus,
    config: PipelineConfig,
}

impl VoicePipeline {
    /// Construct with all dependencies wired in.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wake: Arc<dyn WakeEngine>,
        stt: Arc<dyn SttEngine>,
        router: Arc<IntentRouter>,
        skills: Arc<SkillLoader>,
        dialog: Arc<DialogRenderer>,
        tts: Arc<dyn TtsEngine>,
        bus: VoiceBus,
        config: PipelineConfig,
    ) -> Self {
        Self {
            wake,
            stt,
            router,
            skills,
            dialog,
            tts,
            bus,
            config,
        }
    }

    /// Borrow the shared bus.
    #[must_use]
    pub fn bus(&self) -> &VoiceBus {
        &self.bus
    }

    /// Borrow the skill registry.
    #[must_use]
    pub fn skills(&self) -> &SkillLoader {
        &self.skills
    }

    /// Drive one audio chunk through the full pipeline.
    ///
    /// # Errors
    /// Returns whichever sub-engine surfaced an error — wake / STT /
    /// intent are wrapped in [`VoiceError`].
    pub async fn process_audio(&self, frame: crate::audio::PcmFrame) -> VoiceResult<PipelineOutcome> {
        // Stage 1: wake-word.
        let wake_event = match self.wake.feed(&frame).await? {
            Some(ev) => ev,
            None => return Ok(PipelineOutcome::Idle),
        };
        self.bus.publish_best_effort(VoiceMessage::new(
            "voice.wake.detected",
            json!({
                "wake_word": wake_event.wake_word,
                "score": wake_event.score,
                "at_sample": wake_event.at_sample,
            }),
        ));

        // Stage 2: STT.
        let stt_req = SttRequest::new(frame);
        let transcript = self.stt.transcribe(stt_req).await?;
        self.bus.publish_best_effort(VoiceMessage::new(
            "voice.stt.transcribed",
            json!({
                "text": transcript.text,
                "language": transcript.language,
                "confidence": transcript.confidence,
            }),
        ));

        let language = if transcript.language.is_empty() {
            self.config.default_language.clone()
        } else {
            transcript.language.clone()
        };

        // Stage 3: intent.
        let Some(intent) = self.router.resolve(&transcript.text, &language) else {
            return Ok(PipelineOutcome::NoIntent {
                wake: wake_event,
                transcript,
            });
        };
        self.bus.publish_best_effort(VoiceMessage::new(
            "voice.intent.matched",
            json!({
                "intent": intent.name,
                "source": intent.source,
                "confidence": intent.confidence,
                "slots": intent.slots,
            }),
        ));

        // Stage 4: skill dispatch.
        let ctx = SkillContext {
            bus: self.bus.clone(),
            dialog: self.dialog.clone(),
            skill_id: String::new(),
            language: language.clone(),
        };
        let response = match self.skills.dispatch(&intent, &ctx).await? {
            Some(r) => r,
            None => {
                return Ok(PipelineOutcome::NoSkill {
                    wake: wake_event,
                    transcript,
                    intent,
                });
            }
        };
        self.bus.publish_best_effort(VoiceMessage::new(
            "voice.skill.response",
            json!({
                "utterance": response.utterance,
                "language": response.language,
                "voice": response.voice,
            }),
        ));

        // Stage 5: TTS.
        let voice_id = response
            .voice
            .clone()
            .unwrap_or_else(|| self.config.default_voice.clone());
        let tts_req = TtsRequest {
            text: response.utterance.clone(),
            voice: voice_id,
            language: Some(response.language.clone()),
        };
        let reply_audio = self.tts.synthesize(tts_req).await.map_err(|e| match e {
            VoiceError::Tts(msg) => VoiceError::Tts(msg),
            other => other,
        })?;
        self.bus.publish_best_effort(VoiceMessage::new(
            "voice.tts.spoken",
            json!({
                "voice": reply_audio.voice,
                "language": reply_audio.language,
                "samples": reply_audio.frame.samples.len(),
            }),
        ));

        Ok(PipelineOutcome::Spoken {
            wake: wake_event,
            transcript,
            intent,
            response,
            reply_audio,
        })
    }
}
