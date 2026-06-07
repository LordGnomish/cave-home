// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The dispatch brain: turn a [`Transcript`] into executed service calls and a
//! spoken reply, choosing between two paths.
//!
//! 1. **NLU fast-path** — [`cave_home_voice::understand`] matches the sentence
//!    against the built-in intent grammar. A hit is cheap, deterministic and
//!    needs no model; the matched [`IntentAction`] becomes a tool call.
//! 2. **LLM fall-back** — when the grammar does not match (free-form or
//!    multi-step requests), the local LLM gateway is asked, advertising the same
//!    tool surface. The model's tool calls are validated, executed, and fed back
//!    until it produces a natural-language answer (a bounded tool-calling loop).
//!
//! Both paths converge on the one [`ToolRegistry`] / [`ToolExecutor`] surface,
//! so a command behaves identically however it was understood.

use cave_home_voice::{understand, CompiledIntent, Lang, Understanding};

use crate::error::Result;
use crate::llm::{ChatMessage, ChatRequest, LlmClient};
use crate::stt::Transcript;
use crate::tools::{intent_to_tool_call, ToolExecutor, ToolRegistry, ToolResult};
use crate::llm::ToolCall;

/// Which path produced the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchPath {
    /// Matched the [`cave_home_voice`] intent grammar.
    Nlu,
    /// Resolved by the local LLM with tool calling.
    Llm,
}

/// Who is speaking and where, used to fill in unstated context.
#[derive(Debug, Clone, Default)]
pub struct DispatchContext {
    /// The room the microphone lives in (resolved by the room registry).
    pub room: Option<String>,
    /// The recognised household member, if any.
    pub speaker: Option<String>,
}

/// The full result of dispatching one utterance.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchOutcome {
    /// Which path handled it.
    pub path: DispatchPath,
    /// The natural-language reply to speak.
    pub reply: String,
    /// Every tool call executed, paired with its result, in order.
    pub executed: Vec<(ToolCall, ToolResult)>,
    /// Confidence in `[0,1]` (the NLU match score, or 1.0 for an LLM answer).
    pub confidence: f32,
}

impl DispatchOutcome {
    /// The names of the tools that ran.
    #[must_use]
    pub fn executed_tools(&self) -> Vec<String> {
        self.executed.iter().map(|(c, _)| c.name().to_string()).collect()
    }
}

/// Dispatcher tuning.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// The local model name to ask.
    pub model: String,
    /// The reply language for the NLU path.
    pub lang: Lang,
    /// Maximum tool-call rounds before giving up (loop guard).
    pub max_tool_rounds: u32,
    /// The base system prompt for the LLM path.
    pub system_prompt: String,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            model: "llama3.1".into(),
            lang: Lang::En,
            max_tool_rounds: 4,
            system_prompt: "You are Jarvis, the voice assistant for a private smart home. \
                Use the provided tools to control devices. Keep spoken replies short and \
                grandma-friendly. Never invent device state — call query_state to read it."
                .into(),
        }
    }
}

/// The dispatcher: compiled intents + the LLM client + the tool surface.
#[derive(Debug)]
pub struct Dispatcher<L: LlmClient, E: ToolExecutor> {
    intents: Vec<CompiledIntent>,
    llm: L,
    executor: E,
    registry: ToolRegistry,
    config: DispatchConfig,
}

impl<L: LlmClient, E: ToolExecutor> Dispatcher<L, E> {
    /// Build a dispatcher.
    #[must_use]
    pub const fn new(
        intents: Vec<CompiledIntent>,
        llm: L,
        executor: E,
        registry: ToolRegistry,
        config: DispatchConfig,
    ) -> Self {
        Self {
            intents,
            llm,
            executor,
            registry,
            config,
        }
    }

    /// Borrow the executor (handy for asserting against a mock).
    #[must_use]
    pub const fn executor(&self) -> &E {
        &self.executor
    }

    /// Borrow the LLM client.
    #[must_use]
    pub const fn llm(&self) -> &L {
        &self.llm
    }

    /// Dispatch one transcript to a spoken outcome.
    ///
    /// # Errors
    /// Propagates transport / executor errors from the LLM path.
    pub async fn dispatch(
        &self,
        transcript: &Transcript,
        ctx: &DispatchContext,
    ) -> Result<DispatchOutcome> {
        // 1. NLU fast-path.
        match understand(&transcript.text, &self.intents, self.config.lang) {
            Understanding::Acted {
                action,
                reply,
                confidence,
            } => {
                let call = intent_to_tool_call(&action);
                let result = self.executor.execute(&call).await?;
                Ok(DispatchOutcome {
                    path: DispatchPath::Nlu,
                    reply,
                    executed: vec![(call, result)],
                    confidence,
                })
            }
            // 2. LLM fall-back for everything the grammar didn't catch.
            Understanding::NotUnderstood { .. } | Understanding::NeedsClarification { .. } => {
                self.dispatch_via_llm(transcript, ctx).await
            }
        }
    }

