// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cave-home's self-contained **local LLM gateway**.
//!
//! cave-home never calls a cloud model (Charter §9) and is strictly isolated
//! from `cave-runtime`'s shared `cave-llm-gateway` — this is the home's own
//! mini gateway, speaking the documented Ollama `/api/chat` protocol (chat
//! turns + function/tool calling) to a model the household runs locally.
//!
//! This module is the protocol *model* and the [`LlmClient`] seam; the wire
//! codec lives in [`ollama`] and the socket transport in [`transport`]. Tests
//! drive everything through [`MockLlm`] (scripted responses) or
//! [`ollama::OllamaGateway`] over [`transport::MockTransport`] (scripted HTTP).

pub mod ollama;
pub mod transport;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{JarvisError, Result};

/// A chat role, serialised exactly as Ollama expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The system prompt.
    System,
    /// The household speaking.
    User,
    /// The model.
    Assistant,
    /// A tool's result, fed back into the conversation.
    Tool,
}

/// The `function` payload inside a tool call (Ollama's wire shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallFunction {
    /// The tool name the model wants to call.
    pub name: String,
    /// The arguments object the model produced.
    pub arguments: serde_json::Value,
}

/// A tool call the model emitted — matches Ollama's `{"function": {...}}` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The called function.
    pub function: ToolCallFunction,
}

impl ToolCall {
    /// Build a tool call (mostly for tests and result echoing).
    #[must_use]
    pub fn new(name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            function: ToolCallFunction {
                name: name.into(),
                arguments,
            },
        }
    }

    /// The tool name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.function.name
    }

    /// The argument object.
    #[must_use]
    pub const fn arguments(&self) -> &serde_json::Value {
        &self.function.arguments
    }
}

/// One message in a chat conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Who is speaking.
    pub role: Role,
    /// The textual content (may be empty when the turn is a pure tool call).
    #[serde(default)]
    pub content: String,
    /// Tool calls the assistant emitted (empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For a `tool` result message: which tool produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl ChatMessage {
    /// A system-prompt message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }

    /// A user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }

    /// An assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(Role::Assistant, content)
    }

    /// A tool-result message feeding `result` back for `tool_name`.
    #[must_use]
    pub fn tool_result(tool_name: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: result.into(),
            tool_calls: Vec::new(),
            tool_name: Some(tool_name.into()),
        }
    }

    fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }
}

/// The `function` half of a tool advertised to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunctionSpec {
    /// The tool name.
    pub name: String,
    /// A natural-language description the model uses to decide when to call it.
    pub description: String,
    /// A JSON-Schema object describing the arguments.
    pub parameters: serde_json::Value,
}

/// A tool advertised to the model in a [`ChatRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolSpec {
    /// Always `"function"` for Ollama.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// The function definition.
    pub function: FunctionSpec,
}

impl ToolSpec {
    /// Build a function tool with a JSON-Schema parameter object.
    #[must_use]
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function",
            function: FunctionSpec {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// A chat request to the local model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatRequest {
    /// The model name (e.g. `llama3.1`).
    pub model: String,
    /// The conversation so far.
    pub messages: Vec<ChatMessage>,
    /// Tools the model may call (omitted from the wire when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    /// cave-home always requests a single non-streamed response.
    pub stream: bool,
    /// Optional model options (temperature, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

impl ChatRequest {
    /// A non-streaming request for `model` with `messages` and no tools.
    #[must_use]
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: Vec::new(),
            stream: false,
            options: None,
        }
    }

    /// Attach the advertised tools.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = tools;
        self
    }
}

/// A chat response from the local model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChatResponse {
    /// The model that answered.
    #[serde(default)]
    pub model: String,
    /// The assistant message (content and/or tool calls).
    pub message: ChatMessage,
    /// Whether the model considers the turn complete.
    #[serde(default)]
    pub done: bool,
}

