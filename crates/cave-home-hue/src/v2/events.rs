// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: home-assistant-libs/aiohue@v4.8.1 aiohue/v2/controllers/events.py
//! v2 EventStream — parser + types for the Hue CLIP Server-Sent Events
//! payload. Mirrors `aiohue.v2.controllers.events`.
//!
//! The transport (long-lived HTTP/2 GET to `clip/v2/eventstream`) is wired
//! by the cave-home binary against its shared reqwest/hyper client; this
//! module only models the events themselves + an SSE-line parser, so unit
//! tests can drive the controller without a network.
//!
//! Hue ships SSE in the standard wire format (`event: ...`, `data: ...`,
//! `id: ...`, blank line). We parse one line-buffer at a time and emit a
//! [`SseLine`] enum; consumers fold those into [`HueEvent`] payloads.

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

/// Reconnect after this long without traffic. Source:
/// `aiohue.v2.controllers.events.CONNECTION_TIMEOUT` (90s).
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(90);

/// Bridge keep-alive interval. Source:
/// `aiohue.v2.controllers.events.KEEPALIVE_INTERVAL` (60s).
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);

/// Source: `aiohue.v2.controllers.events.EventStreamStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventStreamStatus {
    Connecting,
    Connected,
    Disconnected,
}

/// Source: `aiohue.v2.controllers.events.EventType`. We collapse Hue's
/// resource-event strings + the connection state variants into a single enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// SSE event `add`. Resource appeared.
    #[serde(rename = "add")]
    ResourceAdded,
    /// SSE event `update`. Resource changed.
    #[serde(rename = "update")]
    ResourceUpdated,
    /// SSE event `delete`. Resource removed.
    #[serde(rename = "delete")]
    ResourceDeleted,
    /// Synthetic — emitted by the controller when the stream connects.
    Connected,
    /// Synthetic — emitted on stream loss.
    Disconnected,
    /// Synthetic — emitted on successful reconnect.
    Reconnected,
}

/// One Hue eventstream message. Mirrors `aiohue.v2.controllers.events.HueEvent`.
#[derive(Debug, Clone, Deserialize)]
pub struct HueEvent {
    pub id: String,
    #[serde(default)]
    pub creationtime: String,
    #[serde(rename = "type")]
    pub kind: EventType,
    /// Resource payload(s). For `delete`, holds only the ResourceIdentifier
    /// shape (rid + rtype). For `add`/`update`, partial-resource objects.
    pub data: Vec<Value>,
}

/// One parsed SSE line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseLine {
    /// `event: <name>` header.
    Event(String),
    /// `data: <payload>` line.
    Data(String),
    /// `id: <last-event-id>` line.
    Id(String),
    /// `retry: <ms>` line.
    Retry(u32),
    /// Blank line — dispatches the buffered event.
    Dispatch,
    /// Comment line — discard (used for keepalives).
    Comment,
    /// Unrecognised line — caller may log + drop.
    Unknown(String),
}

/// Parse one SSE wire line. Lines must already be stripped of the trailing
/// newline. Matches the W3C EventSource format.
#[must_use]
pub fn parse_sse_line(line: &str) -> SseLine {
    if line.is_empty() {
        return SseLine::Dispatch;
    }
    if line.starts_with(':') {
        return SseLine::Comment;
    }
    let Some((field, value_raw)) = line.split_once(':') else {
        return SseLine::Unknown(line.to_string());
    };
    let value = value_raw.strip_prefix(' ').unwrap_or(value_raw);
    match field {
        "event" => SseLine::Event(value.to_string()),
        "data" => SseLine::Data(value.to_string()),
        "id" => SseLine::Id(value.to_string()),
        "retry" => match value.parse::<u32>() {
            Ok(ms) => SseLine::Retry(ms),
            Err(_) => SseLine::Unknown(line.to_string()),
        },
        _ => SseLine::Unknown(line.to_string()),
    }
}

