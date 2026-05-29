# Coverage matrix — cave-home-unifi-network

**Declared:** fill=0.35 · adr_justified=1.00 · honest=1.00 · port method: spec-based (HA + public API shape).
**Verified:** 9/9 mapped symbols found in source · 49 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Network device model (switch / AP / gateway, online state, uplink, switch ports) | src/device.rs::NetworkDevice | yes |
| Network client model (wired/wireless, IP via std::net, SSID, guest/blocked, last-seen tick) | src/client.rs::NetworkClient | yes |
| HA unifi block-client switch semantics (block / unblock, already-in-state rejection) | src/control.rs::{block_client,unblock_client} | yes |
| Reconnect/kick a wireless client (wired-client rejection) | src/control.rs::reconnect_client | yes |
| HA unifi PoE port-power switch (on/off/auto) with port-range + capability + not-a-switch validation | src/control.rs::set_poe | yes |
| WLAN enable/disable, port-forward toggle, device LED — typed commands | src/control.rs::{set_wlan_enabled,set_port_forward_enabled,set_device_led} | yes |
| HA unifi device_tracker home/away derivation from last-seen + consider-home timeout | src/presence.rs::presence_of | yes |
| Firewall port-forward + WLAN + guest-network + bandwidth-profile config model | src/network.rs::{PortForward,Wlan,GuestNetwork,BandwidthProfile} | yes |
| VLAN / network-segment model (id, name, subnet via std::net, purpose corporate/guest/iot) | src/network.rs::Vlan | yes |
| Connectivity summary: per-AP client counts + throughput aggregation over samples + internet-up derivation | src/summary.rs::{summarize,internet_state} | yes |
| Grandma-friendly EN/DE/TR phrasing (Charter §6.3, ADR-007 / ADR-009) | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| UniFi controller REST login + session/auth | phase-1b | ADR-009: authenticating to the local controller (UDM-Pro / Cloud Gateway) and holding the session is network-bound. It feeds devices/clients onto this crate's model and reuses the engine unchanged. I/O adapter only — no new decision logic. |
| UniFi controller WebSocket event transport | phase-1b | ADR-009: the live event stream (client connect/disconnect, device state change) is network-bound. It maps controller events onto this model and re-runs presence/summary. Transport, not logic. |
| Actual control API calls (issue Command over the wire) | phase-1b | ADR-009: this crate produces a typed control::Command (block, set PoE, toggle WLAN, etc.); the phase-1b transport turns each Command into the matching local Network API request. The decision is done here; only the send is deferred. |
| Controller-version negotiation (Network v7 vs v8 API surface) | phase-1b | ADR-009 / Charter §7 always-latest: detecting the controller version and selecting the right endpoint set is a transport concern that lands with the REST client. |
| cave-home-core entity/state + automation-trigger integration | phase-1b | ADR-009: surfacing devices/clients/presence as core State entities + automation triggers lands once cave-home-core's entity API stabilises. The engine is already core-agnostic (no cave-home crate deps). |
| Ubiquiti cloud (Site Manager / remote access) API | permanent | Charter §9 privacy-first / local-first: cave-home talks ONLY to the local controller API and never routes through Ubiquiti's cloud in the critical path. Cloud remote-access is permanently out of scope for this crate. |

## Drift notes
None — every claimed symbol exists in source. All 9 mapped [[mapped]] entries verified:
- `src/device.rs::NetworkDevice` struct defined at line 70
- `src/client.rs::NetworkClient` struct defined at line 51
- `src/control.rs::block_client` fn at line 94; `unblock_client` at line 105; `reconnect_client` at line 119; `set_poe` at line 135; `set_wlan_enabled` at line 154; `set_port_forward_enabled` at line 160; `set_device_led` at line 166
- `src/presence.rs::presence_of` fn at line 44
- `src/network.rs::Wlan` struct at line 13; `PortForward` at line 67; `BandwidthProfile` at line 121; `GuestNetwork` at line 147; `Vlan` at line 194
- `src/summary.rs::internet_state` fn at line 85; `summarize` fn at line 102
- `src/label.rs` module contains 6 public functions for i18n (devices_connected, guest_wifi_state, client_blocked, client_unblocked, internet_status, summary_sentence) with no implementation jargon.

The declared honest_ratio of 1.00 is justified: fill=0.35 ÷ (0.35 + 0 unjustified_gap) = 100% honest. Every unfilled item (6 unmapped areas) carries explicit ADR-009 phase-1b or permanent disposition.
