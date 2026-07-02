# cave-home-jarvis — voice command + local LLM dispatch (handoff)

**Branch:** `feature/jarvis-voice-llm` (worktree `../cave-home-jarvis-voice`)
**Base:** `520b639` (claude/cave-home-tesla-fleet-api-2026-06-07 HEAD)
**Status:** complete, all green, **not pushed**. Local-merge ready.
**Date:** 2026-06-07

A clean-room OpenJarvis-class voice-assistant **pipeline** wrapped around the
existing `cave-home-voice` NLU brain. New crate `cave-home-jarvis` plus the full
4-track (crate + CLI + Portal + metrics). Apache-2.0; no upstream source copied
(OpenJarvis pipeline shape, openWakeWord approach, whisper.cpp/piper engine
surfaces, Ollama `/api/chat` protocol — all spec-based, ADR-024 / ADR-002).

## What was built

| Stage | Module | What's real here |
|---|---|---|
| Audio capture | `audio` | `AudioFrame` (PCM-i16→f32), RMS/ZCR, `AudioSource` seam + `MockAudioSource`, energy+ZCR **VAD** with hangover |
| Features | `features` | pre-emphasis, Hann, **direct DFT** power spectrum, triangular **mel filterbank**, log-mel `FeatureExtractor` (std-only, no FFT crate) |
| Wake word | `wake` | **DTW** keyword spotter over log-mel frames, L2-normalised (loudness-invariant), multi-keyword, `enroll`/`detect` |
| STT | `stt` | `Transcript`/`Segment` model, `SpeechToText` seam + scripted `MockStt` |
| TTS | `tts` | `SpokenReply`, `TextToSpeech` seam + `MockTts` (deterministic PCM) |
| **LLM gateway** | `llm/` | own mini gateway: `HttpTransport` seam + `MockTransport`; Ollama chat + **tool-calling** wire codec (`OllamaGateway`); `LlmClient` + `MockLlm` |
| Tools | `tools` | `intent_to_tool_call` bridge, 6 builtin JSON-Schema tool specs, `ToolRegistry` validation, `ToolExecutor` seam + `MockToolExecutor` |
| **Dispatch** | `dispatch` | NLU fast-path (`cave_home_voice::understand`) → else **bounded LLM tool-calling loop** (validate→execute→feed back); room/speaker context |
| Multi-room | `room` | `RoomRegistry` device→room, `DispatchContext`, deictic ("here") target resolution |
| Voice profiles | `profile` | `SpeakerBook` — mean L2-norm log-mel **d-vectors**, cosine identify (Burak vs Sanja) |
| Pipeline | `pipeline` | `JarvisPipeline` state machine: AudioSource→wake→VAD→STT→speaker→room→dispatch→TTS; `run()` + `handle_command()` |
| Config | `config` | `JarvisConfig` validated; builds `RoomRegistry`/`DispatchConfig` |
| Metrics | `metrics` | Prometheus text: wakes, transcripts, dispatch-by-path, llm turns, tool calls/failures, speaker ids |

**4-track:**
- **CLI:** `cavehomectl jarvis ask|tools|wake` — `ask` really runs the crate's NLU
  path (links `cave-home-jarvis` like `energy` links `cave-home-tesla`).
- **Portal:** `/jarvis` page (`JarvisPage`, self-contained pure UI model) + new
  resident-facing `Card::Jarvis`. EN/DE/TR localised.
- **Metrics:** `jarvis_*` counters in `metrics.rs`.

## Acceptance criteria — evidence

All three required scenarios are covered by **explicit red→green TDD pairs**:

1. **mock audio → wake-word detection** — `wake::tests` (`0496bd7` RED →
   `83b9360` GREEN). `detector_fires_on_enrolled_keyword` (quiet copy still
   fires), `detector_rejects_a_different_word`, `..picks_closest_of_several`.
2. **mock STT → intent extraction → service call** — `dispatch::tests::nlu_path_executes_matched_intent`
   + `pipeline::tests::handle_command_runs_full_chain_nlu` (`573493b` RED →
   `7bbe1d4` GREEN).
3. **LLM tool-calling round-trip (mock LLM)** — `dispatch::tests::llm_path_round_trips_tool_call_then_answers`
   (two chat turns: tool call → execute → tool-result fed back → spoken answer).

```
cargo test -p cave-home-jarvis   →  79 passed; 0 failed
cargo clippy -p cave-home-jarvis --lib  →  0 jarvis warnings (workspace pedantic+nursery)
cargo build -p cave-home-binary  →  links into the single binary
```

## LOC ratio (impl vs test, non-blank non-comment)

```
TOTAL impl 1955 / test 998  →  test:code 0.51    (79 #[test]/#[tokio::test] fns)
```
Plus 8 CLI command tests and 11 Portal page tests. Clean-room: **0 lines copied**
from any upstream — implemented from documented pipeline/protocol shapes only.

## TDD git log

```
a5f229c feat(portal): /jarvis assistant status page + Card::Jarvis (4-track)
bf14966 feat(cli): cavehomectl jarvis — talk to your home (4-track)
ddcf24c feat(jarvis): end-to-end pipeline + validated config + Prometheus metrics
49d09be feat(jarvis): multi-room device context + per-speaker voice profiles
7bbe1d4 feat(jarvis): dispatch brain — NLU fast-path + bounded LLM tool-calling (GREEN)
573493b test(jarvis): dispatch NLU + LLM tool-calling round-trip specs (RED)
6dca2ab feat(jarvis): self-contained local LLM gateway (Ollama chat + tool-calling)
3241a5b feat(jarvis): STT + TTS engine seams with scripted mocks
83b9360 feat(jarvis): wake-word DTW matcher (GREEN)
0496bd7 test(jarvis): wake-word DTW matcher specs (RED)
12f0e5b feat(jarvis): audio capture seam + VAD + log-mel feature front-end
```

## The only seams (Phase-1b ML/hardware bindings)

Each is a trait with an in-crate mock the whole pipeline is tested against:
- `audio::AudioSource` → real ALSA/CPAL microphone
- `stt::SpeechToText` → whisper.cpp (ggml) binding
- `tts::TextToSpeech` → piper (ONNX) binding
- `llm::transport::HttpTransport` → reqwest/socket to the local Ollama/llama.cpp
- `tools::ToolExecutor` → real cave-home service calls (MQTT / device crates)

No cloud STT/LLM/TTS path is ever added (Charter §9). The LLM gateway is the
home's **own** mini gateway — deliberately isolated from `cave-runtime`'s shared
`cave-llm-gateway`.

## Next steps (when wiring the real engines)
1. Implement `HttpTransport` with reqwest behind a `runtime` feature (mirror the
   `cave-home-traefik-rs` precedent); point `OllamaGateway` at `127.0.0.1:11434`.
2. Implement `SpeechToText`/`TextToSpeech` over whisper.cpp/piper FFI; keep the
   16 kHz mono `f32` contract.
3. Implement `ToolExecutor` against the device crates so `set_light` etc. act.
4. Tune `WakeConfig.threshold` / `VadConfig` against real enrolled clips; swap
   the DTW matcher for a trained openWakeWord model behind the same seam if
   desired.

## Isolation note
Built in a dedicated `git worktree` (`../cave-home-jarvis-voice`) off live HEAD
because a concurrent automated loop checks out branches in the shared checkout.
**Not pushed.** To integrate: `git merge --no-ff feature/jarvis-voice-llm`.