    /// The bounded tool-calling conversation with the local model.
    async fn dispatch_via_llm(
        &self,
        transcript: &Transcript,
        ctx: &DispatchContext,
    ) -> Result<DispatchOutcome> {
        let mut messages = vec![
            ChatMessage::system(self.system_prompt_with_context(ctx)),
            ChatMessage::user(&transcript.text),
        ];
        let mut executed: Vec<(ToolCall, ToolResult)> = Vec::new();

        for _round in 0..self.config.max_tool_rounds {
            let req = ChatRequest::new(&self.config.model, messages.clone())
                .with_tools(self.registry.specs().to_vec());
            let resp = self.llm.chat(req).await?;

            if !resp.wants_tools() {
                return Ok(DispatchOutcome {
                    path: DispatchPath::Llm,
                    reply: resp.content().to_string(),
                    executed,
                    confidence: 1.0,
                });
            }

            // Record the assistant's tool-calling turn verbatim.
            messages.push(resp.message.clone());

            for call in resp.tool_calls() {
                let result = match self.registry.validate(call) {
                    Ok(()) => self.executor.execute(call).await?,
                    // Don't abort the conversation on a bad call — tell the
                    // model why so it can correct itself next round.
                    Err(e) => ToolResult::failed(e.to_string()),
                };
                messages.push(ChatMessage::tool_result(call.name(), &result.message));
                executed.push((call.clone(), result));
            }
        }

        // Ran out of rounds still wanting tools — answer with what we have.
        Ok(DispatchOutcome {
            path: DispatchPath::Llm,
            reply: "Sorry, I couldn't finish that.".into(),
            executed,
            confidence: 0.0,
        })
    }

    fn system_prompt_with_context(&self, ctx: &DispatchContext) -> String {
        use std::fmt::Write as _;
        let mut prompt = self.config.system_prompt.clone();
        if let Some(room) = &ctx.room {
            let _ = write!(
                prompt,
                " The person is in the {room}; if they don't name a room, assume the {room}."
            );
        }
        if let Some(speaker) = &ctx.speaker {
            let _ = write!(prompt, " You are speaking with {speaker}.");
        }
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlm;
    use crate::tools::MockToolExecutor;
    use serde_json::json;

    fn intents() -> Vec<CompiledIntent> {
        cave_home_voice::intents::builtin_intents().expect("built-ins")
    }

    fn transcript(text: &str) -> Transcript {
        Transcript::single(text, Lang::En, 1000, 0.95)
    }

    #[tokio::test]
    async fn nlu_path_executes_matched_intent() {
        // Acceptance #2: STT transcript -> intent extraction -> service call.
        let llm = MockLlm::new(); // must NOT be consulted
        let exec = MockToolExecutor::new();
        let d = Dispatcher::new(
            intents(),
            llm,
            exec,
            ToolRegistry::default(),
            DispatchConfig::default(),
        );
        let out = d
            .dispatch(&transcript("turn the kitchen light on"), &DispatchContext::default())
            .await
            .unwrap();
        assert_eq!(out.path, DispatchPath::Nlu);
        assert_eq!(out.executed_tools(), vec!["set_light".to_string()]);
        assert_eq!(out.executed[0].0.arguments()["target"], "kitchen");
        assert_eq!(out.executed[0].0.arguments()["on"], true);
        assert!(out.reply.to_lowercase().contains("turning"));
        assert_eq!(d.llm().request_count(), 0, "LLM should not be called on an NLU hit");
    }

    #[tokio::test]
    async fn llm_path_round_trips_tool_call_then_answers() {
        // Acceptance #3: free-form utterance -> LLM tool call -> execute ->
        // feed result back -> final spoken answer.
        let llm = MockLlm::new()
            .call_tool("set_temperature", json!({"target": "office", "celsius": 22}))
            .reply("Done — I've set the office to 22 degrees.");
        let exec = MockToolExecutor::new();
        let d = Dispatcher::new(
            intents(),
            llm,
            exec,
            ToolRegistry::default(),
            DispatchConfig::default(),
        );
        // A sentence the template grammar won't match.
        let out = d
            .dispatch(
                &transcript("it's a bit chilly in here, warm up the office please"),
                &DispatchContext { room: Some("office".into()), speaker: Some("Burak".into()) },
            )
            .await
            .unwrap();

        assert_eq!(out.path, DispatchPath::Llm);
        assert_eq!(out.executed_tools(), vec!["set_temperature".to_string()]);
        assert_eq!(out.executed[0].1.message, "set_temperature done");
        assert_eq!(out.reply, "Done — I've set the office to 22 degrees.");
        // Two chat turns: the tool-call turn and the final-answer turn.
        assert_eq!(d.llm().request_count(), 2);
        // The second turn must carry the tool result + the room-aware system prompt.
        let second = &d.llm().requests.lock()[1];
        assert!(second.messages.iter().any(|m| m.tool_name.as_deref() == Some("set_temperature")));
        assert!(second.messages[0].content.contains("office"));
    }

    #[tokio::test]
    async fn llm_path_direct_answer_runs_no_tools() {
        let llm = MockLlm::new().reply("There are eight planets.");
        let d = Dispatcher::new(
            intents(),
            llm,
            MockToolExecutor::new(),
            ToolRegistry::default(),
            DispatchConfig::default(),
        );
        let out = d
            .dispatch(&transcript("how many planets are there"), &DispatchContext::default())
            .await
            .unwrap();
        assert_eq!(out.path, DispatchPath::Llm);
        assert!(out.executed.is_empty());
        assert_eq!(out.reply, "There are eight planets.");
    }

    #[tokio::test]
    async fn llm_path_feeds_back_validation_error_for_bad_tool() {
        // Model hallucinates an unknown tool, then recovers next round.
        let llm = MockLlm::new()
            .call_tool("teleport", json!({"to": "mars"}))
            .reply("I can't do that, but everything else is set.");
        let d = Dispatcher::new(
            intents(),
            llm,
            MockToolExecutor::new(),
            ToolRegistry::default(),
            DispatchConfig::default(),
        );
        let out = d
            .dispatch(&transcript("teleport me to mars"), &DispatchContext::default())
            .await
            .unwrap();
        assert_eq!(out.path, DispatchPath::Llm);
        // The bad call was recorded with a failed result; the executor never ran it.
        assert_eq!(out.executed.len(), 1);
        assert!(!out.executed[0].1.ok);
        assert_eq!(d.executor().call_names().len(), 0);
        assert_eq!(out.reply, "I can't do that, but everything else is set.");
    }
}
