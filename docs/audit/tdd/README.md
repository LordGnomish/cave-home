# cave-home TDD coverage-gap audit

Spec-based test-coverage gap analysis across a prioritized batch of
cave-home crates. Read-only pass — each row links a per-crate gap file.
`gpl-agpl-cleanroom` upstreams (Zigbee2MQTT, Frigate) are analyzed from
public spec only; no upstream source was consulted.

**Batch:** 16 crates · **missing behaviors:** 175 (139 strict-TDD-ready).

| Crate | Upstream | License | Cave tests | Missing | TDD-ready |
|---|---|---|--:|--:|--:|
| [`cave-home-air-quality`](cave-home-air-quality-gaps.md) | home-assistant/core (Apache-2.0) | permissive | 39 | 10 | 8 |
| [`cave-home-camera`](cave-home-camera-gaps.md) | blakeblackshear/frigate | gpl-agpl-cleanroom | 63 | 11 | 11 |
| [`cave-home-core`](cave-home-core-gaps.md) | home-assistant/core | permissive | 21 | 8 | 2 |
| [`cave-home-cover`](cave-home-cover-gaps.md) | OpenGarage (Apache-2.0) + ESPHome cover bindings (MIT) + Somfy RTS public RF spec (clean-room) | permissive | 35 | 12 | 7 |
| [`cave-home-display`](cave-home-display-gaps.md) | home-assistant/core (webostv, samsungtv, androidtv, cast integrations) + first-party Portal wall-panel mode | permissive | 44 | 11 | 10 |
| [`cave-home-garden`](cave-home-garden-gaps.md) | home-assistant/core (husqvarna_automower, worx_landroid, weather integrations) | permissive | 36 | 11 | 2 |
| [`cave-home-household`](cave-home-household-gaps.md) | grocy/grocy | permissive | 35 | 12 | 10 |
| [`cave-home-hvac`](cave-home-hvac-gaps.md) | home-assistant/core (climate domain: generic_thermostat, climate entity vocabulary) + Open3EClient (Viessmann ViCare) | permissive | 39 | 12 | 10 |
| [`cave-home-knx`](cave-home-knx-gaps.md) | Public KNX standard (DPT/APCI); xknx (MIT, public-behavior reference); KNXd (GPL-3.0, clean-room recusal) | gpl-agpl-cleanroom | 62 | 12 | 12 |
| [`cave-home-lock`](cave-home-lock-gaps.md) | home-assistant/core (lock-domain integrations, Apache-2.0) + Nuki/SwitchBot public REST APIs | permissive | 40 | 11 | 11 |
| [`cave-home-matter`](cave-home-matter-gaps.md) | project-chip/connectedhomeip (v1.3.0.0) | permissive | 78 | 12 | 11 |
| [`cave-home-notify`](cave-home-notify-gaps.md) | none (first-party ntfy-class semantics) | first-party | 40 | 12 | 10 |
| [`cave-home-water`](cave-home-water-gaps.md) | home-assistant/core (opensprinkler, mbus, modbus integrations) | permissive | 36 | 10 | 9 |
| [`cave-home-wellness`](cave-home-wellness-gaps.md) | home-assistant/core (withings, garmin_connect, fitbit, oura) | permissive | 31 | 10 | 8 |
| [`cave-home-zigbee`](cave-home-zigbee-gaps.md) | Koenkk/zigbee2mqtt | gpl-agpl-cleanroom | 121 | 10 | 7 |
| [`cave-home-zwave`](cave-home-zwave-gaps.md) | Silicon Labs Z-Wave Command Class Specification (public, SDS13781 family) | permissive | 58 | 11 | 11 |

## Method & rules

- Strict TDD: a missing behavior becomes a `test(crate):` failing-test commit
  (verified RED) **before** its `feat(crate):` impl commit (verified GREEN).
- Only genuinely *absent* behaviors are strict-TDD targets; a behavior that is
  already implemented but merely untested is marked not-TDD-ready (test-after).
- Clean-room for GPL/AGPL upstreams extends to tests: cases come from the public
  spec, never from upstream test source.
- License: Apache-2.0; no `cave-runtime` references (strict isolation).
