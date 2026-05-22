# ADR-024 — Voice framework expansion (Mycroft + Rhasspy + OVOS)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

The existing `cave-home-voice` crate (Charter §3 voice pillar)
already covers whisper.cpp (STT) + piper (TTS). The HA "Year of
Voice" pattern adds **wake-word detection**, **intent routing**,
and **dialog management** — three layers that need a richer
framework than whisper / piper alone.

The two relevant upstreams are:
- **Open Voice OS (OVOS)** — Apache-2.0, the actively-maintained
  Mycroft AI fork that carries dialog management forward.
- **Rhasspy** — MIT, the offline voice-assistant framework
  with strong wake-word + intent routing primitives.

## Decision

**No new crate.** `cave-home-voice` is **expanded** in place to
cover the wake-word + intent-routing + dialog-management
surface, porting line-by-line from:

- **Mycroft AI** (`MycroftAI/mycroft-core`) — Apache-2.0
  (Mycroft AI itself is sunsetting; OVOS is the continuation).
- **Open Voice OS / OVOS** (`OpenVoiceOS/ovos-core`) —
  Apache-2.0, the active continuation.
- **Rhasspy** (`rhasspy/rhasspy`) — MIT.

Port method: **line-by-line** (all permissive). The crate's
existing whisper.cpp + piper bindings stay; this expansion adds
wake-word, intent, and dialog layers on top.

## Consequences

### Accepted gains
- "Hey cave-home" wake-word + intent routing into the
  automation engine without an HA add-on container.
- Multi-language voice (TR + EN + DE per Charter §6.3 i18n
  mandate) works because Rhasspy + OVOS both have multilingual
  intent training.

### Accepted costs
- Wake-word training data licensing is per-model; cave-home
  ships a default wake-word and lets users train custom ones
  locally.
- Voice stack is the largest single in-process module after the
  HA core port; resource floor on the primary hub grows.

### Charter §6.3 / ADR-007 compliance
UI says "Hey cave-home", "Akşam moduna geç", "Tüm ışıkları
kapat" — never "intent slot", "Padatious model", "Adapt parser".

## Alternatives considered

- (a) Keep whisper + piper only; defer wake-word. Rejected —
  cave-home's voice promise (Charter §3) needs full pipeline,
  not just STT/TTS.
- (b) Single framework (OVOS only). Rejected — Rhasspy's
  wake-word + intent primitives are best-in-class even if OVOS
  has the dialog story; the two compose rather than compete.

## Notes

[ASSUMPTION: Mycroft AI is sunsetting and OVOS is the
continuation, so the port effectively ports OVOS with Mycroft
as the historical reference. If Mycroft revives, this ADR is
amended.]
