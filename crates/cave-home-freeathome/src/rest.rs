// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! REST request modelling for the SysAP `fhapi` endpoints.

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial, Direction};

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
    fn device_path_uses_serial() {
        let r = RestRequest::Device(serial());
        assert_eq!(r.path(), "device/ABB700C12345");
    }

    #[test]
    fn get_datapoint_path() {
        let r = RestRequest::get_datapoint(
            serial(),
            ChannelId::new(3),
            DatapointId::new(Direction::Input, 0),
        );
        assert_eq!(r.path(), "datapoint/ABB700C12345/ch0003/idp0000");
        assert_eq!(r.method(), HttpMethod::Get);
    }

    #[test]
    fn set_datapoint_is_put_with_body() {
        let r = RestRequest::set_datapoint(
            serial(),
            ChannelId::new(3),
            DatapointId::new(Direction::Input, 1),
            "50",
        );
        assert_eq!(r.path(), "datapoint/ABB700C12345/ch0003/idp0001");
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
