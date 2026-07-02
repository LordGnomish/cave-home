// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! REST request modelling for the SysAP `fhapi` endpoints.
//!
//! A [`RestRequest`] is a pure description of one call — method, path and
//! optional body — with no transport attached. The async client turns it into
//! a real HTTP request; tests pin the wire shape without a network.

use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial};

/// The HTTP method a request uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// Read.
    Get,
    /// Write a datapoint.
    Put,
}

impl HttpMethod {
    /// The uppercase method token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
        }
    }
}

/// One SysAP REST call, described independently of any HTTP client.
#[derive(Debug, Clone)]
pub enum RestRequest {
    /// `GET configuration` — the full SysAP configuration tree.
    Configuration,
    /// `GET devicelist` — the list of device serials.
    DeviceList,
    /// `GET device/{sysApUuid}/{serial}` — one device's detail.
    Device {
        /// The System Access Point UUID (the configuration/devicelist key).
        sysap_uuid: String,
        /// Owning device.
        serial: DeviceSerial,
    },
    /// `GET datapoint/{sysApUuid}/{serial}.{channel}.{datapoint}` — read a datapoint.
    GetDatapoint {
        /// The System Access Point UUID.
        sysap_uuid: String,
        /// Owning device.
        serial: DeviceSerial,
        /// Channel on the device.
        channel: ChannelId,
        /// Datapoint within the channel.
        datapoint: DatapointId,
    },
    /// `PUT datapoint/{sysApUuid}/{serial}.{channel}.{datapoint}` — write a value.
    SetDatapoint {
        /// The System Access Point UUID.
        sysap_uuid: String,
        /// Owning device.
        serial: DeviceSerial,
        /// Channel on the device.
        channel: ChannelId,
        /// Datapoint within the channel (an input datapoint).
        datapoint: DatapointId,
        /// The wire value to write (e.g. `"50"`).
        value: String,
    },
}

impl RestRequest {
    /// Fetch one device's detail.
    pub fn device(sysap_uuid: impl Into<String>, serial: DeviceSerial) -> Self {
        Self::Device {
            sysap_uuid: sysap_uuid.into(),
            serial,
        }
    }

    /// Read a single datapoint's current value.
    pub fn get_datapoint(
        sysap_uuid: impl Into<String>,
        serial: DeviceSerial,
        channel: ChannelId,
        datapoint: DatapointId,
    ) -> Self {
        Self::GetDatapoint {
            sysap_uuid: sysap_uuid.into(),
            serial,
            channel,
            datapoint,
        }
    }

    /// Write a value to an input datapoint.
    pub fn set_datapoint(
        sysap_uuid: impl Into<String>,
        serial: DeviceSerial,
        channel: ChannelId,
        datapoint: DatapointId,
        value: impl Into<String>,
    ) -> Self {
        Self::SetDatapoint {
            sysap_uuid: sysap_uuid.into(),
            serial,
            channel,
            datapoint,
            value: value.into(),
        }
    }

    /// The HTTP method for this request.
    pub const fn method(&self) -> HttpMethod {
        match self {
            Self::SetDatapoint { .. } => HttpMethod::Put,
            _ => HttpMethod::Get,
        }
    }

    /// The path relative to the REST base URL (no leading slash).
    ///
    /// Datapoint addresses are joined with dots (`serial.channel.datapoint`) and
    /// prefixed with the SysAP UUID, per the fhapi v1 local API.
    pub fn path(&self) -> String {
        match self {
            Self::Configuration => "configuration".to_string(),
            Self::DeviceList => "devicelist".to_string(),
            Self::Device { sysap_uuid, serial } => format!("device/{sysap_uuid}/{serial}"),
            Self::GetDatapoint {
                sysap_uuid,
                serial,
                channel,
                datapoint,
            }
            | Self::SetDatapoint {
                sysap_uuid,
                serial,
                channel,
                datapoint,
                ..
            } => format!("datapoint/{sysap_uuid}/{serial}.{channel}.{datapoint}"),
        }
    }

    /// The request body, if any (only `SetDatapoint` carries one).
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::SetDatapoint { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The full URL given a REST base such as `https://h/fhapi/v1/api/rest`.
    pub fn url(&self, rest_base: &str) -> String {
        format!("{}/{}", rest_base.trim_end_matches('/'), self.path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial, Direction};

    const UUID: &str = "00000000-0000-0000-0000-000000000000";

    fn serial() -> DeviceSerial {
        DeviceSerial::parse("ABB700C12345").expect("serial")
    }

    #[test]
    fn configuration_is_get_no_body() {
        let r = RestRequest::Configuration;
        assert_eq!(r.path(), "configuration");
        assert_eq!(r.method(), HttpMethod::Get);
        assert_eq!(r.body(), None);
    }

    #[test]
    fn devicelist_path() {
        assert_eq!(RestRequest::DeviceList.path(), "devicelist");
    }

    #[test]
    fn device_path_includes_sysap_uuid() {
        let r = RestRequest::device(UUID, serial());
        assert_eq!(
            r.path(),
            "device/00000000-0000-0000-0000-000000000000/ABB700C12345"
        );
    }

    #[test]
    fn get_datapoint_path_is_uuid_and_dotted_address() {
        let r = RestRequest::get_datapoint(
            UUID,
            serial(),
            ChannelId::new(3),
            DatapointId::new(Direction::Input, 0),
        );
        assert_eq!(
            r.path(),
            "datapoint/00000000-0000-0000-0000-000000000000/ABB700C12345.ch0003.idp0000"
        );
        assert_eq!(r.method(), HttpMethod::Get);
    }

    #[test]
    fn set_datapoint_is_put_with_dotted_address_and_body() {
        let r = RestRequest::set_datapoint(
            UUID,
            serial(),
            ChannelId::new(3),
            DatapointId::new(Direction::Input, 1),
            "50",
        );
        assert_eq!(
            r.path(),
            "datapoint/00000000-0000-0000-0000-000000000000/ABB700C12345.ch0003.idp0001"
        );
        assert_eq!(r.method(), HttpMethod::Put);
        assert_eq!(r.body(), Some("50"));
    }

    #[test]
    fn url_joins_base_and_path() {
        let r = RestRequest::Configuration;
        assert_eq!(
            r.url("https://h/fhapi/v1/api/rest"),
            "https://h/fhapi/v1/api/rest/configuration"
        );
    }

    #[test]
    fn method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
    }
}
