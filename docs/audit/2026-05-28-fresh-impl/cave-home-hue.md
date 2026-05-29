# Coverage matrix — cave-home-hue

**Declared:** fill=0.50 · adr_justified=ADR-010 · honest=0.50 · port method: line-by-line.
**Verified:** 38/38 mapped symbols found in source · 67 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Normalize bridge ID | src/util.rs::normalize_bridge_id | yes |
| MAC from bridge ID | src/util.rs::mac_from_bridge_id | yes |
| Hue error enum | src/errors.rs::HueError | yes |
| Error conversion helper | src/errors.rs::from_hue_error | yes |
| Discovered bridge type | src/discovery.rs::DiscoveredHueBridge | yes |
| Discover bridge async | src/discovery.rs::discover_bridge | yes |
| Discover via NUPNP | src/discovery.rs::discover_nupnp | yes |
| Is Hue bridge check | src/discovery.rs::is_hue_bridge | yes |
| Is v2 bridge check | src/discovery.rs::is_v2_bridge | yes |
| V1 API items container | src/v1/api.rs::ApiItems | yes |
| V1 light resource | src/v1/lights.rs::Light | yes |
| V1 lights collection | src/v1/lights.rs::Lights | yes |
| V1 light set state | src/v1/lights.rs::Light::set_state | yes |
| XY color point + gamut | src/v1/lights.rs::XYPoint + GamutType | yes |
| V1 group resource | src/v1/groups.rs::Group | yes |
| V1 group set action | src/v1/groups.rs::Group::set_action | yes |
| All lights group helper | src/v1/groups.rs::get_all_lights_group | yes |
| V1 scene resource | src/v1/scenes.rs::Scene | yes |
| V1 generic sensor | src/v1/sensors.rs::GenericSensor | yes |
| Sensor type constants | src/v1/sensors.rs (module constants) | yes |
| V1 bridge config | src/v1/config.rs::Config | yes |
| V1 bridge API | src/v1/bridge.rs::HueBridgeV1 | yes |
| V2 resource types enum | src/v2/models/resource.rs (ResourceTypes + ResourceIdentifier + SENSOR_RESOURCE_TYPES) | yes |
| V2 resource features | src/v2/models/feature.rs (OnFeature, DimmingFeature, ColorFeature, ColorTemperatureFeature, DynamicsFeature, AlertFeature, ButtonReport, MotionReport) | yes |
| V2 light model | src/v2/models/light.rs (Light + LightPut + LightMetaData + LightMode) | yes |
| V2 scene model | src/v2/models/scene.rs (Scene + ScenePut + SceneRecall + SceneAction) | yes |
| V2 motion model | src/v2/models/motion.rs (Motion + MotionSensingFeature) | yes |
| V2 button model | src/v2/models/button.rs (Button + ButtonFeature) | yes |
| V2 device model | src/v2/models/device.rs (Device + ProductData + DeviceMetaData) | yes |
| V2 room model | src/v2/models/room.rs (Room + RoomMetadata) | yes |
| V2 zone model | src/v2/models/zone.rs::Zone | yes |
| V2 grouped light model | src/v2/models/grouped_light.rs (GroupedLight + GroupedLightPut) | yes |
| V2 base controller | src/v2/controllers/base.rs::ResourcesController | yes |
| V2 lights controller | src/v2/controllers/lights.rs::LightsController | yes |
| V2 scenes controller | src/v2/controllers/scenes.rs::ScenesController | yes |
| V2 event stream parser | src/v2/events.rs (EventStreamParser + EventType + HueEvent) | yes |
| V2 bridge API | src/v2/bridge.rs::HueBridgeV2 | yes |
| Bridge facade + sleep const | src/bridge.rs (HueBridge + HUB_BUSY_SLEEP) | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Hue Entertainment / DTLS streaming | phase-2 | v2 entertainment_configuration + realtime DTLS stream is separate sub-protocol; ADR-010 Open questions #1 |
| v2 controllers: devices, groups (rooms/zones), sensors, config | phase-1b | Models in place; controller wiring + EventStream dispatch deferred to Phase 1b |
| v1 rule engine + schedules + resourcelinks | phase-2 | Niche legacy v1 surfaces; v2 supersedes via behavior_instance |
| Behavior scripts / smart scenes (v2 behavior_instance, smart_scene) | phase-1b | Models will land Phase 1b alongside events controller |
| HomeKit / Matter resources (homekit, matter, matter_fabric) | phase-2 | cave-home handles via separate Matter crate; v2 surface informational only |
| ConfigFlow re-auth UI | phase-1b | LinkButtonNotPressed/Unauthorized surfaced; Portal re-pair wiring deferred |
| Migration of v1 → v2 entries | phase-1b | Single-direction helper; runtime priority low because cave-home defaults new entries to v2 |
| Diagnostics dump | phase-1b | Support bundle snapshot; trivial port once Portal wired |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | ADR-003 — Linux 7.1+ only |
| Round-bridge BLE-pairing flow | phase-1b | ADR-010 mandates Hue Bridge v2 (square) + BLE-pairing first; round bridge nice-to-have |

## Drift notes

None — every claimed symbol exists in source. All 38 [[mapped]] entries verified:
- utilities (2): normalize_bridge_id, mac_from_bridge_id
- errors (2): HueError enum, from_hue_error conversion
- discovery (5): bridge detection and v2 capability checks
- v1 API (9): ApiItems, Light/Lights, light state updates, groups, scenes, sensors, config, HueBridgeV1
- v2 models (15): ResourceTypes, feature structs (On, Dimming, Color, ColorTemperature, Dynamics, Alert, ButtonReport, MotionReport), Light/LightPut/LightMetaData/LightMode, Scene/ScenePut/SceneRecall/SceneAction, Motion, Button, Device/ProductData/DeviceMetaData, Room/RoomMetadata, Zone, GroupedLight/GroupedLightPut
- v2 controllers (4): ResourcesController, LightsController, ScenesController
- v2 events (1): EventStreamParser + EventType + HueEvent
- bridge glue (1): HueBridgeV1 + HueBridgeV2 dual-mode wrapper + HUB_BUSY_SLEEP constant

**Test coverage:** 67 test functions across 19 files; mapped_test entries align with bridge, discovery, v1 (lights/groups/scenes/sensors/config), and v2 (events, models, controllers).
