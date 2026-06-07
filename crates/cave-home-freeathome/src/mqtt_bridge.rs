// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Bridge from free@home datapoint updates onto cave-home MQTT topics.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::DatapointUpdate;
    use cave_home_free_home::{ChannelId, DatapointId, DeviceSerial, Direction};

    fn update() -> DatapointUpdate {
        DatapointUpdate::new(
            DeviceSerial::parse("ABB700C12345").expect("serial"),
            ChannelId::new(0),
            DatapointId::new(Direction::Output, 0),
            "1".into(),
        )
    }

    #[test]
    fn state_topic_format() {
        let s = DeviceSerial::parse("ABB700C12345").expect("serial");
        assert_eq!(state_topic(&s), "cave-home/freeathome/ABB700C12345/state");
    }

    #[test]
    fn availability_topic_format() {
        assert_eq!(availability_topic(), "cave-home/freeathome/availability");
    }

    #[test]
    fn payload_carries_value_and_address() {
        let p = state_payload(&update());
        let v: serde_json::Value = serde_json::from_str(&p).expect("json");
        assert_eq!(v["value"], "1");
        assert_eq!(v["address"], "ABB700C12345/ch0000/odp0000");
        assert_eq!(v["channel"], "ch0000");
        assert_eq!(v["datapoint"], "odp0000");
    }

    #[test]
    fn state_is_retained() {
        assert!(STATE_RETAINED);
    }
}
