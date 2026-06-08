// SPDX-License-Identifier: Apache-2.0
//! HTTP/1.1 message model + wire codec for the apiserver transport.
//!
//! Behavioural reference: RFC 9112 (HTTP/1.1 message syntax) and RFC 3986
//! (percent-encoding for query parameters). This is the *transport* layer that
//! the Kubernetes apiserver decision core was missing — a std-only request
//! parser + response serializer, including chunked transfer encoding for watch
//! streams. No external HTTP framework is pulled in; the socket loop lives in
//! [`crate::server`].

use std::fmt;

/// An HTTP request method. Unknown methods are preserved verbatim so the handler
/// can answer `405 Method Not Allowed` rather than mis-route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Method {
    /// `GET`
    Get,
    /// `POST`
    Post,
    /// `PUT`
    Put,
    /// `PATCH`
    Patch,
    /// `DELETE`
    Delete,
    /// `HEAD`
    Head,
    /// `OPTIONS`
    Options,
    /// Any other (verbatim) token.
    Other(String),
}

impl Method {
    /// Parse a method token.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => Method::Other(other.to_string()),
        }
    }

    /// The wire token for this method.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A case-insensitive header collection (insertion order preserved).
#[derive(Clone, Debug, Default)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    /// Empty header set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert/append a header (lowercased name on the wire). Duplicates are kept;
    /// [`Headers::get`] returns the first.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.push((name.into().to_ascii_lowercase(), value.into()));
    }

    /// Case-insensitive lookup of the first value for `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let want = name.to_ascii_lowercase();
        self.entries.iter().find(|(k, _)| *k == want).map(|(_, v)| v.as_str())
    }

    /// All values for `name`, in insertion order (case-insensitive). Used for
    /// repeatable headers such as `X-Remote-Group`.
    #[must_use]
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let want = name.to_ascii_lowercase();
        self.entries.iter().filter(|(k, _)| *k == want).map(|(_, v)| v.as_str()).collect()
    }

    /// Remove every value for `name` (case-insensitive). Returns the number of
    /// entries dropped. The TLS terminator uses this to strip client-supplied
    /// front-proxy headers before injecting verified ones.
    pub fn remove_all(&mut self, name: &str) -> usize {
        let want = name.to_ascii_lowercase();
        let before = self.entries.len();
        self.entries.retain(|(k, _)| *k != want);
        before - self.entries.len()
    }

    /// Iterate all (name, value) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// A failed HTTP parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpError {
    /// Human-readable reason.
    pub message: String,
}

impl HttpError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed HTTP message: {}", self.message)
    }
}

impl std::error::Error for HttpError {}

/// A parsed HTTP/1.1 request.
#[derive(Clone, Debug)]
pub struct Request {
    /// Request method.
    pub method: Method,
    /// Raw request target (path + optional `?query`).
    pub target: String,
    /// HTTP version token (e.g. `HTTP/1.1`).
    pub version: String,
    /// Request headers.
    pub headers: Headers,
    /// Request body (already de-framed by Content-Length).
    pub body: Vec<u8>,
}

