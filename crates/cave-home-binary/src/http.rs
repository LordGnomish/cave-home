// SPDX-License-Identifier: Apache-2.0
//! A minimal, std-only HTTP/1.1 request parser and response writer.
//!
//! The K3s apiserver speaks HTTP; our `cave-home-apiserver-rs` crate is a pure
//! decision core with no transport (Charter §5 keeps the whole stack in one
//! process, and the transport was deferred). This module is the smallest honest
//! HTTP/1.1 codec that lets [`crate::server`] expose the apiserver over a real
//! socket: it parses a request head + body and renders a response, nothing more
//! (no chunked transfer, no keep-alive — every response sets `Connection:
//! close`). It performs no I/O, so the framing logic is fully unit-testable.

/// A parsed HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Upper-case method (`GET`, `POST`, …).
    pub method: String,
    /// The request-target path, percent-encoding left intact, query stripped.
    pub path: String,
    /// The raw query string without the leading `?` (empty if none).
    pub query: String,
    /// Header `(name, value)` pairs in arrival order; names lower-cased.
    pub headers: Vec<(String, String)>,
    /// The request body bytes (may be empty).
    pub body: Vec<u8>,
}

/// Why a request could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The head (request line + headers) is incomplete — need more bytes.
    Incomplete,
    /// The request line was malformed (not `METHOD TARGET VERSION`).
    BadRequestLine,
    /// A header line had no `:` separator.
    BadHeader,
    /// `Content-Length` was present but not a non-negative integer.
    BadContentLength,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Incomplete => "incomplete request head",
            Self::BadRequestLine => "malformed request line",
            Self::BadHeader => "malformed header line",
            Self::BadContentLength => "malformed Content-Length",
        };
        f.write_str(s)
    }
}

impl std::error::Error for ParseError {}

/// The index just past the `\r\n\r\n` that ends the request head, or `None` if
/// the head has not fully arrived yet.
#[must_use]
pub fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

impl HttpRequest {
    /// Parse a complete HTTP/1.1 message (head followed by exactly the body the
    /// `Content-Length` header declares).
    ///
    /// # Errors
    /// [`ParseError`] variants describe the first framing problem found.
    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        let head_end = head_end(buf).ok_or(ParseError::Incomplete)?;
        // The head is ASCII control text; the body that follows is opaque bytes.
        let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| ParseError::BadRequestLine)?;
        let mut lines = head.split("\r\n");

        let request_line = lines.next().ok_or(ParseError::BadRequestLine)?;
        let mut parts = request_line.split(' ');
        let method = parts.next().filter(|s| !s.is_empty()).ok_or(ParseError::BadRequestLine)?;
        let target = parts.next().filter(|s| !s.is_empty()).ok_or(ParseError::BadRequestLine)?;
        let _version = parts.next().filter(|s| s.starts_with("HTTP/")).ok_or(ParseError::BadRequestLine)?;

        let (path, query) = target.split_once('?').map_or((target, ""), |(p, q)| (p, q));

        let mut headers = Vec::new();
        for line in lines {
            if line.is_empty() {
                continue; // the terminating blank line(s)
            }
            let (name, value) = line.split_once(':').ok_or(ParseError::BadHeader)?;
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }

        Ok(Self {
            method: method.to_ascii_uppercase(),
            path: path.to_string(),
            query: query.to_string(),
            headers,
            body: buf[head_end..].to_vec(),
        })
    }

    /// Case-insensitive header lookup.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        let lname = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == lname).map(|(_, v)| v.as_str())
    }

    /// The declared body length, if a valid `Content-Length` header is present.
    ///
    /// # Errors
    /// [`ParseError::BadContentLength`] if the header is present but unparseable.
    pub fn content_length(&self) -> Result<Option<usize>, ParseError> {
        self.header("content-length").map_or(Ok(None), |v| {
            v.parse::<usize>().map(Some).map_err(|_| ParseError::BadContentLength)
        })
    }
}

/// A response to render back onto the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code (e.g. `200`).
    pub status: u16,
    /// Header `(name, value)` pairs (excluding the framing headers this codec
    /// always adds: `Content-Length`, `Connection`).
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A response with an explicit `Content-Type`.
    #[must_use]
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_string(), content_type.to_string())],
            body: body.into(),
        }
    }

    /// A JSON response (`application/json`).
    #[must_use]
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "application/json", body.into().into_bytes())
    }

    /// A plain-text response (`text/plain; charset=utf-8`).
    #[must_use]
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "text/plain; charset=utf-8", body.into().into_bytes())
    }

    /// Serialize the full HTTP/1.1 response (status line, headers, blank line,
    /// body) to bytes ready for the socket.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        use std::fmt::Write as _;
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason_phrase(self.status));
        for (name, value) in &self.headers {
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        // Infallible: writing to a String never errors.
        let _ = write!(head, "Content-Length: {}\r\n", self.body.len());
        head.push_str("Connection: close\r\n");
        head.push_str("\r\n");
        let mut out = head.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }
}

/// The canonical reason phrase for a status code (the subset this server emits).
#[must_use]
pub const fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> HttpRequest {
        HttpRequest::parse(s.as_bytes()).expect("parse")
    }

    #[test]
    fn parses_method_path_and_query() {
        let r = req("GET /api/v1/nodes?limit=5 HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/api/v1/nodes");
        assert_eq!(r.query, "limit=5");
        assert!(r.body.is_empty());
    }

    #[test]
    fn path_without_query_has_empty_query() {
        let r = req("GET /healthz HTTP/1.1\r\n\r\n");
        assert_eq!(r.path, "/healthz");
        assert_eq!(r.query, "");
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let r = req("GET / HTTP/1.1\r\nContent-Type: application/json\r\n\r\n");
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(r.header("CONTENT-TYPE"), Some("application/json"));
        assert_eq!(r.header("missing"), None);
    }

    #[test]
    fn parses_body_per_content_length() {
        let r = req("POST /x HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello");
        assert_eq!(r.method, "POST");
        assert_eq!(r.body, b"hello");
        assert_eq!(r.content_length().unwrap(), Some(5));
    }

    #[test]
    fn incomplete_head_is_incomplete() {
        assert_eq!(HttpRequest::parse(b"GET / HTTP/1.1\r\nHost: x"), Err(ParseError::Incomplete));
        assert_eq!(head_end(b"GET / HTTP/1.1\r\nHost: x"), None);
    }

    #[test]
    fn head_end_points_past_blank_line() {
        let buf = b"GET / HTTP/1.1\r\n\r\nBODY";
        let end = head_end(buf).expect("head end");
        assert_eq!(&buf[end..], b"BODY");
    }

    #[test]
    fn bad_request_line_is_rejected() {
        assert_eq!(HttpRequest::parse(b"GET\r\n\r\n"), Err(ParseError::BadRequestLine));
    }

    #[test]
    fn bad_content_length_is_rejected() {
        let r = req("GET / HTTP/1.1\r\nContent-Length: abc\r\n\r\n");
        assert_eq!(r.content_length(), Err(ParseError::BadContentLength));
    }

    #[test]
    fn response_serializes_status_headers_and_body() {
        let resp = HttpResponse::json(200, "{\"ok\":true}");
        let bytes = resp.to_bytes();
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "got: {text:?}");
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"));
    }

    #[test]
    fn reason_phrases_cover_documented_codes() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(405), "Method Not Allowed");
        assert_eq!(reason_phrase(500), "Internal Server Error");
    }
}
