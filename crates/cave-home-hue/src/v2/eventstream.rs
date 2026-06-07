// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@394aa9394838841bbd5358d78edc140766db127c aiohue/v2/controllers/events.py
//! Live Server-Sent Events client for the Hue CLIP v2 EventStream.
//!
//! The bridge pushes state changes over a long-lived `GET
//! /eventstream/clip/v2` (`Accept: text/event-stream`). This module turns that
//! socket into a stream of typed [`HueEvent`]s:
//!
//! 1. [`LineBuffer`] reframes arbitrarily-chunked bytes into SSE lines,
//! 2. [`super::events::parse_sse_line`] classifies each line,
//! 3. [`super::events::EventStreamParser`] folds them into [`HueEvent`]s,
//! 4. [`EventStream`] runs the reqwest request + reconnect-with-`Last-Event-ID`
//!    loop and forwards events to a tokio channel.
//!
//! Mirrors `aiohue.v2.controllers.events.EventStream`; gated behind `runtime`.

use crate::errors::{HueError, HueResult};
use crate::v2::events::{EventStreamParser, HueEvent, SseLine, parse_sse_line};
use crate::v2::transport::{APP_KEY_HEADER, ReqwestTransport};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;

/// Default pause before re-opening a dropped stream. Source:
/// `aiohue` retries the `EventStream` connection; HA backs off a few seconds.
pub const DEFAULT_RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Channel depth for delivered events.
const EVENT_CHANNEL_DEPTH: usize = 64;

/// Reframes arbitrarily-chunked SSE bytes into newline-delimited lines.
///
/// SSE is line-oriented but the transport delivers bytes in chunks that can
/// split a line anywhere. This buffer accumulates bytes and emits each complete
/// line (newline stripped, plus a trailing `\r` per the W3C `EventSource` spec),
/// retaining any partial trailing line for the next push.
#[derive(Debug, Default)]
pub struct LineBuffer {
    buf: String,
}

impl LineBuffer {
    /// Append `bytes` and return any newly-completed lines.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(idx) = self.buf.find('\n') {
            let mut line = self.buf[..idx].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            out.push(line);
            self.buf.drain(..=idx);
        }
        out
    }
}

/// A live `EventStream` connection to one bridge, with reconnect state.
#[derive(Debug, Clone)]
pub struct EventStream {
    transport: ReqwestTransport,
    last_event_id: String,
    reconnect_delay: Duration,
}

impl EventStream {
    /// Wrap a transport. The stream shares the transport's reqwest client + key.
    #[must_use]
    pub const fn new(transport: ReqwestTransport) -> Self {
        Self {
            transport,
            last_event_id: String::new(),
            reconnect_delay: DEFAULT_RECONNECT_DELAY,
        }
    }

    /// Resume from a known `Last-Event-ID` (e.g. across a restart).
    #[must_use]
    pub fn with_last_event_id(mut self, id: impl Into<String>) -> Self {
        self.last_event_id = id.into();
        self
    }

    /// Override the reconnect backoff (mainly for tests).
    #[must_use]
    pub const fn with_reconnect_delay(mut self, delay: Duration) -> Self {
        self.reconnect_delay = delay;
        self
    }

