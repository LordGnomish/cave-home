# Coverage matrix — cave-home-zwave

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 15/15 mapped symbols found in source · 47 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| CC framing: command-class id + command id + body | src/command_class.rs::CommandClass,Command::decode,Command::encode | yes |
| Basic CC (0x20) Set/Get/Report | src/command_class.rs::Command::{BasicSet,BasicGet,BasicReport} | yes |
| Binary Switch CC (0x25) Set/Get/Report | src/command_class.rs::Command::BinarySwitchSet,BinarySwitchGet,BinarySwitchReport | yes |
| Multilevel Switch CC (0x26) Set/Get/Report + Start/Stop Level Change | src/command_class.rs::Command::MultilevelSwitchSet,Get,Report,StartLevelChange,StopLevelChange | yes |
| Binary Sensor CC (0x30) Get/Report | src/command_class.rs::Command::BinarySensorGet,BinarySensorReport | yes |
| Multilevel Sensor CC (0x31) with precision/scale/size | src/command_class.rs::Command::MultilevelSensorReport + src/sensor_decode.rs | yes |
| Precision/scale/size metadata codec + fixed-point value | src/sensor_decode.rs::split_meta,compose_meta,decode,encode | yes |
| Meter CC (0x32) meter type + fixed-point reading | src/command_class.rs::Command::MeterReport | yes |
| Color Switch CC (0x33) component Set/Get/Report | src/command_class.rs::Command::ColorSwitchSet,Get,Report | yes |
| Thermostat Setpoint CC (0x43) type + float encoding | src/command_class.rs::Command::ThermostatSetpointSet,Get,Report | yes |
| Configuration CC (0x70) parameter/size/value Set/Get/Report | src/command_class.rs::Command::ConfigurationSet,Get,Report | yes |
| Notification CC (0x71) type + event report | src/command_class.rs::Command::NotificationReport | yes |
| Battery CC (0x80) level + 0xFF low sentinel | src/command_class.rs::Command::BatteryReport | yes |
| Vendor-neutral typed value model | src/value.rs::Value | yes |
| Node + endpoint addressing + device-role hint | src/address.rs::Address,DeviceRole | yes |
| Truncated/bad-size/out-of-range rejection (no panics) | src/error.rs::ZwaveError | yes |
| Grandma-friendly EN/DE/TR rendering | src/label.rs::describe,Lang | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Z-Wave Serial API frame layer (SOF/ACK/NAK/CAN, checksum) | phase-1b | Hardware/serial-bound frame routing; transport separable from CC engine |
| Z/IP gateway transport | phase-1b | Network-bound I/O adapter onto same CC engine; no new CC logic |
| Inclusion/exclusion + S2 security (DSK/ECDH/CKDF/nonce) | phase-1b | Crypto-bound (Curve25519 ECDH, CCM, CKDF); security envelope separable layer |
| Controller/network management (node list, lifecycle, heal, routing) | phase-1b | Hardware-bound; orthogonal to CC encode/decode |
| Association groups + multi-channel association | phase-1b | Controller/network-management concern; depends on deferred transport layer |
| OTA firmware update (Firmware Update Meta Data CC) | phase-1b | Transport- and lifecycle-bound; depends on serial transport and controller layers |
| cave-home-core entity/state integration + automation triggers | phase-1b | Deferred until cave-home-core entity API stabilizes; Value model already core-agnostic |
| Legacy Command-Class-version compatibility shims | permanent | Charter §7 always-latest + §8 no-backcompat: only current CC versions shipped |

## Drift notes
None — every claimed symbol exists in source.