impl ChatResponse {
    /// The tool calls the model emitted this turn.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.message.tool_calls
    }

    /// Whether the model wants to call any tool.
    #[must_use]
    pub fn wants_tools(&self) -> bool {
        !self.message.tool_calls.is_empty()
    }

    /// The assistant's natural-language content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.message.content
    }
}

/// The pluggable local-LLM client. Implemented by [`ollama::OllamaGateway`] over
/// a real (Phase-1b) transport and by [`MockLlm`] for the suite.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Run one chat turn.
    ///
    /// # Errors
    /// Transport / HTTP / decode errors per [`JarvisError`].
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
}

/// A scripted LLM for tests: returns queued responses FIFO and records every
/// request, so a multi-turn tool-calling conversation can be asserted exactly.
#[derive(Debug, Default)]
pub struct MockLlm {
    scripted: Mutex<std::collections::VecDeque<ChatResponse>>,
    /// Every request seen, in order.
    pub requests: Mutex<Vec<ChatRequest>>,
}

impl MockLlm {
    /// An empty mock (errors until scripted).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Script a plain-text assistant reply.
    #[must_use]
    pub fn reply(self, content: impl Into<String>) -> Self {
        self.push(ChatResponse {
            model: "mock".into(),
            message: ChatMessage::assistant(content),
            done: true,
        });
        self
    }

    /// Script a turn in which the model calls one tool.
    #[must_use]
    pub fn call_tool(self, name: impl Into<String>, arguments: serde_json::Value) -> Self {
        self.push(ChatResponse {
            model: "mock".into(),
            message: ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![ToolCall::new(name, arguments)],
                tool_name: None,
            },
            done: false,
        });
        self
    }

    /// Script a response verbatim.
    pub fn push(&self, r: ChatResponse) {
        self.scripted.lock().push_back(r);
    }

    /// How many chat turns were requested.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.requests.lock().len()
    }
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        self.requests.lock().push(req);
        self.scripted
            .lock()
            .pop_front()
            .ok_or_else(|| JarvisError::LlmDecode("no scripted response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serialises_to_ollama_shape() {
        let req = ChatRequest::new("llama3.1", vec![ChatMessage::user("hi")]).with_tools(vec![
            ToolSpec::function(
                "set_light",
                "turn a light on or off",
                json!({"type":"object","properties":{"on":{"type":"boolean"}}}),
            ),
        ]);
        let v: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "llama3.1");
        assert_eq!(v["stream"], false);
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["tools"][0]["type"], "function");
        assert_eq!(v["tools"][0]["function"]["name"], "set_light");
    }

    #[test]
    fn empty_tools_are_omitted_from_wire() {
        let req = ChatRequest::new("m", vec![ChatMessage::user("hi")]);
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("tools").is_none());
    }

    #[test]
    fn response_with_tool_call_parses() {
        let body = r#"{
            "model":"llama3.1",
            "message":{"role":"assistant","content":"",
                "tool_calls":[{"function":{"name":"set_light",
                    "arguments":{"room":"kitchen","on":true}}}]},
            "done":false
        }"#;
        let resp: ChatResponse = serde_json::from_str(body).unwrap();
        assert!(resp.wants_tools());
        let call = &resp.tool_calls()[0];
        assert_eq!(call.name(), "set_light");
        assert_eq!(call.arguments()["room"], "kitchen");
        assert_eq!(call.arguments()["on"], true);
    }

    #[test]
    fn tool_result_message_round_trips() {
        let m = ChatMessage::tool_result("set_light", "ok");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"], "ok");
        assert_eq!(v["tool_name"], "set_light");
    }

    #[tokio::test]
    async fn mock_llm_replays_scripted_turns() {
        let llm = MockLlm::new()
            .call_tool("set_light", json!({"on":true}))
            .reply("Done.");
        let first = llm.chat(ChatRequest::new("m", vec![])).await.unwrap();
        assert!(first.wants_tools());
        let second = llm.chat(ChatRequest::new("m", vec![])).await.unwrap();
        assert_eq!(second.content(), "Done.");
        assert_eq!(llm.request_count(), 2);
    }
}
