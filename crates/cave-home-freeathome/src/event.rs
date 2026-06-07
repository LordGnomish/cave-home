// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! WebSocket push-event parsing.

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_free_home::{ChannelId, DatapointId, Direction};

    const WS_FRAME: &str = r#"{
      "00000000-0000-0000-0000-000000000000": {
        "datapoints": {
          "ABB700C12345/ch0000/odp0000": "1",
          "ABB700C12345/ch0000/odp0001": "75"
        },
        "devicesAdded": [],
        "devicesRemoved": []
      }
    }"#;

    #[test]
    fn parses_datapoint_updates() {
        let evs = parse_ws_frame(WS_FRAME).expect("parse");
        let updates: Vec<_> = evs
            .iter()
            .filter_map(FreeAtHomeEvent::as_datapoint_update)
            .collect();
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn datapoint_update_carries_value() {
        let evs = parse_ws_frame(WS_FRAME).expect("parse");
        let u = evs
            .iter()
            .filter_map(FreeAtHomeEvent::as_datapoint_update)
            .find(|u| u.datapoint() == DatapointId::new(Direction::Output, 1))
            .expect("odp0001");
        assert_eq!(u.value(), "75");
        assert_eq!(u.channel(), ChannelId::new(0));
        assert_eq!(u.serial().as_str(), "ABB700C12345");
    }

    #[test]
    fn parse_address_triple() {
        let (s, c, d) =
            parse_datapoint_address("ABB700C12345/ch0003/idp0001").expect("address");
        assert_eq!(s.as_str(), "ABB700C12345");
        assert_eq!(c, ChannelId::new(3));
        assert_eq!(d, DatapointId::new(Direction::Input, 1));
    }

    #[test]
    fn invalid_address_is_skipped_not_fatal() {
        let json = r#"{ "u": { "datapoints": {
            "garbage": "1",
            "ABB700C12345/ch0000/odp0000": "1"
        } } }"#;
        let evs = parse_ws_frame(json).expect("parse");
        assert_eq!(
            evs.iter()
                .filter_map(FreeAtHomeEvent::as_datapoint_update)
                .count(),
            1
        );
    }

    #[test]
    fn empty_frame_yields_no_events() {
        let evs = parse_ws_frame(r#"{ "u": {} }"#).expect("parse");
        assert!(evs.is_empty());
    }

    #[test]
    fn devices_added_and_removed() {
        let json = r#"{ "u": {
            "devicesAdded": ["ABB700C12345"],
            "devicesRemoved": ["ABB700C99999"]
        } }"#;
        let evs = parse_ws_frame(json).expect("parse");
        assert!(evs.iter().any(
            |e| matches!(e, FreeAtHomeEvent::DeviceAdded(s) if s.as_str() == "ABB700C12345")
        ));
        assert!(evs.iter().any(
            |e| matches!(e, FreeAtHomeEvent::DeviceRemoved(s) if s.as_str() == "ABB700C99999")
        ));
    }

    #[test]
    fn malformed_json_errors() {
        assert!(parse_ws_frame("not json").is_err());
    }
}
