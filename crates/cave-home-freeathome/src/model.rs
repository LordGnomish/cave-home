// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Data-transfer objects for the SysAP REST JSON responses.

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::Function;

    const CONFIG_JSON: &str = r#"{
      "00000000-0000-0000-0000-000000000000": {
        "devices": {
          "ABB700C12345": {
            "displayName": "Living Room Light",
            "channels": {
              "ch0000": {
                "functionID": "0012",
                "displayName": "Dimmer",
                "datapoints": {
                  "odp0000": { "value": "1" },
                  "odp0001": { "value": "80" }
                }
              }
            }
          }
        },
        "floorplan": { "ignored": true }
      }
    }"#;

    const DEVICELIST_JSON: &str =
        r#"{ "00000000-0000-0000-0000-000000000000": ["ABB700C12345", "ABB700C99999"] }"#;

    #[test]
    fn parse_devicelist_serials() {
        let dl = DeviceListResponse::parse(DEVICELIST_JSON).expect("parse");
        assert_eq!(dl.serials(), vec!["ABB700C12345", "ABB700C99999"]);
    }

    #[test]
    fn parse_configuration_device_name() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_uuid, sysap) = cfg.first_sysap().expect("one sysap");
        let dev = sysap.devices.get("ABB700C12345").expect("device");
        assert_eq!(dev.display_name.as_deref(), Some("Living Room Light"));
    }

    #[test]
    fn channel_function_resolves() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        let ch = s
            .devices
            .get("ABB700C12345")
            .and_then(|d| d.channels.get("ch0000"))
            .expect("channel");
        assert_eq!(ch.function(), Some(Function::DimmingActuator));
    }

    #[test]
    fn datapoint_value_extracted() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        let dp = s
            .devices
            .get("ABB700C12345")
            .and_then(|d| d.channels.get("ch0000"))
            .and_then(|c| c.datapoints.get("odp0001"))
            .expect("datapoint");
        assert_eq!(dp.value.as_deref(), Some("80"));
    }

    #[test]
    fn function_id_parses_hex_with_and_without_prefix() {
        assert_eq!(parse_function_id("0012"), Some(0x0012));
        assert_eq!(parse_function_id("0x0007"), Some(0x0007));
        assert_eq!(parse_function_id("zz"), None);
    }

    #[test]
    fn missing_display_name_tolerated() {
        let json = r#"{ "u": { "devices": { "S": { "channels": {} } } } }"#;
        let cfg = ConfigurationResponse::parse(json).expect("parse");
        let (_u, s) = cfg.first_sysap().expect("sysap");
        assert!(s.devices.get("S").expect("dev").display_name.is_none());
    }

    #[test]
    fn sysap_count_is_one() {
        let cfg = ConfigurationResponse::parse(CONFIG_JSON).expect("parse");
        assert_eq!(cfg.sysap_count(), 1);
    }
}
