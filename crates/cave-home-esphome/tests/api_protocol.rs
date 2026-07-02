// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Integration tests (against the public API) for the ESPHome native-API
//! semantic layer: the `message` type registry (api.proto IDs 1–29), the
//! `hash::fnv1_hash` entity-key derivation, the `entity` data model, and the
//! EN/DE/TR `label` descriptions. RED phase — these target items that DO NOT
//! YET EXIST.
//!
//! The FNV-1 vectors are computed independently from the published algorithm
//! (32-bit FNV-1: offset 2166136261, prime 16777619, multiply-then-xor), NOT
//! from this crate.

use cave_home_esphome::entity::{EntityCategory, EntityInfo, EntityKind};
use cave_home_esphome::hash::fnv1_hash;
use cave_home_esphome::label::Lang;
use cave_home_esphome::message::MessageType;

// ---------------------------------------------------------------------------
// FNV-1 entity-key hash (esphome/core/helpers.cpp::fnv1_hash).
// ---------------------------------------------------------------------------

#[test]
fn fnv1_matches_reference_vectors() {
    assert_eq!(fnv1_hash(""), 0x811C_9DC5); // the FNV offset basis
    assert_eq!(fnv1_hash("a"), 0x050C_5D7E);
    assert_eq!(fnv1_hash("living_room_light"), 0xC6F8_1EC9);
    assert_eq!(fnv1_hash("sensor"), 0x75E6_1B1B);
    assert_eq!(fnv1_hash("temperature"), 0x35A1_23F9);
}

#[test]
fn fnv1_is_order_sensitive() {
    assert_ne!(fnv1_hash("ab"), fnv1_hash("ba"));
}

// ---------------------------------------------------------------------------
// Message-type registry (api.proto IDs 1..=29).
// ---------------------------------------------------------------------------

#[test]
fn message_ids_match_api_proto() {
    let cases = [
        (MessageType::HelloRequest, 1u32, "HelloRequest"),
        (MessageType::HelloResponse, 2, "HelloResponse"),
        (MessageType::ConnectRequest, 3, "ConnectRequest"),
        (MessageType::DisconnectRequest, 5, "DisconnectRequest"),
        (MessageType::PingRequest, 7, "PingRequest"),
        (MessageType::PingResponse, 8, "PingResponse"),
        (MessageType::DeviceInfoRequest, 9, "DeviceInfoRequest"),
        (MessageType::DeviceInfoResponse, 10, "DeviceInfoResponse"),
        (MessageType::ListEntitiesRequest, 11, "ListEntitiesRequest"),
        (MessageType::ListEntitiesBinarySensorResponse, 12, "ListEntitiesBinarySensorResponse"),
        (MessageType::ListEntitiesSensorResponse, 16, "ListEntitiesSensorResponse"),
        (MessageType::ListEntitiesSwitchResponse, 17, "ListEntitiesSwitchResponse"),
        (MessageType::ListEntitiesDoneResponse, 19, "ListEntitiesDoneResponse"),
        (MessageType::SubscribeStatesRequest, 20, "SubscribeStatesRequest"),
        (MessageType::BinarySensorStateResponse, 21, "BinarySensorStateResponse"),
        (MessageType::SensorStateResponse, 25, "SensorStateResponse"),
        (MessageType::SwitchStateResponse, 26, "SwitchStateResponse"),
        (MessageType::SubscribeLogsRequest, 28, "SubscribeLogsRequest"),
        (MessageType::SubscribeLogsResponse, 29, "SubscribeLogsResponse"),
    ];
    for (variant, id, name) in cases {
        assert_eq!(variant.id(), id, "{name} id");
        assert_eq!(MessageType::from_id(id), Some(variant), "from_id({id})");
        assert_eq!(variant.name(), name, "name of id {id}");
    }
}

#[test]
fn message_from_unknown_id_is_none() {
    assert_eq!(MessageType::from_id(0), None);
    assert_eq!(MessageType::from_id(30), None);
    assert_eq!(MessageType::from_id(9999), None);
}

