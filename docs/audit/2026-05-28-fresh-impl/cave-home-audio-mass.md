# Coverage matrix — cave-home-audio-mass

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · spec-based port method per manifest.
**Verified:** 26/26 mapped symbols found in source · 62 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Music-library media model: Track / Artist / Album / Playlist / MediaItem + ProviderId (no network) | src/media.rs::{Track,Artist,Album,Playlist,MediaItem,ProviderId,TrackId} | yes |
| Deterministic seeded shuffle (LCG + Fisher-Yates; reproducible per seed, full coverage) | src/shuffle.rs::{Lcg,shuffled_order} | yes |
| Playback-queue engine: enqueue / enqueue-next / play-now / clear / move / remove / current index | src/queue.rs::Queue::{enqueue,enqueue_next,play_now,clear,move_item,remove_item,current,current_index} | yes |
| Next / previous track computed from repeat mode (Off/One/All) and shuffle together | src/queue.rs::Queue::{next_index,previous_index,advance,go_back,set_repeat,set_shuffle} | yes |
| Per-room player state: playing/paused/idle, current track, elapsed, volume, active queue | src/player.rs::{Player,PlayState,Volume} | yes |
| Crossfade / gapless transition flags + TTS-announcement interrupt with save/restore | src/player.rs::{TransitionPrefs,Player::begin_announcement,Player::end_announcement} | yes |
| Multi-room 'play on group' fan-out of shared queue across players (sync is Snapcast's job, ADR-020) | src/player.rs::PlayerGroup::{join,leave,fan_out} | yes |
| In-memory library with case-insensitive substring search by title/artist/album | src/library.rs::{Library,SearchFields} | yes |
| Grandma-friendly EN/DE/TR status sentences (Charter §6.3, ADR-007) | src/label.rs::{Lang,Player::status_sentence} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Music-provider adapters (local file scan, Spotify / Tidal / etc.) | phase-1b | ADR-020: provider adapters map their catalog onto media::Track, then reuse this engine unchanged. Network/filesystem-bound I/O; no new queue logic. |
| Streaming / playback pipeline + Snapcast hand-off | phase-1b | ADR-020: the actual audio stream and sample-aligned cross-room sync is Snapcast's job (cave-home-audio-snapcast, clean-room). This engine produces the resolved per-room queue and stays out of the real-time path. |
| Metadata / artwork fetch | phase-1b | ADR-020: titles/artist/album/artwork enrichment is a network fetch that fills media::Track fields. Account/network-bound; the model already carries the fields, only the fetcher is deferred. |
| cave-home-core entity/state integration | phase-1b | ADR-020: surfacing players and queues as core State entities + automation triggers lands once cave-home-core's entity API stabilises. The engine is already core-agnostic. |
| MPD backend (vs Mopidy) support | permanent | ADR-020: MPD is GPL-only; cave-home prefers the Apache-2.0 Mopidy backend. Clean-room MPD path explicitly out of scope unless a future ADR-020b revisits it (Charter §6.1 license-clean stance). |
| Pre-revision / legacy queue-format compatibility mode | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home ships the current queue model only; no historical-snapshot or backward-compat mode. |

## Drift notes

None — every claimed symbol exists in source. All 26 mapped symbols verified:
- 7 types from media.rs (Track, Artist, Album, Playlist, MediaItem, ProviderId, TrackId)
- 2 from shuffle.rs (Lcg, shuffled_order)
- 14 Queue methods: enqueue, enqueue_next, play_now, clear, move_item, remove_item, current, current_index, next_index, previous_index, advance, go_back, set_repeat, set_shuffle
- 3 Player state types (Player, PlayState, Volume)
- 2 Player announcement methods (begin_announcement, end_announcement)
- TransitionPrefs
- 3 PlayerGroup methods (join, leave, fan_out)
- 2 Library types (Library, SearchFields)
- Lang enum + Player::status_sentence

Honest ratio (1.00) is fully supported: all 6 unmapped items carry explicit ADR-020 phase-1b or permanent disposition with technical justification.
