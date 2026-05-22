// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// CLEAN-ROOM: Philips Hue CLIP API v1+v2 public docs reference only.
// Upstream diyHue source NOT consulted. GPL contamination prevented by design.
//! `GET /clip/v2/eventstream` — Server-Sent Events feed.
//!
//! Reference: developer-portal "Core concepts → EventStream" — a persistent
//! HTTP/2 GET that emits SSE-formatted events. The bridge sends one SSE
//! message per change, where `data:` carries a JSON array of
//! `{id, creationtime, type, data: [...]}` objects.
//!
//! We render one [`StreamEvent`] as one SSE message; the cave-home binary
//! is responsible for keeping the connection alive (sending a `: keepalive\n\n`
//! every minute) — that's the only piece of this surface that doesn't fit
//! inside a pure function.

use crate::registry::StreamEvent;

/// Render one event to its wire-format SSE message (without the trailing
/// newline). Reference: published SSE format used by Hue v2 bridges.
#[must_use]
pub fn render_event(event: &StreamEvent) -> String {
    let id_line = format!("id: {}\n", event.id);
    let event_line = match event.kind {
        crate::registry::StreamEventKind::Add => "event: add\n",
        crate::registry::StreamEventKind::Update => "event: update\n",
        crate::registry::StreamEventKind::Delete => "event: delete\n",
    };
    let payload = serde_json::to_string(&[event]).unwrap_or_else(|_| "[]".into());
    let data_line = format!("data: {payload}\n");
    format!("{id_line}{event_line}{data_line}\n")
}

/// Render the keepalive comment per W3C SSE spec.
#[must_use]
pub const fn keepalive() -> &'static str {
    ": keepalive\n\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{StreamEvent, StreamEventKind};
    use serde_json::json;

    fn dummy(kind: StreamEventKind) -> StreamEvent {
        StreamEvent {
            id: "evt-1".into(),
            creationtime: "2026-05-17T20:00:00Z".into(),
            kind,
            data: vec![json!({"id": "light-1", "type": "light", "on": {"on": true}})],
        }
    }

    #[test]
    fn render_event_emits_three_required_sse_lines() {
        let s = render_event(&dummy(StreamEventKind::Update));
        let mut lines = s.lines();
        assert_eq!(lines.next().unwrap(), "id: evt-1");
        assert_eq!(lines.next().unwrap(), "event: update");
        let data_line = lines.next().unwrap();
        assert!(data_line.starts_with("data: "));
        // Trailing blank line per SSE spec.
        assert!(s.ends_with("\n\n"));
    }

    #[test]
    fn render_event_data_is_an_array_of_one() {
        let s = render_event(&dummy(StreamEventKind::Add));
        let prefix = "data: ";
        let line = s
            .lines()
            .find(|l| l.starts_with(prefix))
            .expect("data line");
        let json_text = &line[prefix.len()..];
        let arr: serde_json::Value = serde_json::from_str(json_text).unwrap();
        assert!(arr.is_array());
        assert_eq!(arr.as_array().unwrap().len(), 1);
        assert_eq!(arr[0].get("type").unwrap(), &json!("add"));
    }

    #[test]
    fn keepalive_is_w3c_sse_comment() {
        assert_eq!(keepalive(), ": keepalive\n\n");
    }
}