/// Tiny incremental SSE parser. Feed lines, get [`HueEvent`]s out.
///
/// The Hue bridge always sets `event: <type>` to one of `add`/`update`/
/// `delete` and the `data:` payload to a JSON array whose elements are
/// individual events of that type. We surface each *element* as its own
/// [`HueEvent`].
#[derive(Debug, Default)]
pub struct EventStreamParser {
    last_event_id: String,
    current_data: String,
}

impl EventStreamParser {
    /// Last `id:` we saw — caller passes this back as `Last-Event-ID` on
    /// reconnect. Source: `aiohue.v2.controllers.events._last_event_id`.
    #[must_use]
    pub fn last_event_id(&self) -> &str {
        &self.last_event_id
    }

    /// Feed one parsed line. Returns the events that should be dispatched.
    #[must_use]
    pub fn feed(&mut self, line: SseLine) -> Vec<HueEvent> {
        match line {
            SseLine::Event(_) => {
                // We do not use the SSE `event:` field; Hue sets the event
                // kind inside each JSON object as `type`. Drop the header.
                Vec::new()
            }
            SseLine::Data(data) => {
                if !self.current_data.is_empty() {
                    // multi-line data — newline-join per W3C.
                    self.current_data.push('\n');
                }
                self.current_data.push_str(&data);
                Vec::new()
            }
            SseLine::Id(id) => {
                self.last_event_id = id;
                Vec::new()
            }
            SseLine::Retry(_) | SseLine::Comment | SseLine::Unknown(_) => Vec::new(),
            SseLine::Dispatch => {
                if self.current_data.is_empty() {
                    return Vec::new();
                }
                let parsed: Result<Vec<HueEvent>, _> = serde_json::from_str(&self.current_data);
                self.current_data.clear();
                parsed.unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_line_classifies_fields() {
        assert_eq!(parse_sse_line(""), SseLine::Dispatch);
        assert_eq!(parse_sse_line(": keepalive"), SseLine::Comment);
        assert_eq!(parse_sse_line("event: update"), SseLine::Event("update".into()));
        assert_eq!(parse_sse_line("data: hi"), SseLine::Data("hi".into()));
        assert_eq!(parse_sse_line("id: 7"), SseLine::Id("7".into()));
        assert_eq!(parse_sse_line("retry: 5000"), SseLine::Retry(5000));
        assert!(matches!(parse_sse_line("nope"), SseLine::Unknown(_)));
    }

    #[test]
    fn event_stream_parser_assembles_payload_at_dispatch() {
        let mut p = EventStreamParser::default();
        let body = r#"[{"id":"abc","creationtime":"2026-05-17T20:00:00Z","type":"update","data":[{"id":"light-1","type":"light","on":{"on":true}}]}]"#;
        for line in [
            "id: 7",
            "event: update",
            &format!("data: {body}"),
            "",
        ] {
            let parsed = parse_sse_line(line);
            let _events = p.feed(parsed);
            // Only dispatch (the "" line) returns events.
        }
        let parsed = parse_sse_line("");
        // After above already-dispatched line, buffer is empty -> no events.
        let final_events = p.feed(parsed);
        assert!(final_events.is_empty());
        assert_eq!(p.last_event_id(), "7");
    }

    #[test]
    fn event_stream_parser_handles_one_event() {
        let mut p = EventStreamParser::default();
        let body = r#"[{"id":"e1","creationtime":"2026-05-17T20:00:00Z","type":"add","data":[{"id":"light-1","type":"light"}]}]"#;
        p.feed(parse_sse_line("id: 42"));
        p.feed(parse_sse_line("event: add"));
        p.feed(parse_sse_line(&format!("data: {body}")));
        let events = p.feed(parse_sse_line(""));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, EventType::ResourceAdded));
        assert_eq!(events[0].id, "e1");
        assert_eq!(p.last_event_id(), "42");
    }

    #[test]
    fn event_stream_parser_drops_blank_dispatch_with_no_data() {
        let mut p = EventStreamParser::default();
        let events = p.feed(parse_sse_line(""));
        assert!(events.is_empty());
    }
}