#[test]
fn message_id_round_trips_for_whole_known_block() {
    for id in 1..=29u32 {
        let mt = MessageType::from_id(id).expect("1..=29 are all defined");
        assert_eq!(mt.id(), id);
    }
}

// ---------------------------------------------------------------------------
// Entity model.
// ---------------------------------------------------------------------------

#[test]
fn entity_key_is_fnv1_of_object_id() {
    let e = EntityInfo::new(EntityKind::Light, "living_room_light", "Living Room Light");
    assert_eq!(e.key(), fnv1_hash("living_room_light"));
    assert_eq!(e.key(), 0xC6F8_1EC9);
    assert_eq!(e.object_id, "living_room_light");
    assert_eq!(e.name, "Living Room Light");
    assert_eq!(e.kind, EntityKind::Light);
    assert_eq!(e.category, EntityCategory::None); // default
}

#[test]
fn entity_category_ids_match_esphome() {
    assert_eq!(EntityCategory::None.id(), 0);
    assert_eq!(EntityCategory::Config.id(), 1);
    assert_eq!(EntityCategory::Diagnostic.id(), 2);
    assert_eq!(EntityCategory::from_id(0), Some(EntityCategory::None));
    assert_eq!(EntityCategory::from_id(1), Some(EntityCategory::Config));
    assert_eq!(EntityCategory::from_id(2), Some(EntityCategory::Diagnostic));
    assert_eq!(EntityCategory::from_id(3), None);
}

#[test]
fn entity_kind_maps_to_its_list_and_state_messages() {
    // Each entity kind has a ListEntities*Response and a *StateResponse.
    assert_eq!(EntityKind::BinarySensor.list_response(), MessageType::ListEntitiesBinarySensorResponse);
    assert_eq!(EntityKind::BinarySensor.state_response(), MessageType::BinarySensorStateResponse);
    assert_eq!(EntityKind::Sensor.list_response(), MessageType::ListEntitiesSensorResponse);
    assert_eq!(EntityKind::Sensor.state_response(), MessageType::SensorStateResponse);
    assert_eq!(EntityKind::Switch.list_response(), MessageType::ListEntitiesSwitchResponse);
    assert_eq!(EntityKind::Switch.state_response(), MessageType::SwitchStateResponse);
    assert_eq!(EntityKind::Light.list_response(), MessageType::ListEntitiesLightResponse);
    assert_eq!(EntityKind::Light.state_response(), MessageType::LightStateResponse);
}

// ---------------------------------------------------------------------------
// Grandma-friendly EN / DE / TR labels (Charter §2 persona-1, ADR-007).
// ---------------------------------------------------------------------------

#[test]
fn entity_kind_describes_in_three_languages() {
    assert_eq!(EntityKind::Light.describe(Lang::En), "Light");
    assert_eq!(EntityKind::Light.describe(Lang::De), "Licht");
    assert_eq!(EntityKind::Light.describe(Lang::Tr), "Işık");

    assert_eq!(EntityKind::Switch.describe(Lang::En), "Switch");
    assert_eq!(EntityKind::Switch.describe(Lang::De), "Schalter");
    assert_eq!(EntityKind::Switch.describe(Lang::Tr), "Anahtar");
}

#[test]
fn every_kind_has_a_nonempty_label_in_every_language() {
    let kinds = [
        EntityKind::BinarySensor,
        EntityKind::Cover,
        EntityKind::Fan,
        EntityKind::Light,
        EntityKind::Sensor,
        EntityKind::Switch,
        EntityKind::TextSensor,
    ];
    for kind in kinds {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            assert!(!kind.describe(lang).is_empty(), "{kind:?} in {lang:?}");
        }
    }
}

#[test]
fn labels_avoid_protocol_jargon() {
    // A household never sees "protobuf", "varint", "api.proto", "fnv".
    for kind in [EntityKind::Sensor, EntityKind::Light, EntityKind::BinarySensor] {
        for lang in [Lang::En, Lang::De, Lang::Tr] {
            let s = kind.describe(lang).to_lowercase();
            for banned in ["protobuf", "varint", "proto", "fnv", "esphome"] {
                assert!(!s.contains(banned), "{kind:?}/{lang:?} leaked '{banned}'");
            }
        }
    }
}
