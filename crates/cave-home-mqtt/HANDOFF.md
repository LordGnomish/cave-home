# cave-home-mqtt — MQTT 5.0 broker handoff

Clean-room MQTT 5.0 broker built on `feature/mqtt-clean-room` (worktree
`../cave-home-mqtt-broker`, based on the honest-uplift HEAD `ae73672`,
which already carried the complete MQTT 3.1.1 wire codec).

## Licensing / clean-room posture

Upstream Eclipse Mosquitto is **EPL-2.0 / EDL-1.0** — incompatible with
the Apache-2.0 unified binary. **No Mosquitto source was read.** Every
type and test was derived from the published OASIS specs:

- *OASIS MQTT Version 3.1.1 (Plus Errata 01)*
- *OASIS MQTT Version 5.0*

Clean-room evidence is the git history itself: strict TDD, where a
`test(...)` commit with a failing/non-compiling test precedes every
`feat(...)` implementation commit, and each test names the spec section
it was designed from. See **TDD log** below and `parity.manifest.toml`
(`[[spec_section]]` / `[[spec_test]]` map code ↔ cited spec, never to
Mosquitto's EPL test suite).

## What shipped

| Layer | Module | Notes |
|-------|--------|-------|
| 3.1.1 codec | `codec.rs`, `packet.rs` | all 14 control packets (pre-existing base) |
| 5.0 properties | `v5/property.rs` | §2.2.2 — 27 identifiers, dup detection |
| 5.0 reason codes | `v5/reason.rs` | §2.4 full table + error classification |
| 5.0 wire types | `v5/wire.rs` | §1.5 cursor primitives |
| 5.0 packets | `v5/packet.rs`, `v5/codec.rs` | all 15 incl. AUTH; sub options; compact acks |
| topic matching | `broker/topic.rs` | §4.7 +/#, §4.7.2 $-shield, §4.8.2 shared subs |
| retained | `broker/retain.rs` | §3.3.1.3 store + zero-length clear |
| auth/ACL | `broker/auth.rs` | §3.1.3.5/6 creds + ordered topic ACL |
| sessions | `broker/session.rs` | §4.1/§4.3 QoS state + persistent queue |
| router | `broker/mod.rs` | full QoS 0/1/2 both ways, LWT, takeover, No Local |
| hooks | `broker/hooks.rs` | automation seam: observe + veto publish |
| metrics | `metrics.rs` | counters/gauges + Prometheus exposition |
| bridge | `bridge.rs` | Mosquitto `topic` directive parse + prefix mapping |
| runtime | `runtime/*` (feature) | TCP + TLS (rustls/ring) + WebSocket (§6) |

The broker decision core is **I/O-free**: `Broker::{connect,handle,
network_disconnect}` take packets and return `Action::{Send,Drop}` for
the transport to apply. This is what makes QoS 1/2 handshakes, retained
delivery, persistent sessions and LWT unit-testable without sockets.

## Acceptance criteria

- ✅ `cargo test -p cave-home-mqtt` — **92** unit/integration tests pass.
- ✅ `cargo test -p cave-home-mqtt --features runtime` — **+5** e2e socket
  tests (TCP, TLS handshake, WebSocket, QoS 1 round-trip, persistent
  session over the wire). 97 total.
- ✅ MQTT 5.0 conformance-style codec tests (every packet round-trips;
  level-4 / reserved-bit / unknown-property / malformed-frame rejections).
- ✅ QoS 0/1/2 round-trip tests (both the in-core router and over real
  TCP).
- ✅ TLS connection test (rcgen self-signed cert → rustls acceptor →
  tokio-rustls client handshake → CONNECT/CONNACK).
- ✅ Persistent session test (core + over-the-wire resume delivery).
- ✅ `cargo clippy -p cave-home-mqtt --features runtime --all-targets` —
  no errors (pedantic/nursery warnings match the crate baseline).

## Clean-room LOC report (spec-referenced, no Mosquitto source)

Production Rust, by area (inline `#[cfg(test)]` mods included per file):

```
 src/codec.rs           714   3.1.1 wire codec        (base)
 src/v5/codec.rs        730   5.0 packet codec        §3.1-§3.15
 src/broker/mod.rs      996   broker router           §3.x/§4
 src/v5/property.rs     300   properties              §2.2.2
 src/packet.rs          244   3.1.1 packet types      (base)
 src/runtime/mod.rs     239   hub + listeners         §6/§2.1.4
 src/bridge.rs          228   bridge mapping          (Mosquitto cfg compat)
 src/v5/packet.rs       228   5.0 packet types        §3
 src/broker/topic.rs    186   wildcard matcher        §4.7/§4.8.2
 src/v5/reason.rs       173   reason codes            §2.4
 src/broker/auth.rs     170   auth + ACL              §3.1.3.5/6
 src/broker/hooks.rs    203   automation seam         (cave-home)
 src/metrics.rs         145   metrics + prometheus    -
 src/v5/wire.rs         138   wire primitives         §1.5
 src/broker/retain.rs   106   retained store          §3.3.1.3
 src/runtime/ws.rs       85   websocket adapter       §6
 src/broker/session.rs   84   session state           §4.1/§4.3
 src/runtime/frame.rs    71   async framing           §2.1.4
 src/runtime/tls.rs      32   rustls acceptor         -
 tests/runtime.rs       231   e2e socket tests        -
```

`parity.manifest.toml`: `test_port_ratio = 0.0` — **zero** tests ported;
all designed from spec. `fill_ratio = 0.78` against the target slice of a
usable embedded MQTT 5.0 broker, `fill_ratio_full_spec = 0.55` against
the whole Mosquitto feature surface.

## TDD log (9 test→feat pairs)

```
test → feat: 5.0 reason codes + property codec      (§2.2.2/§2.4)
test → feat: 5.0 control-packet codec               (§3.1-§3.15)
test → feat: topic matcher + shared subs            (§4.7/§4.8.2)
test → feat: retained store + auth/ACL              (§3.3.1.3/§3.1.3.5)
test → feat: broker router                          (QoS 0/1/2, LWT, ACL…)
test → feat: async TCP/TLS/WebSocket runtime
test → feat: metrics + Prometheus exposition
test → feat: plugin hooks + metrics broker wiring
test → feat: Mosquitto-compat bridge mapping
```

## How to run

```bash
# core (no async deps)
cargo test -p cave-home-mqtt
# + real TCP/TLS/WebSocket listeners and e2e tests
cargo test -p cave-home-mqtt --features runtime
```

Embedding the broker:

```rust
use cave_home_mqtt::broker::{Broker, BrokerConfig};
use cave_home_mqtt::broker::auth::Authenticator;
use cave_home_mqtt::runtime::Server;            // feature = "runtime"
use tokio::net::TcpListener;

let mut auth = Authenticator::default();
auth.set_anonymous(false);
auth.add_user("admin", b"secret");

let server = Server::new(Broker::new(BrokerConfig::default(), auth));
let tcp = TcpListener::bind("0.0.0.0:1883").await?;
server.serve_tcp(tcp).await?;                   // also serve_tls / serve_ws
```

## What's next (see `[[unmapped]]` in the manifest)

1. **Bridge runtime pump** — the mapping core is done; add the remote
   client loop (connect, subscribe `remote_subscriptions()`, forward
   mapped messages, reconnect/backoff). Wire it as a `BrokerHook` for the
   outbound direction.
2. **Flow control** — enforce Receive Maximum (§3.3.4) inflight windows
   and maintain per-connection Topic Alias tables (§3.3.2.3).
3. **Keep-alive enforcement** — runtime timer that disconnects on 1.5×
   keep-alive silence (§3.1.2.10); server-side AUTH exchange (§4.12).
4. **cavehomectl / portal wiring** — `Broker::prometheus()` and the
   `BrokerHook` seam are the integration points; cross-crate wiring was
   kept out of this isolated worktree to avoid destabilising the
   workspace.

## Branch / merge status

- Branch `feature/mqtt-clean-room`, worktree `../cave-home-mqtt-broker`,
  based on `ae73672`.
- Merged `--no-ff` into a local integration branch; **not pushed**.
