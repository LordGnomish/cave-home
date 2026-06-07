// SPDX-License-Identifier: Apache-2.0
//! HTTP/1.1 message model + wire codec for the apiserver transport.
//!
//! Behavioural reference: RFC 9112 (HTTP/1.1 message syntax) and RFC 3986
//! (percent-encoding for query parameters). This is the *transport* layer that
//! the Kubernetes apiserver decision core was missing — a std-only request
//! parser + response serializer, including chunked transfer encoding for watch
//! streams. No external HTTP framework is pulled in; the socket loop lives in
//! [`crate::server`].

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
