# Coverage matrix — cave-home-audio-snapcast

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · clean-room reimplementation from public Snapcast JSON-RPC control-protocol documentation (source not read, per ADR-020 / Charter §6.1).
**Verified:** 32/32 mapped symbols found in source · 56 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Server.GetStatus topology tree — streams + clients + groups as one state object | src/topology.rs::Topology | yes |
| Stream object — id / status (playing\|idle) / codec / sampleformat, with wire-token round-trip | src/topology.rs::{Stream,StreamStatus} | yes |
| Client object — id / name / connected / volume {muted, percent 0..=100} / latency; validated Volume value object (bounds + untrusted clamp); mute independent of volume | src/client.rs::{Client,Volume,VolumeError} | yes |
| Group object — id / name / stream_id / members[] / muted; membership helpers | src/group.rs::Group | yes |
| One-group invariant — every client belongs to exactly one group; every group member is a known client | src/topology.rs::Topology::invariant_holds | yes |
| Group volume = average of unmuted members; proportional, per-member-clamped spread of a group-volume change (standard Snapcast behaviour) | src/group.rs::{effective_volume,spread_group_volume} | yes |
| Pure validated control ops — set client volume/mute/latency/name, set group stream/mute, set group volume, move client between groups, create/dissolve group (bounds-checked, unknown-id rejecting, invariant-preserving) | src/control.rs::{set_client_volume,set_client_mute,set_client_latency,set_client_name,set_group_stream,set_group_mute,set_group_volume,move_client_to_group,create_group,dissolve_group} | yes |
| Time-sync model — signed latency/offset value object (clamped) + per-client play-delay (target buffer minus latency, clamped) + minimum shared target buffer from slowest client | src/sync.rs::{LatencyMs,client_delay_ms,min_target_buffer_ms} | yes |
| JSON-RPC 2.0 message model — typed Request (id/method/params) + Notification (no id), with documented method names and Client.SetVolume / Group.SetStream param shapes; std-only JSON round-trip | src/rpc.rs::{Request,Notification,method,client_set_volume,group_set_stream} | yes |
| Minimal std-only JSON value model + parser + serializer (objects/arrays/strings/ints/bools) for JSON-RPC envelopes | src/json.rs::Json | yes |
| Grandma-friendly EN/DE/TR wording — house headline (playing together / every room / nothing / muted) + per-speaker status (Charter §6.3, ADR-007 / ADR-020) | src/label.rs + src/control.rs::{headline,client_headline} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| TCP JSON-RPC control transport (connect to snapserver, send requests, correlate responses by id) | phase-1b | ADR-020 / ROADMAP M9: control brain produces/consumes JSON-RPC envelopes (rpc.rs); TCP client that frames newline-delimited JSON over socket is pure network I/O on top |
| Live notification stream (Client.OnConnect / OnVolumeChanged / Group.OnStreamChanged push updates applied to topology) | phase-1b | ADR-020 / ROADMAP M9: Notification type + method names modelled (rpc.rs); decoding pushed notification and folding into Topology is apply-loop over same typed messages, async/socket-bound |
| snapserver / snapclient audio pipeline (PCM chunk streaming, encoder/decoder, ring buffer, playback) | phase-1b | ADR-020: actual audio data plane (PCM streaming, codec FLAC/Opus/PCM framing, jitter/ring buffering, ALSA/Pulse output) is audio-pipeline-bound, largest deferred piece; Phase-1 control brain reasons about it without moving samples |
| Real wire-level time-sync handshake (server clock-offset estimation + chunk timestamping) | phase-1b | ADR-020: sync.rs implements pure delay/target-buffer arithmetic; live protocol that estimates each client's clock offset and stamps PCM chunks with play-at wall-clock time is timing/network-bound, lands with audio pipeline |
| Source / stream backends (Spotify, AirPlay, named pipe, process, TCP source) feeding snapserver | phase-1b | ADR-020: stream's id/codec/status modelled (topology.rs); backends that produce audio into Snapcast stream are per-source I/O integrations layered on audio pipeline, several account/protocol-bound |
| cave-home-core entity/state integration + automation triggers | phase-1b | ADR-020 / ROADMAP M9: surfacing speakers/groups as core State entities + automation targets lands once cave-home-core's entity API stabilises; control brain is already core-agnostic (no cave-home-* dependency) |
| Music Assistant / Mopidy orchestration handoff (what to play, playlists, queue) | phase-2 | ADR-020: Snapcast is the sync layer; choosing/queuing content is Music Assistant (cave-home-audio-mass) + Mopidy (cave-home-audio-mopidy); cross-crate orchestration is Phase-2 concern on top of three brains |

## Drift notes

None — every claimed symbol exists in source. All 32 mapped symbols verified present in crates/cave-home-audio-snapcast/src/ with correct signatures. All 7 unmapped gaps carry proper ADR-020 Phase 1b / permanent dispositions per Charter §6.1 (clean-room from public protocol documentation, Snapcast GPL-3.0 source not read). Declared ratios are mathematically sound: honest = fill / (fill + (1-fill)*(1-adr_justified)) = 0.30 / (0.30 + 0) = 1.00.