impl Request {
    /// Parse a complete request message: request line, headers, then the body
    /// (everything after the blank line). The caller is responsible for having
    /// read `Content-Length` bytes of body off the socket before calling.
    ///
    /// # Errors
    /// [`HttpError`] for a missing/short request line or absent header
    /// terminator.
    pub fn parse(raw: &[u8]) -> Result<Self, HttpError> {
        let sep = find_subsequence(raw, b"\r\n\r\n")
            .ok_or_else(|| HttpError::new("no header terminator (CRLFCRLF)"))?;
        let head = std::str::from_utf8(&raw[..sep]).map_err(|_| HttpError::new("non-UTF8 head"))?;
        let body = raw[sep + 4..].to_vec();

        let mut lines = head.split("\r\n");
        let request_line = lines.next().ok_or_else(|| HttpError::new("empty request"))?;
        let mut parts = request_line.split(' ');
        let method = parts.next().ok_or_else(|| HttpError::new("no method"))?;
        let target = parts.next().ok_or_else(|| HttpError::new("no target"))?;
        let version = parts.next().ok_or_else(|| HttpError::new("no version"))?;
        if parts.next().is_some() {
            return Err(HttpError::new("malformed request line"));
        }

        let mut headers = Headers::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| HttpError::new("malformed header line"))?;
            headers.insert(k.trim(), v.trim());
        }

        Ok(Request {
            method: Method::parse(method),
            target: target.to_string(),
            version: version.to_string(),
            headers,
            body,
        })
    }

    /// The path portion of the target (before any `?`).
    #[must_use]
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    /// The raw query string (after the first `?`), empty if none.
    #[must_use]
    pub fn query(&self) -> &str {
        self.target.split_once('?').map(|(_, q)| q).unwrap_or("")
    }

    /// Decoded `(key, value)` query pairs (percent + `+` decoding).
    #[must_use]
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        let q = self.query();
        if q.is_empty() {
            return Vec::new();
        }
        q.split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| match kv.split_once('=') {
                Some((k, v)) => (percent_decode(k), percent_decode(v)),
                None => (percent_decode(kv), String::new()),
            })
            .collect()
    }

    /// The first decoded value for query parameter `key`.
    #[must_use]
    pub fn query_get(&self, key: &str) -> Option<String> {
        self.query_pairs().into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// An HTTP response to serialize.
#[derive(Clone, Debug)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Headers (a `content-length` is added automatically by [`Response::to_bytes`]).
    pub headers: Headers,
    /// Body bytes. For a chunked response this holds the already-framed chunks
    /// ([`encode_chunk`] output), not the raw payload.
    pub body: Vec<u8>,
    /// When set, the response is `transfer-encoding: chunked` (used for watch
    /// streams): [`Response::to_bytes`] emits the streaming head, then `body`,
    /// then the terminating [`last_chunk`], and never a `content-length`.
    pub chunked: bool,
}

impl Response {
    /// A response with the given status and no body.
    #[must_use]
    pub fn new(status: u16) -> Self {
        Self { status, headers: Headers::new(), body: Vec::new(), chunked: false }
    }

    /// A chunked `application/json` streaming response whose `body` already holds
    /// the framed chunks (see [`encode_chunk`]). Used for watch streams.
    #[must_use]
    pub fn chunked_json(framed_body: Vec<u8>) -> Self {
        let mut headers = Headers::new();
        headers.insert("content-type", "application/json");
        Self { status: 200, headers, body: framed_body, chunked: true }
    }

    /// Append a header (builder style).
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Set a `content-type` and body (builder style).
    #[must_use]
    pub fn with_body(mut self, content_type: &str, body: Vec<u8>) -> Self {
        self.headers.insert("content-type", content_type);
        self.body = body;
        self
    }

    /// Serialize the full response (status line, headers, auto `content-length`,
    /// blank line, body).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        if self.chunked {
            let mut out = self.streaming_head();
            out.extend_from_slice(&self.body);
            out.extend_from_slice(last_chunk());
            return out;
        }
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason_phrase(self.status))
            .into_bytes();
        for (k, v) in self.headers.iter() {
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(format!("content-length: {}\r\n", self.body.len()).as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }

    /// Serialize only the status line + headers for a *streaming* (chunked)
    /// response. Advertises `transfer-encoding: chunked` and omits
    /// `content-length`; the caller then writes [`encode_chunk`] frames followed
    /// by [`last_chunk`].
    #[must_use]
    pub fn streaming_head(&self) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason_phrase(self.status))
            .into_bytes();
        for (k, v) in self.headers.iter() {
            if k.eq_ignore_ascii_case("content-length") {
                continue;
            }
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"transfer-encoding: chunked\r\n\r\n");
        out
    }
}

/// Encode one chunk for `transfer-encoding: chunked`.
#[must_use]
pub fn encode_chunk(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

/// The terminating zero-length chunk.
#[must_use]
pub fn last_chunk() -> &'static [u8] {
    b"0\r\n\r\n"
}

/// The canonical HTTP reason phrase for a status code (the subset the apiserver
/// emits). Unknown codes fall back to a generic phrase by class.
#[must_use]
pub fn reason_phrase(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => match code / 100 {
            2 => "OK",
            4 => "Client Error",
            5 => "Server Error",
            _ => "Unknown",
        },
    }
}

