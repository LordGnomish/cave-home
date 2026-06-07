// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! In-process HTTP mock bridge for the transport tests.
//!
//! Deliberately *not* an external mock crate (wiremock/httpmock): a bare tokio
//! `TcpListener` that captures one request per connection and replays a canned
//! raw HTTP/1.1 response. That is enough to drive the real `reqwest` client
//! end-to-end — URL building, the `hue-application-key` header, request bodies,
//! status-code mapping and SSE framing all go over a real socket.

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// One request the mock bridge saw, captured verbatim.
#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl CapturedRequest {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Spawn a mock bridge that serves each raw HTTP/1.1 string in `responses`,
/// one per inbound connection, then stops. Returns the base URL the
/// [`super::transport::ReqwestTransport`] should target plus the shared capture log.
pub async fn spawn_mock(responses: Vec<String>) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock bridge");
    let addr = listener.local_addr().expect("mock addr");
    let base = format!("http://{addr}");
    let caps = Arc::new(Mutex::new(Vec::new()));
    let caps2 = Arc::clone(&caps);
    tokio::spawn(async move {
        for resp in responses {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            if let Some(req) = read_request(&mut sock).await {
                caps2.lock().expect("lock").push(req);
            }
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
            let _ = sock.shutdown().await;
        }
    });
    (base, caps)
}

async fn read_request(sock: &mut TcpStream) -> Option<CapturedRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut reqline = lines.next()?.split_whitespace();
    let method = reqline.next()?.to_string();
    let path = reqline.next()?.to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim().to_string(), v.trim().to_string());
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }

    let body_start = header_end + 4;
    let mut body = buf[body_start..].to_vec();
    while body.len() < content_length {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some(CapturedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A `200 OK` JSON response (Hue envelope), `Connection: close`.
pub fn http_ok(body: &str) -> String {
    http_status(200, body)
}

/// An arbitrary-status JSON response, `Connection: close`.
pub fn http_status(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Status",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A `text/event-stream` response carrying a fixed SSE payload.
pub fn http_sse(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