    /// Open the stream once, forwarding every [`HueEvent`] to `tx` until the
    /// connection closes. Returns the last event id seen so the caller can
    /// resume. Source: `aiohue.v2.controllers.events.EventStream.__event_reader`.
    pub async fn connect_once(&self, tx: &mpsc::Sender<HueEvent>) -> HueResult<String> {
        let url = self.transport.eventstream_url();
        let mut builder = self
            .transport
            .client()
            .get(&url)
            .header(APP_KEY_HEADER, self.transport.app_key())
            .header(reqwest::header::ACCEPT, "text/event-stream");
        if !self.last_event_id.is_empty() {
            builder = builder.header("Last-Event-ID", &self.last_event_id);
        }

        let resp = builder
            .send()
            .await
            .map_err(|e| HueError::Transport(format!("eventstream {url}: {e}")))?;
        if !resp.status().is_success() {
            return Err(HueError::Transport(format!(
                "eventstream HTTP {}",
                resp.status().as_u16()
            )));
        }

        let mut stream = resp.bytes_stream();
        let mut lines = LineBuffer::default();
        let mut parser = EventStreamParser::default();
        let mut last_id = self.last_event_id.clone();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| HueError::Transport(format!("stream read: {e}")))?;
            for raw in lines.push(chunk.as_ref()) {
                let parsed = parse_sse_line(&raw);
                if let SseLine::Id(id) = &parsed {
                    last_id.clone_from(id);
                }
                for event in parser.feed(parsed) {
                    if tx.send(event).await.is_err() {
                        // Receiver dropped — stop reading this connection.
                        return Ok(last_id);
                    }
                }
            }
        }
        if !parser.last_event_id().is_empty() {
            last_id = parser.last_event_id().to_string();
        }
        Ok(last_id)
    }

    /// Spawn the reconnect loop on the tokio runtime. Returns a receiver of
    /// events plus the task handle. The loop reconnects (carrying the last
    /// event id) with [`Self::reconnect_delay`] backoff, and exits once the
    /// receiver is dropped. Source: `aiohue` `EventStream.initialize` loop.
    #[must_use]
    pub fn spawn(self) -> (mpsc::Receiver<HueEvent>, tokio::task::JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_DEPTH);
        let handle = tokio::spawn(async move {
            let mut stream = self;
            loop {
                match stream.connect_once(&tx).await {
                    Ok(last) => stream.last_event_id = last,
                    Err(err) => {
                        tracing::debug!(target: "cave_home_hue::eventstream", error = %err, "eventstream dropped");
                    }
                }
                if tx.is_closed() {
                    break;
                }
                tokio::time::sleep(stream.reconnect_delay).await;
                if tx.is_closed() {
                    break;
                }
            }
        });
        (rx, handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::events::EventType;
    use crate::v2::test_support::{http_sse, spawn_mock};
    use crate::v2::transport::ReqwestTransport;

    #[test]
    fn line_buffer_splits_on_newlines_and_keeps_partial() {
        let mut lb = LineBuffer::default();
        assert_eq!(lb.push(b"id: 1\nda"), vec!["id: 1".to_string()]);
        assert_eq!(lb.push(b"ta: x\n\n"), vec!["data: x".to_string(), String::new()]);
        assert!(lb.push(b"partial").is_empty());
    }

    #[test]
    fn line_buffer_strips_trailing_cr() {
        let mut lb = LineBuffer::default();
        assert_eq!(lb.push(b"event: add\r\n"), vec!["event: add".to_string()]);
    }

    #[tokio::test]
    async fn connect_once_emits_events_from_a_real_sse_body() {
        let payload = r#"[{"id":"e1","creationtime":"2026-06-07T20:00:00Z","type":"update","data":[{"id":"light-1","type":"light","on":{"on":true}}]}]"#;
        let sse = format!("id: 7\nevent: update\ndata: {payload}\n\n");
        let (base, caps) = spawn_mock(vec![http_sse(&sse)]).await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();

        let es = EventStream::new(t);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let last_id = es.connect_once(&tx).await.unwrap();
        drop(tx);

        let ev = rx.recv().await.expect("one event");
        assert_eq!(ev.kind, EventType::ResourceUpdated);
        assert_eq!(ev.id, "e1");
        assert_eq!(last_id, "7");

        let log = caps.lock().unwrap();
        let req = &log[0];
        assert_eq!(req.path, "/eventstream/clip/v2");
        assert_eq!(req.header("accept"), Some("text/event-stream"));
        assert_eq!(req.header("hue-application-key"), Some("k"));
    }

    #[tokio::test]
    async fn resume_sends_last_event_id_header() {
        let (base, caps) = spawn_mock(vec![http_sse("data: []\n\n")]).await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();
        let es = EventStream::new(t).with_last_event_id("42");
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        es.connect_once(&tx).await.unwrap();
        let log = caps.lock().unwrap();
        assert_eq!(log[0].header("last-event-id"), Some("42"));
    }

    #[tokio::test]
    async fn spawn_reconnect_loop_delivers_then_stops_when_receiver_dropped() {
        let payload = r#"[{"id":"e9","creationtime":"t","type":"add","data":[{"id":"l1","type":"light"}]}]"#;
        let sse = format!("id: 9\ndata: {payload}\n\n");
        // Only one response is served; the reconnect attempt afterward fails,
        // and once the receiver is dropped the loop exits.
        let (base, _caps) = spawn_mock(vec![http_sse(&sse)]).await;
        let t = ReqwestTransport::with_base_url(base, "k").unwrap();
        let es = EventStream::new(t).with_reconnect_delay(std::time::Duration::from_millis(10));

        let (mut rx, handle) = es.spawn();
        let ev = rx.recv().await.expect("event delivered by loop");
        assert_eq!(ev.id, "e9");
        drop(rx);
        // The background task should observe the closed channel and finish.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
    }
}
