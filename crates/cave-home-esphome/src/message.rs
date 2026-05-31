// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! `ESPHome` native-API message-type registry.
//!
//! Every frame carries a numeric message type (the `id` of a message in the
//! public `api.proto`). This enum is the registry for the core block — the
//! connection handshake, device-info, entity listing, state streaming and log
//! subscription messages, ids `1..=29`. Later message ids (Home Assistant
//! service calls, time sync, the per-domain command messages, etc.) are
//! Phase-2; see `parity.manifest.toml`.

/// A native-API message type, identified by its `api.proto` id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageType {
    /// id 1
    HelloRequest,
    /// id 2
    HelloResponse,
    /// id 3
    ConnectRequest,
    /// id 4
    ConnectResponse,
    /// id 5
    DisconnectRequest,
    /// id 6
    DisconnectResponse,
    /// id 7
    PingRequest,
    /// id 8
    PingResponse,
    /// id 9
    DeviceInfoRequest,
    /// id 10
    DeviceInfoResponse,
    /// id 11
    ListEntitiesRequest,
    /// id 12
    ListEntitiesBinarySensorResponse,
    /// id 13
    ListEntitiesCoverResponse,
    /// id 14
    ListEntitiesFanResponse,
    /// id 15
    ListEntitiesLightResponse,
    /// id 16
    ListEntitiesSensorResponse,
    /// id 17
    ListEntitiesSwitchResponse,
    /// id 18
    ListEntitiesTextSensorResponse,
    /// id 19
    ListEntitiesDoneResponse,
    /// id 20
    SubscribeStatesRequest,
    /// id 21
    BinarySensorStateResponse,
    /// id 22
    CoverStateResponse,
    /// id 23
    FanStateResponse,
    /// id 24
    LightStateResponse,
    /// id 25
    SensorStateResponse,
    /// id 26
    SwitchStateResponse,
    /// id 27
    TextSensorStateResponse,
    /// id 28
    SubscribeLogsRequest,
    /// id 29
    SubscribeLogsResponse,
}

impl MessageType {
    /// The `api.proto` numeric id this message is framed with.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::HelloRequest => 1,
            Self::HelloResponse => 2,
            Self::ConnectRequest => 3,
            Self::ConnectResponse => 4,
            Self::DisconnectRequest => 5,
            Self::DisconnectResponse => 6,
            Self::PingRequest => 7,
            Self::PingResponse => 8,
            Self::DeviceInfoRequest => 9,
            Self::DeviceInfoResponse => 10,
            Self::ListEntitiesRequest => 11,
            Self::ListEntitiesBinarySensorResponse => 12,
            Self::ListEntitiesCoverResponse => 13,
            Self::ListEntitiesFanResponse => 14,
            Self::ListEntitiesLightResponse => 15,
            Self::ListEntitiesSensorResponse => 16,
            Self::ListEntitiesSwitchResponse => 17,
            Self::ListEntitiesTextSensorResponse => 18,
            Self::ListEntitiesDoneResponse => 19,
            Self::SubscribeStatesRequest => 20,
            Self::BinarySensorStateResponse => 21,
            Self::CoverStateResponse => 22,
            Self::FanStateResponse => 23,
            Self::LightStateResponse => 24,
            Self::SensorStateResponse => 25,
            Self::SwitchStateResponse => 26,
            Self::TextSensorStateResponse => 27,
            Self::SubscribeLogsRequest => 28,
            Self::SubscribeLogsResponse => 29,
        }
    }

    /// The message type for a wire id, or `None` for an id outside the known
    /// `1..=29` block.
    #[must_use]
    pub const fn from_id(id: u32) -> Option<Self> {
        let mt = match id {
            1 => Self::HelloRequest,
            2 => Self::HelloResponse,
            3 => Self::ConnectRequest,
            4 => Self::ConnectResponse,
            5 => Self::DisconnectRequest,
            6 => Self::DisconnectResponse,
            7 => Self::PingRequest,
            8 => Self::PingResponse,
            9 => Self::DeviceInfoRequest,
            10 => Self::DeviceInfoResponse,
            11 => Self::ListEntitiesRequest,
            12 => Self::ListEntitiesBinarySensorResponse,
            13 => Self::ListEntitiesCoverResponse,
            14 => Self::ListEntitiesFanResponse,
            15 => Self::ListEntitiesLightResponse,
            16 => Self::ListEntitiesSensorResponse,
            17 => Self::ListEntitiesSwitchResponse,
            18 => Self::ListEntitiesTextSensorResponse,
            19 => Self::ListEntitiesDoneResponse,
            20 => Self::SubscribeStatesRequest,
            21 => Self::BinarySensorStateResponse,
            22 => Self::CoverStateResponse,
            23 => Self::FanStateResponse,
            24 => Self::LightStateResponse,
            25 => Self::SensorStateResponse,
            26 => Self::SwitchStateResponse,
            27 => Self::TextSensorStateResponse,
            28 => Self::SubscribeLogsRequest,
            29 => Self::SubscribeLogsResponse,
            _ => return None,
        };
        Some(mt)
    }

    /// The message's `api.proto` name (e.g. `"HelloRequest"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::HelloRequest => "HelloRequest",
            Self::HelloResponse => "HelloResponse",
            Self::ConnectRequest => "ConnectRequest",
            Self::ConnectResponse => "ConnectResponse",
            Self::DisconnectRequest => "DisconnectRequest",
            Self::DisconnectResponse => "DisconnectResponse",
            Self::PingRequest => "PingRequest",
            Self::PingResponse => "PingResponse",
            Self::DeviceInfoRequest => "DeviceInfoRequest",
            Self::DeviceInfoResponse => "DeviceInfoResponse",
            Self::ListEntitiesRequest => "ListEntitiesRequest",
            Self::ListEntitiesBinarySensorResponse => "ListEntitiesBinarySensorResponse",
            Self::ListEntitiesCoverResponse => "ListEntitiesCoverResponse",
            Self::ListEntitiesFanResponse => "ListEntitiesFanResponse",
            Self::ListEntitiesLightResponse => "ListEntitiesLightResponse",
            Self::ListEntitiesSensorResponse => "ListEntitiesSensorResponse",
            Self::ListEntitiesSwitchResponse => "ListEntitiesSwitchResponse",
            Self::ListEntitiesTextSensorResponse => "ListEntitiesTextSensorResponse",
            Self::ListEntitiesDoneResponse => "ListEntitiesDoneResponse",
            Self::SubscribeStatesRequest => "SubscribeStatesRequest",
            Self::BinarySensorStateResponse => "BinarySensorStateResponse",
            Self::CoverStateResponse => "CoverStateResponse",
            Self::FanStateResponse => "FanStateResponse",
            Self::LightStateResponse => "LightStateResponse",
            Self::SensorStateResponse => "SensorStateResponse",
            Self::SwitchStateResponse => "SwitchStateResponse",
            Self::TextSensorStateResponse => "TextSensorStateResponse",
            Self::SubscribeLogsRequest => "SubscribeLogsRequest",
            Self::SubscribeLogsResponse => "SubscribeLogsResponse",
        }
    }
}
