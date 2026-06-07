// SPDX-License-Identifier: Apache-2.0
//! The blocking socket loop that binds the [`crate::handler::ApiServer`] to a
//! real TCP listener — the transport's outermost layer.
//!
//! Behavioural reference: the HTTP/1.1 connection lifecycle (read one request,
//! dispatch, write one response, close). This is a std-only,
//! thread-per-connection server: no async runtime and no external HTTP/TLS
//! crate. The connection handler ([`serve_stream`]) is generic over any
//! `Read + Write`, so a rustls `StreamOwned` (TLS termination) or an h2 framer
//! slots in front of it without touching the handler chain — TLS itself is
//! deferred (see `parity.manifest.toml`). HTTP keep-alive is also deferred: each
//! connection serves exactly one request and then closes (`Connection: close`).

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpStream;

    /// An in-memory duplex stream: reads drain `input`, writes accumulate in
    /// `output`. Lets us exercise [`serve_stream`] without a socket.
    struct MockStream {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }
    impl Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn read_request_parses_buffered_message() {
        let raw = b"POST /api/v1/namespaces/default/pods HTTP/1.1\r\ncontent-length: 5\r\n\r\nhello";
        let mut cur = Cursor::new(raw.to_vec());
        let req = read_request(&mut cur).expect("io").expect("some");
        assert_eq!(req.path(), "/api/v1/namespaces/default/pods");
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn read_request_clean_eof_is_none() {
        let mut cur = Cursor::new(Vec::new());
        assert!(read_request(&mut cur).expect("io").is_none());
    }

    #[test]
    fn serve_stream_writes_handler_response() {
        let app = Mutex::new(ApiServer::new());
        let raw = b"GET /healthz HTTP/1.1\r\n\r\n";
        let mut stream = MockStream { input: Cursor::new(raw.to_vec()), output: Vec::new() };
        serve_stream(&mut stream, &app).expect("serve");
        let text = String::from_utf8(stream.output).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.to_ascii_lowercase().contains("connection: close"));
        assert!(text.ends_with("ok"));
    }

    // --- real TCP integration: a kubectl-style REST session -----------------

    /// Open a fresh connection, send `raw`, and read the whole response to EOF.
    fn http_call(addr: SocketAddr, raw: &str) -> String {
        let mut conn = TcpStream::connect(addr).expect("connect");
        conn.write_all(raw.as_bytes()).expect("write");
        conn.flush().expect("flush");
        let mut resp = Vec::new();
        conn.read_to_end(&mut resp).expect("read");
        String::from_utf8_lossy(&resp).into_owned()
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nhost: test\r\n\r\n")
    }

    fn post(path: &str, body: &str) -> String {
        format!(
            "POST {path} HTTP/1.1\r\nhost: test\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn kubectl_style_rest_session_over_tcp() {
        let app = Arc::new(Mutex::new(ApiServer::new()));
        let server = Server::bind("127.0.0.1:0", app).expect("bind");
        let addr = server.local_addr().expect("addr");

        // Serve 5 connections (create, get, list, watch, delete) on a worker.
        let worker = std::thread::spawn(move || {
            for _ in 0..5 {
                server.serve_once().expect("serve_once");
            }
        });

        // 1. Create a pod (POST collection → 201).
        let pod = r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"nginx","namespace":"default"}}"#;
        let created = http_call(addr, &post("/api/v1/namespaces/default/pods", pod));
        assert!(created.starts_with("HTTP/1.1 201 Created\r\n"), "create: {created}");
        assert!(created.contains(r#""name":"nginx""#));
        assert!(created.contains(r#""resourceVersion":"1""#));

        // 2. Get it back (200).
        let got = http_call(addr, &get("/api/v1/namespaces/default/pods/nginx"));
        assert!(got.starts_with("HTTP/1.1 200 OK\r\n"), "get: {got}");
        assert!(got.contains(r#""name":"nginx""#));

        // 3. List the collection (200, PodList).
        let listed = http_call(addr, &get("/api/v1/namespaces/default/pods"));
        assert!(listed.contains(r#""kind":"PodList""#), "list: {listed}");

        // 4. Watch the collection from rv 0 (chunked stream with an ADDED event).
        let watched = http_call(addr, &get("/api/v1/namespaces/default/pods?watch=true&resourceVersion=0"));
        assert!(watched.contains("transfer-encoding: chunked"), "watch head: {watched}");
        assert!(watched.contains(r#""type":"ADDED""#), "watch body: {watched}");
        assert!(watched.trim_end().ends_with("0"), "watch terminator: {watched:?}");

        // 5. Delete it (200).
        let deleted = http_call(addr, &format!("DELETE /api/v1/namespaces/default/pods/nginx HTTP/1.1\r\nhost: test\r\n\r\n"));
        assert!(deleted.starts_with("HTTP/1.1 200 OK\r\n"), "delete: {deleted}");

        worker.join().expect("worker");
    }
}
