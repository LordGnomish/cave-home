// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! The HTTP transport seam for the local LLM gateway.
//!
//! cave-home talks to a model server the household runs on its own hardware
//! (Ollama / llama.cpp). The real `reqwest`/socket transport is the only
//! deferred piece (Phase-1b); the gateway is built and tested entirely against
//! [`MockTransport`], so no network is touched in the suite (Charter §9).

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::{JarvisError, Result};

/// HTTP methods the gateway needs (only `POST`, in practice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
}

/// A transport-ready request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The method.
    pub method: HttpMethod,
    /// The absolute URL.
    pub url: String,
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// The request body, if any.
    pub body: Option<String>,
}

/// A transport response (a non-2xx status is data here, not an error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body.
    pub body: String,
}

/// The pluggable HTTP transport. The production socket transport is Phase-1b.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Perform the request.
    ///
    /// # Errors
    /// [`JarvisError::Transport`] on a socket failure; a non-2xx HTTP status is
    /// *not* an error — it is returned as an [`HttpResponse`].
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse>;
}

/// An in-memory transport for tests and the integration suite.
///
/// Matches requests against URL substrings, supports a FIFO queue of sequenced
/// responses, logs every request, and can be forced to fail.
#[derive(Debug, Default)]
pub struct MockTransport {
    routes: Mutex<Vec<(String, HttpResponse)>>,
    queue: Mutex<std::collections::VecDeque<HttpResponse>>,
    /// Every request seen, in order.
    pub requests: Mutex<Vec<HttpRequest>>,
    failure: Mutex<Option<String>>,
}

impl MockTransport {
    /// An empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: respond to any URL containing `matches` with `status`/`body`.
    #[must_use]
    pub fn route(self, matches: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        self.routes.lock().push((
            matches.into(),
            HttpResponse {
                status,
                body: body.into(),
            },
        ));
        self
    }

    /// Queue a one-shot response, consumed FIFO before routes are consulted.
    /// This is how a multi-turn tool-calling conversation is scripted.
    pub fn push_response(&self, status: u16, body: impl Into<String>) {
        self.queue.lock().push_back(HttpResponse {
            status,
            body: body.into(),
        });
    }

    /// Force every subsequent `send` to fail (or clear it with `None`).
    pub fn set_failure(&self, msg: Option<String>) {
        *self.failure.lock() = msg;
    }

    /// How many requests have been sent.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.requests.lock().len()
    }

    /// The body of the n-th request, if any.
    #[must_use]
    pub fn nth_body(&self, n: usize) -> Option<String> {
        self.requests.lock().get(n).and_then(|r| r.body.clone())
    }
}

#[async_trait]
impl HttpTransport for MockTransport {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().push(req.clone());
        let failure = self.failure.lock().clone();
        if let Some(msg) = failure {
            return Err(JarvisError::Transport(msg));
        }
        let queued = self.queue.lock().pop_front();
        if let Some(r) = queued {
            return Ok(r);
        }
        let routes = self.routes.lock();
        routes
            .iter()
            .find(|(m, _)| req.url.contains(m.as_str()))
            .map(|(_, r)| r.clone())
            .ok_or_else(|| JarvisError::Transport(format!("no mock route for {}", req.url)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(url: &str, body: &str) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Post,
            url: url.into(),
            headers: Vec::new(),
            body: Some(body.into()),
        }
    }

    #[tokio::test]
    async fn route_matches_by_substring() {
        let t = MockTransport::new().route("/api/chat", 200, "ok");
        let r = t.send(post("http://localhost:11434/api/chat", "{}")).await.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, "ok");
        assert_eq!(t.request_count(), 1);
        assert_eq!(t.nth_body(0).as_deref(), Some("{}"));
    }

    #[tokio::test]
    async fn queue_consumed_before_routes() {
        let t = MockTransport::new().route("/api/chat", 200, "route");
        t.push_response(200, "queued");
        assert_eq!(t.send(post("x/api/chat", "{}")).await.unwrap().body, "queued");
        assert_eq!(t.send(post("x/api/chat", "{}")).await.unwrap().body, "route");
    }

    #[tokio::test]
    async fn forced_failure_surfaces_transport_error() {
        let t = MockTransport::new();
        t.set_failure(Some("connection refused".into()));
        assert!(matches!(
            t.send(post("x", "{}")).await.unwrap_err(),
            JarvisError::Transport(_)
        ));
    }

    #[tokio::test]
    async fn unrouted_url_errors() {
        let t = MockTransport::new();
        assert!(t.send(post("nowhere", "{}")).await.is_err());
    }
}
