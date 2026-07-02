// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The end-to-end assistant pipeline: the state machine that ties every stage
//! together.
//!
//! ```text
//!   AudioSource ─▶ [wait for wake] ─▶ WakeWordDetector
//!                        │ fires
//!                        ▼
//!                  [capture utterance via VAD] ─▶ SpeechToText
//!                        │                              │
//!                        ▼                              ▼
//!                  SpeakerBook (who?)   RoomRegistry (where?)
//!                        └──────────────┬───────────────┘
//!                                       ▼
//!                                  Dispatcher ─▶ tools / LLM
//!                                       ▼
//!                                 TextToSpeech (reply)
//! ```
//!
//! [`JarvisPipeline::run`] drives a live [`AudioSource`] to completion, emitting
//! a [`PipelineEvent`] each time it wakes and each time it handles a command.
//! [`JarvisPipeline::handle_command`] is the per-utterance core, exposed
//! directly so the chain can be tested without synthesising a wake word.

use crate::audio::{AudioFrame, AudioSource, Vad, VadConfig, VadEvent};
use crate::dispatch::{DispatchContext, DispatchOutcome, Dispatcher};
use crate::error::{JarvisError, Result};
use crate::llm::LlmClient;
use crate::metrics::Metrics;
use crate::profile::SpeakerBook;
use crate::room::RoomRegistry;
use crate::stt::SpeechToText;
use crate::tools::ToolExecutor;
use crate::tts::{SpokenReply, TextToSpeech};
use crate::wake::WakeWordDetector;

/// One fully-handled voice interaction.
#[derive(Debug, Clone)]
pub struct Turn {
    /// The device that captured it.
    pub device: String,
    /// The resolved room, if the device was known.
    pub room: Option<String>,
    /// The recognised household member, if any.
    pub speaker: Option<String>,
    /// What was said.
    pub transcript: String,
    /// What the dispatcher did.
    pub outcome: DispatchOutcome,
    /// The synthesised spoken reply.
    pub reply: SpokenReply,
}

/// An event emitted while the pipeline runs.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// The wake word fired on a device.
    Woke {
        /// The device that heard it.
        device: String,
        /// Which keyword.
        keyword: String,
        /// Match confidence.
        confidence: f32,
    },
    /// A command was captured and handled.
    Handled(Box<Turn>),
}

/// The wired voice-assistant pipeline.
pub struct JarvisPipeline<S, L, E, T>
where
    S: SpeechToText,
    L: LlmClient,
    E: ToolExecutor,
    T: TextToSpeech,
{
    wake: WakeWordDetector,
    speakers: SpeakerBook,
    rooms: RoomRegistry,
    stt: S,
    dispatcher: Dispatcher<L, E>,
    tts: T,
    vad_config: VadConfig,
    wake_window_samples: usize,
    metrics: Metrics,
}

