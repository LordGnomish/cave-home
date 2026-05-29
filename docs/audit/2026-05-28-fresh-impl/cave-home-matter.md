# Coverage matrix — cave-home-matter

**Declared:** fill=0.48 · adr_justified=N/A · honest=0.48 · port method per manifest.
**Verified:** 54/54 mapped symbols found in source · 78 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Onboarding payload parsing (QR + manual pairing code) | src/setup_payload.rs::SetupPayload | yes |
| QR code payload parser | src/setup_payload.rs::parse_qr_payload | yes |
| Manual pairing code parser | src/setup_payload.rs::parse_manual_pairing_code | yes |
| Base38 decode | src/setup_payload.rs::base38_decode | yes |
| Commissioning flow enum | src/setup_payload.rs::CommissioningFlow | yes |
| Rendezvous information flags | src/setup_payload.rs::RendezvousInformationFlags | yes |
| Verhoeff check digit computation | src/setup_payload.rs::verhoeff_compute_check | yes |
| PASE session establishment | src/pase.rs::PaseSession | yes |
| PASE wait for establishment | src/pase.rs::PaseSession::wait_for_establishment | yes |
| PASE pairing handshake | src/pase.rs::PaseSession::pair | yes |
| PASE PBKDF param request | src/pase.rs::PaseSession::send_pbkdf_param_request | yes |
| PASE derive secure session | src/pase.rs::PaseSession::derive_secure_session | yes |
| Spake2p P256 SHA256 | src/pase.rs::Spake2pP256Sha256 | yes |
| CASE session establishment | src/case.rs::CaseSession | yes |
| CASE sigma1 handshake | src/case.rs::CaseSession::send_sigma1 | yes |
| CASE sigma2 response | src/case.rs::CaseSession::handle_sigma2 | yes |
| CASE sigma3 response | src/case.rs::CaseSession::handle_sigma3 | yes |
| CASE derive secure session | src/case.rs::CaseSession::derive_secure_session | yes |
| Fabric table | src/fabric.rs::FabricTable | yes |
| Add pending fabric | src/fabric.rs::FabricTable::add_pending | yes |
| Commit pending fabric | src/fabric.rs::FabricTable::commit_pending | yes |
| Delete fabric | src/fabric.rs::FabricTable::delete_fabric | yes |
| Fabric info structure | src/fabric.rs::FabricInfo | yes |
| Fabric table iterator | src/fabric.rs::FabricTable::iter | yes |
| Access control enforcement | src/acl.rs::AccessControl | yes |
| ACL check operation | src/acl.rs::AccessControl::check | yes |
| ACL entry | src/acl.rs::Entry | yes |
| Privilege levels | src/acl.rs::Privilege | yes |
| Authentication modes | src/acl.rs::AuthMode | yes |
| Group data provider | src/group_key.rs::GroupDataProvider | yes |
| Set group key | src/group_key.rs::GroupDataProvider::set_group_key | yes |
| Set group info | src/group_key.rs::GroupDataProvider::set_group_info | yes |
| OTA requestor | src/ota.rs::OtaRequestor | yes |
| Trigger OTA query | src/ota.rs::OtaRequestor::trigger_immediate_query | yes |
| Apply OTA update | src/ota.rs::OtaRequestor::apply_update | yes |
| OnOff cluster client | src/clusters/on_off.rs::OnOffClient | yes |
| OnOff::On command | src/clusters/on_off.rs::OnOffClient::on | yes |
| OnOff::Off command | src/clusters/on_off.rs::OnOffClient::off | yes |
| OnOff::Toggle command | src/clusters/on_off.rs::OnOffClient::toggle | yes |
| LevelControl move to level | src/clusters/level_control.rs::LevelControlClient::move_to_level | yes |
| ColorControl move to hue and saturation | src/clusters/color_control.rs::ColorControlClient::move_to_hue_and_saturation | yes |
| ColorControl move to color temperature | src/clusters/color_control.rs::ColorControlClient::move_to_color_temperature | yes |
| Thermostat setpoint raise/lower | src/clusters/thermostat.rs::ThermostatClient::setpoint_raise_lower | yes |
| DoorLock lock door | src/clusters/door_lock.rs::DoorLockClient::lock_door | yes |
| DoorLock unlock door | src/clusters/door_lock.rs::DoorLockClient::unlock_door | yes |
| Network commissioning add Thread network | src/clusters/network_commissioning.rs::NetworkCommissioningClient::add_thread_network | yes |
| Network commissioning connect network | src/clusters/network_commissioning.rs::NetworkCommissioningClient::connect_network | yes |
| UDP transport | src/transport/udp.rs::UdpTransport | yes |
| BLE transport | src/transport/ble.rs::BleTransport | yes |
| Matter commissioner | src/commissioner.rs::Commissioner | yes |
| Commissioner pair device | src/commissioner.rs::Commissioner::pair_device | yes |
| Commissioner unpair device | src/commissioner.rs::Commissioner::unpair_device | yes |
| Matter device controller | src/controller.rs::Controller | yes |
| Matter error type | src/error.rs::MatterError | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Device-side server implementation (src/app/server/Server.cpp) | phase-1b | cave-home is the commissioner/admin role for Phase 1, not the device role. |
| Interaction Model Read/Write/Subscribe handlers | phase-1b | Phase 1 commissioner sends per-cluster commands directly; subscription/read paths land with the dashboard. |
| ICD (Intermittently Connected Devices) | phase-1b | Battery-powered sleepy device coordination. |
| BDX (Bulk Data Transfer) | phase-1b | Used by OTA image download; Phase 1 OTA simulates the image transport. |
| ExchangeManager full retry/reliability layer | phase-1b | Phase 1 uses a minimal exchange wrapper; MRP back-off lands later. |
| mDNS commissionable/operational advertisement (src/lib/dnssd/) | phase-1b | Discovery uses BLE advert in Phase 1; mDNS browse depends on cave-home-node-discovery. |
| DAC chain validation (attestation_verifier) | phase-1b | DAC/PAI chain verification against the test PAA roots is stubbed out for Phase 1 dev-mode pairing. |
| Window covering cluster (roller-shutter / blind) | phase-1b | cave-home-cover plans to integrate. |
| Fan control cluster | phase-1b | HVAC-adjacent. |
| Pump configuration and control cluster | phase-1b | Water/pool pumps. |
| Operational state + RVC operational state | phase-1b | Vacuum cleaner support. |
| Refrigerator and temperature controlled cabinet | phase-2 | Smart fridge — not in Charter pillars. |
| Microwave oven control + cook surface | phase-2 | Kitchen appliances — Charter §3 deferral. |
| InetLayer non-Linux porting (src/inet/) | permanent | cave-home is Linux 7.1+ only per ADR-003; tokio + socket2 replace the CHIP InetLayer. |
| Platform RTOS bindings (Zephyr, FreeRTOS, ESP-IDF, etc.) | permanent | Embedded RTOS porting layers; cave-home runs on Linux hosts. |
| Apple/Android SDK bindings (darwin/Framework + java/) | permanent | Mobile client bindings; the cave-home mobile app talks to cave-home-portal, not the chip stack. |
| C++ reference apps (chip-tool, all-clusters-app, lighting-app, etc.) | permanent | C++ reference apps; cave-home ships its own commissioner via cavehomectl. |

## Drift notes
None — every claimed symbol (54/54) exists in source.
