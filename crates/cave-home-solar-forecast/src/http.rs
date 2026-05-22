// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! HTTP transport abstraction. The trait is intentionally minimal so
//! cave-home-binary can ship a `reqwest` adapter and tests can plug
//! in [`MockHttpClient`] with canned responses.

use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// Outgoing HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl HttpRequest {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
        }
    }
}

/// Incoming HTTP response — status + body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Async HTTP transport trait used by both Forecast.Solar and PVGIS
/// clients. The cave-home-binary adapter implements it with `reqwest`.
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn fetch(&self, req: HttpRequest) -> Result<HttpResponse>;
}

/// Canned-response mock client. Looks up the URL in a map; missing
/// URLs return HTTP 404. Useful for deterministic unit tests.
#[derive(Debug, Default)]
pub struct MockHttpClient {
    pub responses: Mutex<HashMap<String, HttpResponse>>,
    pub calls: Mutex<Vec<HttpRequest>>,
}

impl MockHttpClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-can a response for `url`.
    pub fn insert(&self, url: impl Into<String>, response: HttpResponse) {
        self.responses
            .lock()
            .expect("mock responses lock poisoned")
            .insert(url.into(), response);
    }

    /// Snapshot of all requests made so far.
    #[must_use]
    pub fn calls(&self) -> Vec<HttpRequest> {
        self.calls
            .lock()
            .expect("mock calls lock poisoned")
            .clone()
    }
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn fetch(&self, req: HttpRequest) -> Result<HttpResponse> {
        self.calls
            .lock()
            .expect("mock calls lock poisoned")
            .push(req.clone());
        self.responses
            .lock()
            .expect("mock responses lock poisoned")
            .get(&req.url)
            .cloned()
            .map_or_else(
                || {
                    Ok(HttpResponse {
                        status: 404,
                        body: "not found".to_string(),
                    })
                },
                Ok,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_canned() {
        let m = MockHttpClient::new();
        m.insert(
            "https://example/api",
            HttpResponse {
                status: 200,
                body: "{}".into(),
            },
        );
        let r = m.fetch(HttpRequest::get("https://example/api")).await.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, "{}");
    }

    #[tokio::test]
    async fn mock_returns_404_when_unknown() {
        let m = MockHttpClient::new();
        let r = m.fetch(HttpRequest::get("https://x/nope")).await.unwrap();
        assert_eq!(r.status, 404);
    }

    #[tokio::test]
    async fn mock_records_calls() {
        let m = MockHttpClient::new();
        m.insert(
            "https://example/api",
            HttpResponse {
                status: 200,
                body: "{}".into(),
            },
        );
        let _ = m.fetch(HttpRequest::get("https://example/api")).await.unwrap();
        let _ = m.fetch(HttpRequest::get("https://x/foo")).await.unwrap();
        assert_eq!(m.calls().len(), 2);
    }
}