impl<S, L, E, T> JarvisPipeline<S, L, E, T>
where
    S: SpeechToText,
    L: LlmClient,
    E: ToolExecutor,
    T: TextToSpeech,
{
    /// Wire a pipeline from its stages.
    #[must_use]
    pub fn new(
        wake: WakeWordDetector,
        speakers: SpeakerBook,
        rooms: RoomRegistry,
        stt: S,
        dispatcher: Dispatcher<L, E>,
        tts: T,
    ) -> Self {
        Self {
            wake,
            speakers,
            rooms,
            stt,
            dispatcher,
            tts,
            vad_config: VadConfig::default(),
            wake_window_samples: 4096,
            metrics: Metrics::new(),
        }
    }

    /// Override the VAD configuration used to bracket commands.
    #[must_use]
    pub const fn with_vad(mut self, vad: VadConfig) -> Self {
        self.vad_config = vad;
        self
    }

    /// Override how many trailing samples the wake stage scores at a time.
    #[must_use]
    pub const fn with_wake_window(mut self, samples: usize) -> Self {
        self.wake_window_samples = samples;
        self
    }

    /// The metric registry (Prometheus exposition via [`Metrics::render`]).
    #[must_use]
    pub const fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Handle one already-captured command utterance end-to-end: identify the
    /// speaker, resolve the room, transcribe, dispatch, and synthesise a reply.
    ///
    /// # Errors
    /// Propagates STT / dispatch / TTS errors.
    pub async fn handle_command(&self, device: &str, command: &[AudioFrame]) -> Result<Turn> {
        // Who is speaking? (Concatenate the captured samples for the embedder.)
        let all_samples: Vec<f32> = command.iter().flat_map(|f| f.samples.clone()).collect();
        let speaker = self.speakers.identify(&all_samples).map(|m| m.name);

        // Where? (Unknown device -> no room context, not an error.)
        let room = self.rooms.room_of(device).ok().map(ToString::to_string);

        // What was said?
        let transcript = self.stt.transcribe(command).await?;
        self.metrics.record_transcript();

        let ctx = DispatchContext {
            room: room.clone(),
            speaker: speaker.clone(),
        };
        let outcome = self.dispatcher.dispatch(&transcript, &ctx).await?;

        self.metrics.record_dispatch(outcome.path);
        self.metrics.record_speaker(speaker.as_deref().unwrap_or(""));
        for (call, result) in &outcome.executed {
            self.metrics.record_tool(call.name(), !result.ok);
        }

        let reply = self
            .tts
            .synthesize(&outcome.reply, self.dispatcher.lang())
            .await?;

        Ok(Turn {
            device: device.to_string(),
            room,
            speaker,
            transcript: transcript.text,
            outcome,
            reply,
        })
    }

    /// Drive a live audio source to completion, waking on the keyword and
    /// handling each bracketed command. Returns the events in order.
    ///
    /// # Errors
    /// Propagates non-end-of-stream audio errors and any stage error.
    pub async fn run<A: AudioSource>(&self, source: &A) -> Result<Vec<PipelineEvent>> {
        let mut events = Vec::new();
        let mut listening = true; // true = waiting for wake, false = capturing
        let mut wake_buf: Vec<f32> = Vec::new();
        let mut cmd_frames: Vec<AudioFrame> = Vec::new();
        let mut vad = Vad::new(self.vad_config);
        let mut device = String::new();

        loop {
            let frame = match source.next_frame().await {
                Ok(f) => f,
                Err(JarvisError::AudioEnded) => break,
                Err(other) => return Err(other),
            };

            if listening {
                device = frame.device.clone();
                wake_buf.extend_from_slice(&frame.samples);
                if wake_buf.len() >= self.wake_window_samples {
                    let start = wake_buf.len() - self.wake_window_samples;
                    if let Some(hit) = self.wake.detect(&wake_buf[start..]) {
                        self.metrics.record_wake(&hit.keyword);
                        events.push(PipelineEvent::Woke {
                            device: device.clone(),
                            keyword: hit.keyword,
                            confidence: hit.confidence,
                        });
                        listening = false;
                        wake_buf.clear();
                        cmd_frames.clear();
                        vad = Vad::new(self.vad_config);
                    } else {
                        // Keep only the trailing window so the buffer can't grow.
                        wake_buf.drain(..start);
                    }
                }
                continue;
            }

            // Capturing a command: bracket it with the VAD.
            match vad.observe(&frame) {
                VadEvent::SpeechStart | VadEvent::SpeechContinue => cmd_frames.push(frame),
                VadEvent::SpeechEnd => {
                    let turn = self.handle_command(&device, &cmd_frames).await?;
                    events.push(PipelineEvent::Handled(Box::new(turn)));
                    listening = true;
                }
                VadEvent::Idle => {}
            }
        }

        // Source ended mid-capture with buffered speech -> handle it.
        if !listening && !cmd_frames.is_empty() {
            let turn = self.handle_command(&device, &cmd_frames).await?;
            events.push(PipelineEvent::Handled(Box::new(turn)));
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{MockAudioSource, SAMPLE_RATE_HZ};
    use crate::dispatch::{DispatchConfig, DispatchPath};
    use crate::llm::MockLlm;
    use crate::stt::MockStt;
    use crate::tools::{MockToolExecutor, ToolRegistry};
    use crate::tts::MockTts;
    use crate::wake::WakeConfig;
    use cave_home_voice::Lang;
    use std::f32::consts::PI;

    fn tone(freq: f32, n: usize, amp: f32) -> Vec<f32> {
        #[allow(clippy::cast_precision_loss)]
        let srf = SAMPLE_RATE_HZ as f32;
        (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / srf;
                amp * (2.0 * PI * freq * t).sin()
            })
            .collect()
    }

    /// The enrolled wake keyword signal (three tone segments).
    fn keyword(amp: f32) -> Vec<f32> {
        let mut s = Vec::new();
        s.extend(tone(450.0, 1600, amp));
        s.extend(tone(900.0, 1600, amp));
        s.extend(tone(1600.0, 1600, amp));
        s
    }

    /// A loud voiced command frame (low ZCR -> passes the VAD).
    fn voiced(n: usize) -> Vec<f32> {
        let period = 64.0;
        (0..n)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let phase = (i as f32 % period) / period;
                let tri = if phase < 0.5 { 4.0 * phase - 1.0 } else { 3.0 - 4.0 * phase };
                tri * 0.6
            })
            .collect()
    }

    fn dispatcher(llm: MockLlm, exec: MockToolExecutor) -> Dispatcher<MockLlm, MockToolExecutor> {
        Dispatcher::new(
            cave_home_voice::intents::builtin_intents().unwrap(),
            llm,
            exec,
            ToolRegistry::default(),
            DispatchConfig::default(),
        )
    }

    fn pipeline(
        stt: MockStt,
        llm: MockLlm,
        exec: MockToolExecutor,
    ) -> JarvisPipeline<MockStt, MockLlm, MockToolExecutor, MockTts> {
        let mut wake = WakeWordDetector::new(WakeConfig::default());
        wake.enroll("jarvis", &keyword(0.8));
        let mut speakers = SpeakerBook::with_defaults();
        speakers.enroll("Burak", &tone(150.0, 8192, 0.8));
        let rooms = RoomRegistry::new().with_device("mic-kitchen", "kitchen");
        JarvisPipeline::new(wake, speakers, rooms, stt, dispatcher(llm, exec), MockTts::new())
    }

    #[tokio::test]
    async fn handle_command_runs_full_chain_nlu() {
        // mock STT -> intent extraction -> service call -> spoken reply.
        let stt = MockStt::new().say("turn the kitchen light on");
        let p = pipeline(stt, MockLlm::new(), MockToolExecutor::new());
        let cmd = vec![AudioFrame::new("mic-kitchen", voiced(4096))];
        let turn = p.handle_command("mic-kitchen", &cmd).await.unwrap();

        assert_eq!(turn.transcript, "turn the kitchen light on");
        assert_eq!(turn.room.as_deref(), Some("kitchen"));
        assert_eq!(turn.outcome.path, DispatchPath::Nlu);
        assert_eq!(turn.outcome.executed_tools(), vec!["set_light".to_string()]);
        assert!(!turn.reply.samples.is_empty());
        // Metrics recorded the transcript + dispatch + tool.
        let m = p.metrics().render();
        assert!(m.contains("jarvis_transcripts_total 1"));
        assert!(m.contains("jarvis_dispatch_total{path=\"nlu\"} 1"));
        assert!(m.contains("jarvis_tool_calls_total{tool=\"set_light\"} 1"));
    }

    #[tokio::test]
    async fn run_wakes_then_handles_a_command() {
        let stt = MockStt::new().say("turn the kitchen light on");
        let p = pipeline(stt, MockLlm::new(), MockToolExecutor::new())
            .with_wake_window(4096);

        // Wake clip, a pause, the voiced command, then trailing silence to end it.
        let frames = vec![
            AudioFrame::new("mic-kitchen", keyword(0.8)),
            AudioFrame::new("mic-kitchen", voiced(512)),
            AudioFrame::new("mic-kitchen", voiced(512)),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
            AudioFrame::new("mic-kitchen", vec![0.0; 512]),
        ];
        let source = MockAudioSource::from_frames(frames);
        let events = p.run(&source).await.unwrap();

        let woke = events.iter().any(|e| matches!(e, PipelineEvent::Woke { keyword, .. } if keyword == "jarvis"));
        assert!(woke, "expected a wake event; got {events:?}");
        let handled = events.iter().find_map(|e| match e {
            PipelineEvent::Handled(t) => Some(t),
            PipelineEvent::Woke { .. } => None,
        });
        let turn = handled.expect("a handled command");
        assert_eq!(turn.transcript, "turn the kitchen light on");
        assert_eq!(turn.outcome.executed_tools(), vec!["set_light".to_string()]);
        assert!(p.metrics().render().contains("jarvis_wake_total{keyword=\"jarvis\"} 1"));
    }

    #[tokio::test]
    async fn unknown_device_yields_no_room_but_still_handles() {
        let stt = MockStt::new().say("turn the kitchen light on");
        let p = pipeline(stt, MockLlm::new(), MockToolExecutor::new());
        let cmd = vec![AudioFrame::new("mic-attic", voiced(4096))];
        let turn = p.handle_command("mic-attic", &cmd).await.unwrap();
        assert_eq!(turn.room, None);
        assert_eq!(turn.outcome.executed_tools(), vec!["set_light".to_string()]);
    }

    #[tokio::test]
    async fn lang_is_carried_into_tts() {
        let _ = Lang::En; // language plumbing is asserted via the reply below.
        let stt = MockStt::new().say("turn the kitchen light on");
        let p = pipeline(stt, MockLlm::new(), MockToolExecutor::new());
        let cmd = vec![AudioFrame::new("mic-kitchen", voiced(4096))];
        let turn = p.handle_command("mic-kitchen", &cmd).await.unwrap();
        assert_eq!(turn.reply.language, Lang::En);
    }
}