/// Find the first index of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Decode an `application/x-www-form-urlencoded` token: `%XX` hex escapes and
/// `+` → space.
#[must_use]
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_get() {
        let raw = b"GET /api/v1/pods HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = Request::parse(raw).expect("parse");
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.target, "/api/v1/pods");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(req.headers.get("host"), Some("localhost"));
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_post_with_body() {
        let raw = b"POST /api/v1/namespaces/default/pods HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"kind\":\"x\"}\n";
        let req = Request::parse(raw).expect("parse");
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.headers.get("content-type"), Some("application/json"));
        assert_eq!(req.body, b"{\"kind\":\"x\"}\n");
    }

    #[test]
    fn header_get_all_and_remove_all() {
        let mut h = Headers::new();
        h.insert("X-Remote-Group", "system:masters");
        h.insert("x-remote-group", "dev");
        h.insert("x-remote-user", "alice");
        assert_eq!(h.get_all("X-Remote-Group"), vec!["system:masters", "dev"]);
        assert_eq!(h.remove_all("x-remote-group"), 2);
        assert!(h.get_all("x-remote-group").is_empty());
        assert_eq!(h.get("x-remote-user"), Some("alice"));
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let raw = b"GET / HTTP/1.1\r\nAuthorization: Bearer t0ken\r\n\r\n";
        let req = Request::parse(raw).expect("parse");
        assert_eq!(req.headers.get("AUTHORIZATION"), Some("Bearer t0ken"));
        assert_eq!(req.headers.get("authorization"), Some("Bearer t0ken"));
        assert_eq!(req.headers.get("missing"), None);
    }

    #[test]
    fn path_and_query_split() {
        let raw = b"GET /api/v1/pods?watch=true&resourceVersion=7 HTTP/1.1\r\n\r\n";
        let req = Request::parse(raw).expect("parse");
        assert_eq!(req.path(), "/api/v1/pods");
        assert_eq!(req.query(), "watch=true&resourceVersion=7");
    }

    #[test]
    fn query_pairs_percent_decoded() {
        let raw = b"GET /api/v1/pods?labelSelector=app%3Dweb%2Ctier%3Dfront&limit=5 HTTP/1.1\r\n\r\n";
        let req = Request::parse(raw).expect("parse");
        let pairs = req.query_pairs();
        assert_eq!(req.query_get("labelSelector").as_deref(), Some("app=web,tier=front"));
        assert_eq!(req.query_get("limit").as_deref(), Some("5"));
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn method_parse_and_unknown() {
        assert_eq!(Method::parse("PUT"), Method::Put);
        assert_eq!(Method::parse("DELETE"), Method::Delete);
        assert_eq!(Method::parse("PATCH"), Method::Patch);
        assert_eq!(Method::parse("WAT"), Method::Other("WAT".to_string()));
    }

    #[test]
    fn malformed_request_line_errors() {
        assert!(Request::parse(b"GET\r\n\r\n").is_err());
        assert!(Request::parse(b"").is_err());
        assert!(Request::parse(b"no crlf at all").is_err());
    }

    #[test]
    fn response_serializes_status_headers_body() {
        let resp = Response::new(200)
            .with_body("application/json", b"{\"ok\":true}".to_vec());
        let bytes = resp.to_bytes();
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "got: {text}");
        assert!(text.contains("content-type: application/json\r\n"));
        assert!(text.contains("content-length: 11\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"));
    }

    #[test]
    fn reason_phrases_match_http() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(201), "Created");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(422), "Unprocessable Entity");
        assert_eq!(reason_phrase(500), "Internal Server Error");
    }

    #[test]
    fn chunked_framing_round_trip() {
        // Each chunk: <hex-len>\r\n<data>\r\n ; terminated by 0\r\n\r\n.
        let c = encode_chunk(b"hello");
        assert_eq!(c, b"5\r\nhello\r\n");
        assert_eq!(last_chunk(), b"0\r\n\r\n");
    }

    #[test]
    fn chunked_response_to_bytes_streams_and_terminates() {
        let mut body = Vec::new();
        body.extend_from_slice(&encode_chunk(b"{\"type\":\"ADDED\"}\n"));
        let resp = Response::chunked_json(body);
        assert!(resp.chunked);
        let text = String::from_utf8(resp.to_bytes()).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("transfer-encoding: chunked\r\n"));
        assert!(text.contains("content-type: application/json\r\n"));
        assert!(text.ends_with("0\r\n\r\n"), "got tail: {text:?}");
        assert!(!text.to_ascii_lowercase().contains("content-length"));
    }

    #[test]
    fn streaming_response_headers_use_chunked() {
        let head = Response::new(200)
            .header("content-type", "application/json")
            .streaming_head();
        let text = String::from_utf8(head).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("transfer-encoding: chunked\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
        // A streaming head must NOT advertise a content-length.
        assert!(!text.to_ascii_lowercase().contains("content-length"));
    }
}
