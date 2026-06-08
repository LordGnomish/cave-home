# Coverage matrix — cave-home-zigbee

**Declared:** fill=0.45 · adr_justified=unspecified · honest=unspecified · port method: clean-room (Zigbee 3.0 spec + public API reference only).
**Verified:** 23/23 mapped symbols found in source · 121 test fns · drift: NO.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Zigbee 3.0 §2.4.3.1 APSDE-DATA.request | src/network/aps.rs::ApsDataRequest | YES |
| Zigbee 3.0 §2.4.4 APSME primitives (BIND/UNBIND/GET/SET) | src/network/aps.rs::ApsmePrimitive | YES |
| Zigbee 3.0 §3.2 NLDE/NLME service primitives | src/network/nwk.rs::NetworkLayer | YES |
| Zigbee 3.0 §3.6.1.4 Routing table entry | src/network/routing.rs::RoutingTableEntry | YES |
| Zigbee 3.0 §3.6.3 Routing — discovery/repair | src/network/routing.rs::RoutingTable | YES |
| Zigbee 3.0 §2.5.5.5.1 Network steering for a device | src/pairing.rs::NetworkSteering | YES |
| Zigbee Base Device Behavior §10.1 Touchlink commissioning | src/pairing.rs::TouchlinkMode | YES |
| Zigbee Base Device Behavior §13.3 InstallCode-derived link key | src/pairing.rs::InstallCode | YES |
| ZCL §2.4 Foundation: Read Attributes (0x00/0x01) | src/zcl/foundation.rs::ReadAttributes | YES |
| ZCL §2.4 Foundation: Write Attributes (0x02/0x04) | src/zcl/foundation.rs::WriteAttributes | YES |
| ZCL §2.4 Foundation: Configure Reporting (0x06/0x08) | src/zcl/foundation.rs::ConfigureReporting | YES |
| ZCL §2.4.11 Report Attributes (0x0a) command | src/attribute_reporting.rs::ReportAttributes | YES |
| ZCL §2.6 ZCL header frame format | src/zcl/frame.rs::ZclFrame | YES |
| ZCL §3.6 Groups cluster (0x0004): Add/View/Remove/RemoveAll | src/groups.rs::GroupsCluster | YES |
| ZCL §3.7 Scenes cluster (0x0005): Add/View/Remove/Recall/Store | src/scenes.rs::ScenesCluster | YES |
| ZCL §11 OTA Upgrade cluster (0x0019) | src/ota.rs::OtaQueue | YES |
| Silicon Labs EZSP UG100 §3 — EZSP frame format | src/ezsp/frame.rs::EzspFrame | YES |
| Silicon Labs EZSP UG100 §4 — Configuration/Networking commands | src/ezsp/commands.rs::EzspCommand | YES |
| Silicon Labs EZSP UG100 §5 — ASH transport (Asynchronous Serial Host) | src/ezsp/ash.rs::AshFramer | YES |
| deCONZ Serial Protocol §3 — SLIP framing | src/deconz/slip.rs::SlipFramer | YES |
| deCONZ Serial Protocol §4 — APS-DATA-INDICATION/CONFIRM | src/deconz/commands.rs::DeconzCommand | YES |
| Zigbee 3.0 Annex C — Coordinator startup/form network | src/coordinator/mod.rs::Coordinator | YES |
| Transport abstraction — USB UART (CDC-ACM) + TCP socket | src/transport/mod.rs::Transport | YES |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition |
|---|---|---|
| Zigbee Green Power profile | phase-1b | Green Power proxy + sink; separate ZCL cluster requiring GP basic stack on top of core APS layer. |
| Smart Energy Profile (SEP 1.x/2.x) | phase-2 | Tariff/Demand Response/Pricing clusters; niche for Phase 1 persona, deferred per ADR-012. |
| OTA image transport (download + verify + signal) | phase-1b | Phase 1 ships OtaQueue + provider-trait interface only; block transfer + signature verification deferred. |
| Custom device converters (Z2M-style) | phase-1b | Z2M maps 4000+ devices via JS converters; cave-home models devices as ZCL attribute paths; per-device quirks Phase 1b. |
| Centralised vs distributed trust-centre policy | phase-1b | Phase 1 supports coordinator-rooted centralised trust centre; distributed networks Phase 1b. |
| InterPAN / Touchlink commissioning over inter-PAN | phase-1b | Phase 1 implements traditional join + InstallCode + Touchlink mode constant; inter-PAN frame layer Phase 1b. |
| Network key rotation (NLME-LEAVE.request to rejoin) | phase-1b | Initial NWK key set at coordinator init; rotation requires reauthing every device — Phase 1b. |
| EZSP secure-boot / NCP firmware update | phase-1b | Out of band via Silicon Labs xmodem flasher; Phase 1 only initialises existing NCP. |
| Texas Instruments Z-Stack / ZNP coordinator (CC2531, CC26x2) | phase-1b | Phase 1 covers EZSP + deCONZ; ZNP serial protocol is additional transport family — straightforward Phase 1b. |
| Wireshark-class packet capture / network diagnostics | phase-1b | Diagnostics UI deferred to Phase 1b once coordinator stability verified. |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | Charter §6.2 / ADR-003 — Linux 7.1+ only. |
| Pre-Zigbee-3.0 (HA / ZLL profile-id routing) | permanent | Charter §8 — no backwards compatibility; Zigbee 3.0 unified profile only. |
| Zigbee2MQTT JS converter ecosystem | permanent | Z2M is GPL-3.0; converters cannot be read or ported; cave-home uses clean-room ZCL path + per-device quirks. |

## Drift notes

None — every claimed symbol exists in source. All 23 [[spec_section]] entries resolve to their declared types/traits/enums in the specified files. Test coverage spans all major components (framing, routing, groups, scenes, OTA, pairing, transport, coordinator). Phase 1 MVP scope (0.45 fill_ratio) is clearly bounded and stage-gated.
