// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The Ollama `/api/chat` wire codec — cave-home's local LLM gateway.
//!
//! Serialises a [`ChatRequest`] to Ollama's documented JSON body, POSTs it over
//! the injected [`HttpTransport`], maps the status, and decodes the response
//! into a [`ChatResponse`] (content + tool calls). All first-party; no Ollama
//! client library is used.

use async_trait::async_trait;

use super::transport::{HttpMethod, HttpRequest, HttpTransport};
use super::{ChatRequest, ChatResponse, LlmClient};
use crate::error::{JarvisError, Result};

/// The local-LLM gateway: an injected transport + the model server's base URL +
/// the default model name.
#[derive(Debug)]
pub struct OllamaGateway<T: HttpTransport> {
    transport: T,
    base_url: String,
    model: String,
}

impl<T: HttpTransport> OllamaGateway<T> {
    /// A gateway pointing at `base_url` (e.g. `http://127.0.0.1:11434`) using
    /// `model` by default.
    #[must_use]
    pub fn new(transport: T, base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            transport,
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    /// The absolute chat endpoint URL.
    #[must_use]
    pub fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }

    /// Borrow the transport (handy for asserting against a mock in tests).
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }
}

#[async_trait]
impl<T: HttpTransport> LlmClient for OllamaGateway<T> {
    async fn chat(&self, mut req: ChatRequest) -> Result<ChatResponse> {
        // Default the model and force non-streaming.
        if req.model.is_empty() {
            req.model = self.model.clone();
        }
        req.stream = false;

        let body = serde_json::to_string(&req)
            .map_err(|e| JarvisError::LlmDecode(format!("encode request: {e}")))?;

        let http = HttpRequest {
            method: HttpMethod::Post,
            url: self.chat_url(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: Some(body),
        };

        let resp = self.transport.send(http).await?;
        if !(200..300).contains(&resp.status) {
            return Err(JarvisError::LlmHttp {
                status: resp.status,
                body: resp.body.chars().take(200).collect(),
            });
        }

        serde_json::from_str(&resp.body)
            .map_err(|e| JarvisError::LlmDecode(format!("decode response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::transport::MockTransport;
    use crate::llm::{ChatMessage, ToolSpec};
    use serde_json::json;

    fn gateway(t: MockTransport) -> OllamaGateway<MockTransport> {
        OllamaGateway::new(t, "http://127.0.0.1:11434/", "llama3.1")
    }

    #[test]
    fn chat_url_normalises_trailing_slash() {
        let g = gateway(MockTransport::new());
        assert_eq!(g.chat_url(), "http://127.0.0.1:11434/api/chat");
    }

    #[tokio::test]
    async fn posts_request_body_and_parses_reply() {
        let reply = r#"{"model":"llama3.1","message":{"role":"assistant",
            "content":"Hello!"},"done":true}"#;
        let g = gateway(MockTransport::new().route("/api/chat", 200, reply));
        let resp = g
            .chat(ChatRequest::new("", vec![ChatMessage::user("hi")]))
            .await
            .unwrap();
        assert_eq!(resp.content(), "Hello!");
        // The request actually carried the model + the user message.
        let sent = g.transport().nth_body(0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(v["model"], "llama3.1"); // defaulted from the gateway
        assert_eq!(v["stream"], false);
        assert_eq!(v["messages"][0]["content"], "hi");
    }

    #[tokio::test]
    async fn forwards_tool_specs_and_decodes_tool_call() {
        let reply = r#"{"model":"llama3.1","message":{"role":"assistant","content":"",
            "tool_calls":[{"function":{"name":"set_light",
                "arguments":{"room":"kitchen","on":true}}}]},"done":false}"#;
        let g = gateway(MockTransport::new().route("/api/chat", 200, reply));
        let req = ChatRequest::new("llama3.1", vec![ChatMessage::user("kitchen on")])
            .with_tools(vec![ToolSpec::function(
                "set_light",
                "switch a light",
                json!({"type":"object"}),
            )]);
        let resp = g.chat(req).await.unwrap();
        assert!(resp.wants_tools());
        assert_eq!(resp.tool_calls()[0].name(), "set_light");
        // The tools array reached the server.
        let sent: serde_json::Value =
            serde_json::from_str(&g.transport().nth_body(0).unwrap()).unwrap();
        assert_eq!(sent["tools"][0]["function"]["name"], "set_light");
    }

    #[tokio::test]
    async fn non_2xx_maps_to_llm_http_error() {
        let g = gateway(MockTransport::new().route("/api/chat", 500, "model overloaded"));
        let err = g.chat(ChatRequest::new("m", vec![])).await.unwrap_err();
        assert!(matches!(err, JarvisError::LlmHttp { status: 500, .. }));
    }

    #[tokio::test]
    async fn malformed_body_maps_to_decode_error() {
        let g = gateway(MockTransport::new().route("/api/chat", 200, "not json"));
        let err = g.chat(ChatRequest::new("m", vec![])).await.unwrap_err();
        assert!(matches!(err, JarvisError::LlmDecode(_)));
    }

    #[tokio::test]
    async fn transport_failure_propagates() {
        let t = MockTransport::new();
        t.set_failure(Some("refused".into()));
        let g = gateway(t);
        assert!(matches!(
            g.chat(ChatRequest::new("m", vec![])).await.unwrap_err(),
            JarvisError::Transport(_)
        ));
    }
}
