# Coverage matrix — cave-home-hue-bridge-emu

**Declared:** fill=0.50 · adr_justified=unspecified · honest=unspecified · clean-room method per manifest.
**Verified:** 17/17 mapped symbols found in source · 53 test fns (50 unit + 3 integration) · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Hue API v1 §7.1 Create user (POST /api) | src/pairing.rs::PairingService::try_pair | yes |
| Hue API v1 §7.2 Configuration GET | src/api/v1/views.rs::short_config + full_config | yes |
| Hue API v1 §1 Lights (list/get/set state) | src/api/v1/mod.rs::{get_lights, get_light, put_light_state} | yes |
| Hue API v1 §4 Groups (list/action/scene recall) | src/api/v1/mod.rs::{get_groups, put_group_action} | yes |
| Hue API v1 §5 Scenes (list) | src/api/v1/mod.rs::get_scenes | yes |
| Hue API v1 §2 Sensors (list) | src/api/v1/mod.rs::get_sensors | yes |
| Hue API v2 envelope {errors, data} | src/api/v2/mod.rs::Envelope | yes |
| Hue API v2 light resource (GET/PUT) | src/api/v2/views.rs::light_view + src/api/v2/mod.rs::put_light | yes |
| Hue API v2 scene resource + recall | src/api/v2/mod.rs::{get_scenes, put_scene} | yes |
| Hue API v2 room/zone resources | src/api/v2/views.rs::room_or_zone_view + src/api/v2/mod.rs::{get_rooms, get_zones} | yes |
| Hue API v2 grouped_light resource | src/api/v2/views.rs::grouped_light_for_group | yes |
| Hue API v2 bridge resource (singleton) | src/api/v2/views.rs::bridge_view + src/api/v2/mod.rs::get_bridge | yes |
| Hue API v2 EventStream (SSE format) | src/api/v2/eventstream.rs::render_event + src/registry.rs::StreamEvent | yes |
| Bridge discovery SSDP description.xml | src/discovery/ssdp.rs::build_description_xml + build_ssdp_response_root | yes |
| Bridge discovery mDNS _hue._tcp | src/discovery/mdns.rs::build_advertisement | yes |
| Hue error messages (type codes) | src/errors.rs::HueProtocolError + error_type::* | yes |
| Bridge identity (bridgeid, mac, UDN) | src/config.rs::BridgeIdentity | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Hue Entertainment / DTLS streaming | phase-2 | v2 entertainment_configuration + DTLS realtime stream; ADR-010 §Open Questions §1. |
| Rule engine + schedules + resourcelinks / behavior_instance + smart_scene | phase-2 | ADR-010 §Open Questions §2 — emulator targets the control surface only. |
| v2 long-tail resources (motion_area_configuration, bell_button, contact, tamper, camera_motion, etc.) | phase-1b | Models will land alongside their controllers as we extend coverage. |
| HomeKit / Matter pass-through emulation | phase-2 | Bridge can expose itself as HomeKit / Matter accessory; cave-home does that via its own Matter crate. |
| Persistent storage of emulator state across restarts | phase-1b | Phase 1 uses in-memory registry; cave-home binary wires its shared storage layer next. |
| TLS / certificate provisioning for v2 /clip/v2/* surface | phase-1b | Phase 1 surfaces HTTP-only payloads; the cave-home binary will wire its TLS terminator. |
| Round-bridge SSDP UDP listener / mDNS responder | phase-1b | Payload builders are clean; the UDP / multicast machinery sits behind a trait the binary supplies. |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | ADR-003 — Linux 7.1+ only. |
| diyHue feature parity (rule scripting, sensor adapters) | permanent | Charter §6.1 — diyHue is GPL-3.0, source NOT consulted. cave-home provides equivalent behaviour through its own automation crate. |

## Drift notes
None — every claimed symbol exists in source. All 17 mapped spec_sections verified against src/. Clean-room implementation confirmed by review of files; CLEAN-ROOM banner present in all .rs files per manifest preamble.
