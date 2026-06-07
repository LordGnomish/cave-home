// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! A tiny, dependency-free HTTP server exposing the latest snapshot as
//! Prometheus metrics on `/metrics`.
//!
//! There is no async runtime and no HTTP crate: a `std::net::TcpListener`
//! handles one blocking connection at a time, which is plenty for a metrics
//! endpoint scraped every 15–60s. The routing is split into a pure [`route`]
//! function so it can be unit-tested without a socket.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::metrics::render_prometheus;
use crate::snapshot::Snapshot;

/// A minimal HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Reason phrase.
    pub reason: &'static str,
    /// `Content-Type` header value.
    pub content_type: &'static str,
    /// Response body.
    pub body: String,
}

impl HttpResponse {
    /// Serialise to HTTP/1.1 wire bytes.
    #[must_use]
    pub fn to_wire(&self) -> Vec<u8> {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            self.reason,
            self.content_type,
            self.body.len(),
        );
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(self.body.as_bytes());
        bytes
    }
}

/// Pure router: map a `method`/`path` plus the current snapshot to a response.
#[must_use]
pub fn route(method: &str, path: &str, snap: Option<&Snapshot>) -> HttpResponse {
    if method != "GET" {
        return HttpResponse {
            status: 405,
            reason: "Method Not Allowed",
            content_type: "text/plain; charset=utf-8",
            body: "method not allowed\n".to_owned(),
        };
    }
    // Strip any query string.
    let path = path.split('?').next().unwrap_or(path);
    match path {
        "/metrics" => HttpResponse {
            status: 200,
            reason: "OK",
            content_type: "text/plain; version=0.0.4; charset=utf-8",
            body: snap.map_or_else(
                || "# no snapshot available; run `cave-home-tracker measure`\n".to_owned(),
                render_prometheus,
            ),
        },
        "/healthz" => HttpResponse {
            status: 200,
            reason: "OK",
            content_type: "text/plain; charset=utf-8",
            body: "ok\n".to_owned(),
        },
        "/" => HttpResponse {
            status: 200,
            reason: "OK",
            content_type: "text/plain; charset=utf-8",
            body: snap.map_or_else(
                || "cave-home-tracker — no snapshot yet\n".to_owned(),
                |s| {
                    format!(
                        "cave-home-tracker\nproject: {}\ndate: {}\noverall: {:.1}%\nsubsystems: {}\n\nmetrics: /metrics\n",
                        s.project,
                        s.date,
                        s.overall_real_pct(),
                        s.subsystems.len(),
                    )
                },
            ),
        },
        _ => HttpResponse {
            status: 404,
            reason: "Not Found",
            content_type: "text/plain; charset=utf-8",
            body: "not found\n".to_owned(),
        },
    }
}

/// Read just the request line (`METHOD PATH HTTP/1.1`) and drain headers.
fn read_request(stream: &TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    // Drain remaining headers so the client is not left hanging.
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(_) if h == "\r\n" || h == "\n" => break,
            Ok(_) => {}
        }
    }
    Some((method, path))
}

/// Handle a single connection with the snapshot from `provider`.
fn handle_connection<P>(mut stream: TcpStream, provider: &P)
where
    P: Fn() -> Option<Snapshot>,
{
    let resp = match read_request(&stream) {
        Some((method, path)) => {
            let snap = provider();
            route(&method, &path, snap.as_ref())
        }
        None => HttpResponse {
            status: 400,
            reason: "Bad Request",
            content_type: "text/plain; charset=utf-8",
            body: "bad request\n".to_owned(),
        },
    };
    let _ = stream.write_all(&resp.to_wire());
    let _ = stream.flush();
}

/// Serve metrics forever on `addr`, fetching the latest snapshot via `provider`
/// on every request (so a fresh `measure` is picked up without a restart).
///
/// # Errors
/// Returns an error if the address cannot be bound.
pub fn serve<A, P>(addr: A, provider: P) -> crate::Result<()>
where
    A: ToSocketAddrs,
    P: Fn() -> Option<Snapshot>,
{
    let listener = TcpListener::bind(addr)?;
    serve_with(&listener, &provider, None);
    Ok(())
}

/// Accept connections on an existing `listener`. When `limit` is `Some(n)`, the
/// loop returns after handling `n` connections (used by tests); `None` serves
/// forever.
pub fn serve_with<P>(listener: &TcpListener, provider: &P, limit: Option<usize>)
where
    P: Fn() -> Option<Snapshot>,
{
    let mut handled = 0usize;
    for stream in listener.incoming() {
        match stream {
            Ok(s) => handle_connection(s, provider),
            Err(_) => continue,
        }
        handled += 1;
        if limit.is_some_and(|n| handled >= n) {
            break;
        }
    }
}

/// Issue a single blocking GET to `addr` for `path`, returning the raw response
/// (helper for tests and the `dashboard --probe` flow).
///
/// # Errors
/// Returns an error if the connection or read fails.
pub fn http_get(addr: &str, path: &str) -> crate::Result<String> {
    let mut stream = TcpStream::connect(addr)?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes())?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SubsystemMetric;
    use crate::stubs::StubCount;
    use std::thread;

    fn snap() -> Snapshot {
        Snapshot {
            project: "cave-home".into(),
            date: "2026-06-07".into(),
            generated_at: "2026-06-07T06:00:00Z".into(),
            subsystems: vec![SubsystemMetric::derive(
                "kine",
                "k3s",
                1000,
                500,
                true,
                8,
                0,
                0,
                StubCount::default(),
            )],
        }
    }

    #[test]
    fn route_metrics_renders_prometheus() {
        let s = snap();
        let r = route("GET", "/metrics", Some(&s));
        assert_eq!(r.status, 200);
        assert!(r.body.contains("cave_home_tracker_real_pct"));
    }

    #[test]
    fn route_metrics_without_snapshot() {
        let r = route("GET", "/metrics", None);
        assert_eq!(r.status, 200);
        assert!(r.body.contains("no snapshot"));
    }

    #[test]
    fn route_strips_query_and_handles_unknown() {
        assert_eq!(route("GET", "/healthz?x=1", None).status, 200);
        assert_eq!(route("GET", "/nope", None).status, 404);
        assert_eq!(route("POST", "/metrics", None).status, 405);
    }

    #[test]
    fn wire_format_has_status_and_length() {
        let wire = String::from_utf8(route("GET", "/healthz", None).to_wire()).unwrap();
        assert!(wire.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(wire.contains("Content-Length: 3"));
        assert!(wire.ends_with("ok\n"));
    }

    #[test]
    fn end_to_end_over_a_real_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            serve_with(&listener, &snap_provider, Some(1));
        });
        let resp = http_get(&addr, "/metrics").unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("cave_home_tracker_overall_real_pct"));
        handle.join().unwrap();
    }

    fn snap_provider() -> Option<Snapshot> {
        Some(snap())
    }
}
