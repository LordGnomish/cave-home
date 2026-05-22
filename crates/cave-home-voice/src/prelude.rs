// SPDX-License-Identifier: Apache-2.0
//! Common imports for downstream crates.

pub use crate::audio::{AudioRing, PcmFrame};
pub use crate::bus::{VoiceBus, VoiceEventSink, VoiceMessage};
pub use crate::dialog::DialogRenderer;
pub use crate::error::{VoiceError, VoiceResult};
pub use crate::intent::{
    AdaptIntent, AdaptParser, IntentMatch, IntentParser, IntentRouter, PadatiousIntent,
    PadatiousParser,
};
pub use crate::pipeline::{PipelineConfig, PipelineOutcome, VoicePipeline};
pub use crate::skill::{Skill, SkillContext, SkillLoader, SkillResponse};
pub use crate::stt::{MockSttEngine, SttEngine, SttRequest, Transcript, TranscriptSegment};
pub use crate::tts::{MockTtsEngine, SynthesisResult, TtsEngine, TtsRequest, VoiceRegistry};
pub use crate::wake::{MockWakeEngine, WakeEngine, WakeEvent};
