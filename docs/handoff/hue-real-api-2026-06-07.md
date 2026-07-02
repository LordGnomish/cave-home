# Handoff — cave-home-hue real CLIP v2 transport (2026-06-07)

Branch: `feature/hue-real-api` (off `520b639`, the tesla base).
Local integration merge: `claude/hue-real-api-integration` (`--no-ff`, **not pushed**).
Worktree: `../cave-home-hue-real`.

## What landed

The `cave-home-hue` scaffold already had the full v2 model/controller layer and
the `V2Request` / SSE-parser **seams** but no real transport behind them. This
branch fills in the real runtime + the colour science + the 4-track wiring.

### Backend (the core ask)

| Module | What | Tests |
|---|---|---|
| `v2/color.rs` | CIE xy ⇄ sRGB ⇄ HSV, gamut A/B/C triangle `contains`/`clamp`, mirek ⇄ kelvin. Line-by-line port of HA `homeassistant/util/color.py` + Philips gamut corners. Pure `f64`, no deps. | 11 |
| `v2/transport.rs` | `ReqwestTransport` implements `V2Request` (GET/PUT/POST/DELETE on `/clip/v2/<path>`). HTTPS to the bridge's self-signed LAN cert (`danger_accept_invalid_certs`); `with_base_url` for explicit-scheme/pinned-CA/test use. `hue-application-key` auth header. Status→`HueError` mapping. | 7 |
| `v2/eventstream.rs` | Live SSE: `LineBuffer` (byte→line reframer), `EventStream::connect_once` (GET `/eventstream/clip/v2`, `Accept: text/event-stream`, optional `Last-Event-ID`, streams reqwest bytes → `parse_sse_line` → `EventStreamParser` → `HueEvent` channel), `EventStream::spawn` reconnect-with-backoff loop. | 5 |
| `metrics.rs` | `Metrics` Prometheus registry: `hue_lights_on/total`, `hue_scenes_total`, `hue_bridge_reachable`, `hue_eventstream_events_total`, request-duration summary, `hue_api_errors_total`. Mirrors the Tesla adapter. | 6 |
| `v2/models/scene.rs` | `SceneRecallAction::from_cli` + `as_str` — the CLI vocabulary contract (mirrors `OpMode::from_cli`). | 2 |
| `v2/test_support.rs` | In-process **mock Hue bridge**: bare tokio `TcpListener` that captures one request per connection and replays a canned raw HTTP/1.1 response. **No wiremock/httpmock** (wiremock isn't in the offline cache; this is lighter and tests the real reqwest path over a real socket). | — |

`runtime` feature (default-on) gates the reqwest/rustls transport; `--no-default-features`
leaves a pure dependency-light core (color/scene/models still present).

### 4-track wiring

- **CLI**: `cave-home-cli` now depends on `cave-home-hue` (`default-features = false`,
  so **no reqwest in the cavehomectl build**). `hue::run` re-parses argv and
  dispatches like `energy::run`. `hue list-lights` renders demo lights with colour
  via `cave_home_hue::v2::color::xy_to_rgb`; `hue set-scene --id <id> [--action
  active|dynamic|static|off]` builds a `SceneRecall` via `SceneRecallAction::from_cli`
  (unknown action exits 1). Other verbs remain Phase-2 stubs. (+8 tests)
- **Portal**: new `cave-home-portal/src/hue.rs` `HuePage` view-model (bridge health
  line + light rows + scene chips, localised Lights/Lichter/Işıklar). `HuePage::cards()`
  emits the **existing** `Card::Light`/`Card::Scene` widgets — no new `Card` variant
  was needed (Light + Scene already exist), all resident-visible. (+4 tests)
- **Metrics**: see `metrics.rs` above.

## Verification

```
cargo test -p cave-home-hue                    # 98 pass (runtime)
cargo test -p cave-home-hue --no-default-features  # 86 pass (pure core)
cargo test -p cave-home-cli                    # 143 pass
cargo test -p cave-home-portal                 # 70 pass
cargo clippy -p cave-home-hue --lib            # 59 warnings == baseline (new modules add 0)
cargo build -p cave-home-binary                # builds
```

Acceptance items met: mock-bridge end-to-end tests (light GET + PUT body, scene
recall contract, SSE event delivery + reconnect loop), Hue colour-model
conversion tests, strict RED→GREEN TDD per module (see git log), LOC report below.

## LOC

1,780 insertions / 13 files. 42 new test fns. impl:test by file —
color 340/117, transport 166/89, eventstream 173/74, metrics 152/67,
portal hue 137/43, test_support 140/0 (test infra). Crate test total 67 → 98.

Upstream references (Apache-2.0): HA `homeassistant/util/color.py`,
aiohue `v2/controllers/events.py` + `v2/__init__.py` request layer.

## Notes / follow-ups

- The bridge IPC surface in the single binary still needs to construct a
  `ReqwestTransport` per paired bridge and pump `EventStream::spawn` into the
  controllers — the seam is ready (`HueBridgeV2::new` takes `Arc<dyn V2Request>`;
  `ReqwestTransport` is that impl).
- Self-signed acceptance is `danger_accept_invalid_certs`; a stricter mode that
  pins the bridge-id cert can hang off `with_base_url` + a custom rustls verifier
  later (left out to keep the offline build pure-rustls and the diff focused).
- Discovery (`discovery.rs`) still uses the `HueHttpClient` seam; a reqwest impl
  of *that* trait is a small follow-up (not in scope here).
- `runtime` is on by default; if a downstream wants the pure core it must set
  `default-features = false` (the CLI already does).
- **Not pushed.** Loop-owned scaffold branch `claude/cave-home-hue-scaffold-2026-06-07`
  was left untouched; this work is on `feature/hue-real-api`.
