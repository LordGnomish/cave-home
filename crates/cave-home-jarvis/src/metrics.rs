// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — the Prometheus metrics for the voice-assistant pipeline.
//!
//! [`Metrics`] is a tiny in-process registry: wake firings, transcripts,
//! dispatches split by path (NLU vs. LLM), tool executions and failures, and
//! speaker identifications. It renders the Prometheus text exposition format
//! directly (no client library), matching the convention used elsewhere in
//! cave-home.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use parking_lot::Mutex;

use crate::dispatch::DispatchPath;

#[derive(Debug, Default)]
struct Inner {
    // keyword -> wake count
    wakes: BTreeMap<String, u64>,
    transcripts: u64,
    dispatch_nlu: u64,
    dispatch_llm: u64,
    // tool name -> (calls, failures)
    tools: BTreeMap<String, (u64, u64)>,
    // speaker name -> identifications ("" for unknown)
    speakers: BTreeMap<String, u64>,
    llm_turns: u64,
}

/// The voice-assistant metric registry.
#[derive(Debug, Default)]
pub struct Metrics {
    inner: Mutex<Inner>,
}

#[allow(clippy::significant_drop_tightening)]
impl Metrics {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one wake-word firing for `keyword`.
    pub fn record_wake(&self, keyword: &str) {
        *self.inner.lock().wakes.entry(keyword.to_string()).or_insert(0) += 1;
    }

    /// Record one successful transcription.
    pub fn record_transcript(&self) {
        self.inner.lock().transcripts += 1;
    }

    /// Record one dispatch, split by the path that handled it.
    pub fn record_dispatch(&self, path: DispatchPath) {
        let mut i = self.inner.lock();
        match path {
            DispatchPath::Nlu => i.dispatch_nlu += 1,
            DispatchPath::Llm => i.dispatch_llm += 1,
        }
    }

    /// Record one tool execution and whether it failed.
    pub fn record_tool(&self, name: &str, failed: bool) {
        let mut i = self.inner.lock();
        let e = i.tools.entry(name.to_string()).or_insert((0, 0));
        e.0 += 1;
        if failed {
            e.1 += 1;
        }
    }

    /// Record one speaker identification (empty name = unrecognised).
    pub fn record_speaker(&self, name: &str) {
        *self.inner.lock().speakers.entry(name.to_string()).or_insert(0) += 1;
    }

    /// Record one LLM chat turn (a request/response round to the model).
    pub fn record_llm_turn(&self) {
        self.inner.lock().llm_turns += 1;
    }

    /// Render the registry as Prometheus text exposition.
    #[must_use]
    pub fn render(&self) -> String {
        let i = self.inner.lock();
        let mut out = String::new();

        out.push_str("# HELP jarvis_wake_total Wake-word firings by keyword.\n");
        out.push_str("# TYPE jarvis_wake_total counter\n");
        for (kw, n) in &i.wakes {
            let _ = writeln!(out, "jarvis_wake_total{{keyword=\"{kw}\"}} {n}");
        }

        out.push_str("# HELP jarvis_transcripts_total Successful transcriptions.\n");
        out.push_str("# TYPE jarvis_transcripts_total counter\n");
        let _ = writeln!(out, "jarvis_transcripts_total {}", i.transcripts);

        out.push_str("# HELP jarvis_dispatch_total Dispatches by path.\n");
        out.push_str("# TYPE jarvis_dispatch_total counter\n");
        let _ = writeln!(out, "jarvis_dispatch_total{{path=\"nlu\"}} {}", i.dispatch_nlu);
        let _ = writeln!(out, "jarvis_dispatch_total{{path=\"llm\"}} {}", i.dispatch_llm);

        out.push_str("# HELP jarvis_llm_turns_total Chat turns to the local model.\n");
        out.push_str("# TYPE jarvis_llm_turns_total counter\n");
        let _ = writeln!(out, "jarvis_llm_turns_total {}", i.llm_turns);

        out.push_str("# HELP jarvis_tool_calls_total Tool executions by name.\n");
        out.push_str("# TYPE jarvis_tool_calls_total counter\n");
        for (name, (calls, _)) in &i.tools {
            let _ = writeln!(out, "jarvis_tool_calls_total{{tool=\"{name}\"}} {calls}");
        }

        out.push_str("# HELP jarvis_tool_failures_total Failed tool executions by name.\n");
        out.push_str("# TYPE jarvis_tool_failures_total counter\n");
        for (name, (_, fails)) in &i.tools {
            let _ = writeln!(out, "jarvis_tool_failures_total{{tool=\"{name}\"}} {fails}");
        }

        out.push_str("# HELP jarvis_speaker_id_total Speaker identifications by member.\n");
        out.push_str("# TYPE jarvis_speaker_id_total counter\n");
        for (name, n) in &i.speakers {
            let label = if name.is_empty() { "unknown" } else { name.as_str() };
            let _ = writeln!(out, "jarvis_speaker_id_total{{member=\"{label}\"}} {n}");
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counters_in_exposition_format() {
        let m = Metrics::new();
        m.record_wake("jarvis");
        m.record_wake("jarvis");
        m.record_transcript();
        m.record_dispatch(DispatchPath::Nlu);
        m.record_dispatch(DispatchPath::Llm);
        m.record_llm_turn();
        m.record_tool("set_light", false);
        m.record_tool("set_cover", true);
        m.record_speaker("Burak");
        m.record_speaker("");

        let out = m.render();
        assert!(out.contains("jarvis_wake_total{keyword=\"jarvis\"} 2"));
        assert!(out.contains("jarvis_transcripts_total 1"));
        assert!(out.contains("jarvis_dispatch_total{path=\"nlu\"} 1"));
        assert!(out.contains("jarvis_dispatch_total{path=\"llm\"} 1"));
        assert!(out.contains("jarvis_llm_turns_total 1"));
        assert!(out.contains("jarvis_tool_calls_total{tool=\"set_light\"} 1"));
        assert!(out.contains("jarvis_tool_failures_total{tool=\"set_cover\"} 1"));
        assert!(out.contains("jarvis_speaker_id_total{member=\"Burak\"} 1"));
        assert!(out.contains("jarvis_speaker_id_total{member=\"unknown\"} 1"));
        // Every metric carries HELP/TYPE headers.
        assert_eq!(out.matches("# TYPE").count(), 7);
    }
}
