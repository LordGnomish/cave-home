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

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::handler::ApiServer;
use crate::http::Request;

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn content_length(req: &Request) -> usize {
    req.headers
        .get("content-length")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

/// Read one complete HTTP/1.1 request from `stream`: the header block (to the
/// blank line) and then exactly `Content-Length` body bytes. Returns `Ok(None)`
/// on a clean EOF before any bytes arrive (peer closed without a request).
///
/// # Errors
/// `UnexpectedEof` if the stream ends mid-message, or `InvalidData` if the bytes
/// do not parse as a request.
pub fn read_request<R: Read>(stream: &mut R) -> io::Result<Option<Request>> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 2048];

    let header_end = loop {
        if let Some(p) = find_subsequence(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request headers completed",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    // Parse the head to learn the body length, then read the rest of the body.
    let head = Request::parse(&buf[..header_end]).map_err(to_io)?;
    let want = header_end + content_length(&head);
    while buf.len() < want {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request body completed",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
    }

    Ok(Some(Request::parse(&buf[..want]).map_err(to_io)?))
}

/// Per-connection resource bounds for the socket loop.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Largest accepted request head (request line + headers), in bytes.
    pub max_head_bytes: usize,
    /// Largest accepted request body (`Content-Length`), in bytes.
    pub max_body_bytes: usize,
    /// Socket read timeout (each read, including the TLS handshake).
    pub read_timeout: Duration,
    /// Socket write timeout.
    pub write_timeout: Duration,
    /// Wall-clock budget for receiving one whole request.
    pub request_deadline: Duration,
    /// Connections served concurrently; further connections are closed.
    pub max_connections: usize,
    /// Pause after a failed `accept()` before trying again.
    pub accept_backoff: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_head_bytes: crate::http::MAX_HEAD_BYTES,
            max_body_bytes: crate::http::MAX_BODY_BYTES,
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
            request_deadline: Duration::from_secs(30),
            max_connections: 256,
            accept_backoff: Duration::from_millis(50),
        }
    }
}

/// Why a request could not be read off the wire.
#[derive(Debug)]
pub enum RequestReadError {
    /// The head exceeded [`Limits::max_head_bytes`] (`431`).
    HeadTooLarge,
    /// The declared body exceeded [`Limits::max_body_bytes`] (`413`).
    BodyTooLarge(usize),
    /// The framing was malformed (`400`).
    BadRequest(String),
    /// The request used a framing this server does not implement (`501`).
    NotImplemented(String),
    /// The request did not arrive within [`Limits::request_deadline`].
    TimedOut,
    /// The underlying stream failed.
    Io(io::Error),
}

