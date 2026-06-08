# Coverage matrix — cave-home-voice

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method spec-based / clean-room.
**Verified:** 9/9 mapped symbols found in source · 78 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Sentence-template grammar: optional [..], alternatives (a\|b), {slot}, nesting (Rhasspy/Assist) | src/template.rs::Template::parse | yes |
| Input normalisation (lower-case, trim, collapse whitespace, strip punctuation) | src/matcher.rs::normalize_tokens | yes |
| Intent matcher: walk compiled templates over an utterance, extract slots, best-match selection | src/matcher.rs::match_intent | yes |
| Backtracking slot capture with inline resolution (greedy span, shrink on resolve failure) | src/matcher.rs::walk | yes |
| Slot types: fixed value list + synonyms, bounded number, open capture; validation + canonicalisation | src/slot.rs::{SlotKind,ValueList,resolve} | yes |
| Spoken-number parsing (digits + words) for EN/DE/TR, 0..=100 household range | src/number_words.rs::parse_number | yes |
| Built-in intent set (light on/off/brightness, HVAC set temp, cover open/close, scene activate, state queries) with EN/DE/TR sentence sets | src/intents.rs::builtin_intents | yes |
| Intent routing to a typed IntentAction + carried confidence | src/route.rs::route | yes |
| Grandma-friendly localised spoken-reply generation (EN/DE/TR) + no-match/ambiguity replies | src/response.rs::{respond,not_understood,please_clarify} | yes |
| Wake-word + assistant configuration model (enabled wake words / language / voice) with validation — config only | src/config.rs::AssistantConfig | yes |
| Ambiguous-match handling (report tied intents) + unknown-slot-value rejection | src/matcher.rs::MatchOutcome + src/lib.rs::Understanding | yes |
| End-to-end understand(): utterance → match → route → spoken reply | src/lib.rs::understand | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Speech-to-text engine (whisper.cpp-class) | phase-1b | ADR-024: STT is a native model bound to C library + model weights; produces recognised text the engine consumes. Model/audio-bound; no NLU changes when it lands. |
| Text-to-speech engine (piper-class) | phase-1b | ADR-024: TTS turns reply strings this crate generates into audio via piper (native model). Consumes response::respond output unchanged. |
| Wake-word detection (openWakeWord-class) | phase-1b | ADR-024: detecting 'Hey cave-home' in live audio is ONNX inference. Config model (enabled wake words/voice/language) implemented here; detector itself is model/audio-bound. |
| Audio capture pipeline + voice-activity detection (VAD) | phase-1b | ADR-024: microphone capture, framing, and VAD feed the STT stage. No text-level logic; lands with audio layer. |
| Per-user voice profiles | phase-1b | ADR-024: per-speaker recognition depends on audio + STT layers existing first; personalises which member is speaking, not how sentence is parsed. |
| cave-home-core intent execution wiring | phase-1b | ADR-024: handing routed IntentAction to cave-home-core to switch light/read temperature lands once core's entity/command API stabilises. Engine is core-agnostic. |
| Dialog management / multi-turn follow-ups + slot-filling re-prompts | phase-2 | ADR-024: single-shot intent matching is Phase-1 MVP. Multi-turn dialog state (asking follow-up to fill missing slot) is Phase-2 layer; ambiguity outcome already exposes re-prompt hook. |
| Cloud speech-to-text / text-to-speech | permanent | Charter §9 local-first / no cloud: cave-home never sends audio or text to cloud STT/TTS. On-device whisper/piper only; permanently out of scope. |

## Drift notes
None — every claimed symbol exists in source. All 9 mapped capabilities verified. All 7 unmapped areas carry explicit ADR-024 or Charter disposition. The declared honest_ratio of 1.00 is supported: fill_ratio of 0.30 divided by (0.30 + 0 unjustified gap) = 1.00.