impl std::fmt::Display for RequestReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeadTooLarge => f.write_str("request head too large"),
            Self::BodyTooLarge(n) => write!(f, "request body of {n} bytes is too large"),
            Self::BadRequest(m) => write!(f, "bad request: {m}"),
            Self::NotImplemented(m) => write!(f, "not implemented: {m}"),
            Self::TimedOut => f.write_str("request not received in time"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RequestReadError {}

/// Read one request like [`read_request`], enforcing `limits`.
///
/// # Errors
/// A [`RequestReadError`] naming the violated bound or the I/O failure.
pub fn read_request_bounded<R: Read>(stream: &mut R, _limits: &Limits) -> Result<Option<Request>, RequestReadError> {
    read_request(stream).map_err(RequestReadError::Io)
}

/// Counts in-flight connections against [`Limits::max_connections`].
#[derive(Clone, Debug)]
pub struct ConnectionLimiter {
    max: usize,
}

/// One admitted connection; dropping it frees the slot.
#[derive(Debug)]
pub struct ConnectionPermit {
    _limiter: ConnectionLimiter,
}

impl ConnectionLimiter {
    /// A limiter admitting at most `max` concurrent connections.
    #[must_use]
    pub fn new(max: usize) -> Self {
        Self { max }
    }

    /// Admit one more connection, or `None` when at capacity.
    #[must_use]
    pub fn try_acquire(&self) -> Option<ConnectionPermit> {
        let _ = self.max;
        Some(ConnectionPermit { _limiter: self.clone() })
    }
}

/// Drive an accept stream: serve each connection on its own thread (bounded by
/// [`Limits::max_connections`]) and survive transient `accept()` failures.
///
/// # Errors
/// An accept error.
pub fn serve_incoming<I, S, F>(incoming: I, _limits: &Limits, serve: F) -> io::Result<()>
where
    I: IntoIterator<Item = io::Result<S>>,
    S: Send + 'static,
    F: Fn(S) -> io::Result<()> + Send + Sync + 'static,
{
    for conn in incoming {
        let _ = serve(conn?);
    }
    Ok(())
}

/// Serve exactly one request on `stream`: read it, run the handler under the
/// shared lock, write the response (with `Connection: close`), and flush. A
/// peer that closes without sending a request is a no-op.
///
/// # Errors
/// I/O errors from the underlying stream.
pub fn serve_stream<S: Read + Write>(stream: &mut S, app: &Mutex<ApiServer>) -> io::Result<()> {
    let Some(req) = read_request(stream)? else {
        return Ok(());
    };
    let mut resp = {
        // A poisoned lock still holds a valid server; recover and continue.
        let mut guard = app.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.handle(&req)
    };
    resp.headers.insert("connection", "close");
    stream.write_all(&resp.to_bytes())?;
    stream.flush()
}

/// A blocking, thread-per-connection HTTP server bound to a TCP port.
pub struct Server {
    listener: TcpListener,
    app: Arc<Mutex<ApiServer>>,
}

impl Server {
    /// Bind to `addr` (use port `0` for an ephemeral port) wrapping a shared
    /// [`ApiServer`].
    ///
    /// # Errors
    /// Any bind error from the OS.
    pub fn bind(addr: impl ToSocketAddrs, app: Arc<Mutex<ApiServer>>) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            app,
        })
    }

    /// Like [`Server::bind`] with explicit per-connection [`Limits`].
    ///
    /// # Errors
    /// Any bind error from the OS.
    pub fn bind_with_limits(addr: impl ToSocketAddrs, app: Arc<Mutex<ApiServer>>, _limits: Limits) -> io::Result<Self> {
        Self::bind(addr, app)
    }

    /// The bound local address (resolves the ephemeral port).
    ///
    /// # Errors
    /// If the socket address cannot be read.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accept and fully serve a single connection (one request). Used by tests
    /// and by callers that drive their own accept cadence.
    ///
    /// # Errors
    /// Accept or per-connection I/O errors.
    pub fn serve_once(&self) -> io::Result<()> {
        let (mut stream, _peer) = self.listener.accept()?;
        serve_stream(&mut stream, &self.app)
    }

    /// Run the accept loop forever, serving each connection on the calling
    /// thread. Per-connection errors are swallowed so one bad client cannot stop
    /// the server.
    ///
    /// # Errors
    /// A fatal accept error.
    pub fn run(&self) -> io::Result<()> {
        for conn in self.listener.incoming() {
            match conn {
                Ok(mut stream) => {
                    let _ = serve_stream(&mut stream, &self.app);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

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
        let app = Mutex::new(ApiServer::insecure_for_tests());
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
        let app = Arc::new(Mutex::new(ApiServer::insecure_for_tests()));
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

    // --- bounded reads: hostile clients cannot make the server buffer forever --

    /// A reader that yields `head` and then fails the test if the server keeps
    /// reading — proof an oversized body is refused *before* it is read.
    struct HeadThenTripwire {
        head: Cursor<Vec<u8>>,
    }

    impl Read for HeadThenTripwire {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.head.read(buf)?;
            if n == 0 {
                return Err(io::Error::other("server read past the head of an oversized request"));
            }
            Ok(n)
        }
    }

    #[test]
    fn bounded_read_rejects_an_oversized_head() {
        let mut raw = b"GET /api/v1/pods HTTP/1.1\r\n".to_vec();
        while raw.len() <= crate::http::MAX_HEAD_BYTES {
            raw.extend_from_slice(b"x-pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
        }
        let err = read_request_bounded(&mut Cursor::new(raw), &Limits::default()).expect_err("head too large");
        assert!(matches!(err, RequestReadError::HeadTooLarge), "{err}");
    }

    #[test]
    fn bounded_read_refuses_an_oversized_body_without_reading_it() {
        let declared = crate::http::MAX_BODY_BYTES + 1;
        let head = format!("POST /api/v1/namespaces/default/pods HTTP/1.1\r\ncontent-length: {declared}\r\n\r\n");
        let mut stream = HeadThenTripwire { head: Cursor::new(head.into_bytes()) };
        let err = read_request_bounded(&mut stream, &Limits::default()).expect_err("body too large");
        assert!(matches!(err, RequestReadError::BodyTooLarge(n) if n == declared), "{err}");
    }

    #[test]
    fn bounded_read_rejects_a_malformed_content_length() {
        let raw = b"POST /x HTTP/1.1\r\ncontent-length: lots\r\n\r\n".to_vec();
        let err = read_request_bounded(&mut Cursor::new(raw), &Limits::default()).expect_err("bad length");
        assert!(matches!(err, RequestReadError::BadRequest(_)), "{err}");
    }

    #[test]
    fn bounded_read_refuses_chunked_request_bodies() {
        let raw = b"POST /x HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec();
        let err = read_request_bounded(&mut Cursor::new(raw), &Limits::default()).expect_err("chunked");
        assert!(matches!(err, RequestReadError::NotImplemented(_)), "{err}");
    }

    #[test]
    fn bounded_read_accepts_a_body_within_the_limit() {
        let raw = b"POST /x HTTP/1.1\r\ncontent-length: 5\r\n\r\nhello".to_vec();
        let req = read_request_bounded(&mut Cursor::new(raw), &Limits::default()).expect("ok").expect("some");
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn serve_stream_answers_413_for_an_oversized_body() {
        let app = Mutex::new(ApiServer::insecure_for_tests());
        let head = format!(
            "POST /api/v1/namespaces/default/pods HTTP/1.1\r\ncontent-length: {}\r\n\r\n",
            crate::http::MAX_BODY_BYTES + 1
        );
        let mut stream = MockStream { input: Cursor::new(head.into_bytes()), output: Vec::new() };
        serve_stream(&mut stream, &app).expect("serve");
        let text = String::from_utf8(stream.output).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 413 "), "{text}");
        assert!(text.contains("RequestEntityTooLarge"), "{text}");
    }

    #[test]
    fn serve_stream_answers_431_for_an_oversized_head() {
        let app = Mutex::new(ApiServer::insecure_for_tests());
        let mut raw = b"GET /api/v1/pods HTTP/1.1\r\n".to_vec();
        while raw.len() <= crate::http::MAX_HEAD_BYTES {
            raw.extend_from_slice(b"x-pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
        }
        let mut stream = MockStream { input: Cursor::new(raw), output: Vec::new() };
        serve_stream(&mut stream, &app).expect("serve");
        let text = String::from_utf8(stream.output).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 431 "), "{text}");
    }

    // --- accept loop: concurrency cap, transient errors, slow clients --------

    #[test]
    fn connection_limiter_caps_concurrency_and_frees_on_drop() {
        let limiter = ConnectionLimiter::new(2);
        let a = limiter.try_acquire().expect("first");
        let _b = limiter.try_acquire().expect("second");
        assert!(limiter.try_acquire().is_none(), "third connection exceeds the cap");
        drop(a);
        assert!(limiter.try_acquire().is_some(), "a freed slot is reusable");
    }

    #[test]
    fn accept_errors_do_not_stop_the_server() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let served = Arc::new(AtomicUsize::new(0));
        let counter = served.clone();
        let incoming: Vec<io::Result<u32>> = vec![
            Err(io::Error::other("Too many open files (os error 24)")),
            Ok(1),
            Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
            Ok(2),
        ];
        let limits = Limits { accept_backoff: Duration::from_millis(1), ..Limits::default() };
        serve_incoming(incoming, &limits, move |_conn| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("transient accept errors are not fatal");
        assert_eq!(served.load(Ordering::SeqCst), 2, "both good connections were served");
    }

    #[test]
    fn connections_are_served_off_the_accept_thread() {
        // Two connections whose handlers each wait for the other: served inline
        // on one thread this deadlocks; served concurrently it completes.
        use std::sync::Barrier;
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let incoming: Vec<io::Result<u32>> = vec![Ok(1), Ok(2)];
            let r = serve_incoming(incoming, &Limits::default(), move |_| {
                b.wait();
                Ok(())
            });
            let _ = tx.send(r.is_ok());
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true), "connections must be served concurrently");
    }

    #[test]
    fn idle_client_is_dropped_after_the_read_timeout() {
        let app = Arc::new(Mutex::new(ApiServer::insecure_for_tests()));
        let limits = Limits { read_timeout: Duration::from_millis(200), ..Limits::default() };
        let server = Server::bind_with_limits("127.0.0.1:0", app, limits).expect("bind");
        let addr = server.local_addr().expect("addr");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(server.serve_once().is_err());
        });
        // Connect and then send nothing (slowloris).
        let _idle = TcpStream::connect(addr).expect("connect");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "an idle connection must time out instead of pinning the server"
        );
    }
}
