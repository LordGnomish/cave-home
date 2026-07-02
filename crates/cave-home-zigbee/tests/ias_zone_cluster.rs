// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §8.2 (CSA public PDF) only; Z2M source NOT consulted.
//! IAS Zone cluster (0x0500) — ZCL §8.2.
//!
//! RED: references `cave_home_zigbee::ias_zone`, not yet implemented. Wire
//! vectors hand-computed from the ZCL §8.2 command/attribute tables.

use cave_home_zigbee::ias_zone::{
    attribute_id, command_id, notification_id, EnrollResponseCode, IasZoneCommand,
    ZoneEnrollRequest, ZoneStatus, ZoneStatusChangeNotification, ZoneType, IAS_ZONE_CLUSTER_ID,
};

#[test]
fn cluster_and_ids_match_spec() {
    assert_eq!(IAS_ZONE_CLUSTER_ID, 0x0500);
    // §8.2.2.4 received commands (client→server).
    assert_eq!(command_id::ZONE_ENROLL_RESPONSE, 0x00);
    assert_eq!(command_id::INITIATE_NORMAL_OPERATION_MODE, 0x01);
    assert_eq!(command_id::INITIATE_TEST_MODE, 0x02);
    // §8.2.2.5 generated commands (server→client).
    assert_eq!(notification_id::ZONE_STATUS_CHANGE_NOTIFICATION, 0x00);
    assert_eq!(notification_id::ZONE_ENROLL_REQUEST, 0x01);
}

#[test]
fn attribute_ids_match_spec() {
    assert_eq!(attribute_id::ZONE_STATE, 0x0000);
    assert_eq!(attribute_id::ZONE_TYPE, 0x0001);
    assert_eq!(attribute_id::ZONE_STATUS, 0x0002);
    assert_eq!(attribute_id::IAS_CIE_ADDRESS, 0x0010);
    assert_eq!(attribute_id::ZONE_ID, 0x0011);
}

#[test]
fn zone_type_known_values() {
    // §8.2.2.2.1 Table 8-4.
    assert_eq!(ZoneType::from_u16(0x0000), ZoneType::StandardCie);
    assert_eq!(ZoneType::from_u16(0x000d), ZoneType::MotionSensor);
    assert_eq!(ZoneType::from_u16(0x0015), ZoneType::ContactSwitch);
    assert_eq!(ZoneType::from_u16(0x0028), ZoneType::FireSensor);
    assert_eq!(ZoneType::from_u16(0x002a), ZoneType::WaterSensor);
    assert_eq!(ZoneType::from_u16(0x002b), ZoneType::CarbonMonoxide);
    assert_eq!(ZoneType::from_u16(0x0226), ZoneType::GlassBreak);
    assert_eq!(ZoneType::from_u16(0xffff), ZoneType::Invalid);
    // Unknown → Other.
    assert_eq!(ZoneType::from_u16(0x1234), ZoneType::Other(0x1234));
}

#[test]
fn zone_type_round_trips() {
    for raw in [0x0000u16, 0x000d, 0x0015, 0x0028, 0x002a, 0x002b, 0x0226, 0xffff, 0x1234] {
        assert_eq!(ZoneType::from_u16(raw).to_u16(), raw);
    }
}

#[test]
fn zone_status_bit_accessors() {
    // §8.2.2.2.2 Table 8-5 bit layout.
    let s = ZoneStatus::from_bits(0b0000_0000_0000_0001); // bit0 Alarm1
    assert!(s.alarm1());
    assert!(!s.alarm2());

    let s = ZoneStatus::from_bits(0b0000_0000_0000_0100); // bit2 Tamper
    assert!(s.tamper());
    assert!(!s.alarm1());

    let s = ZoneStatus::from_bits(0b0000_0000_0000_1000); // bit3 Battery low
    assert!(s.battery_low());

    let s = ZoneStatus::from_bits(0b0000_0010_0000_0000); // bit9 Battery defect
    assert!(s.battery_defect());
    assert!(!s.battery_low());
}

#[test]
fn zone_status_builder_round_trips() {
    let s = ZoneStatus::default()
        .with_alarm1(true)
        .with_tamper(true)
        .with_battery_low(true);
    assert!(s.alarm1());
    assert!(s.tamper());
    assert!(s.battery_low());
    // bits 0 + 2 + 3 = 0x000d
    assert_eq!(s.bits(), 0x000d);
    assert_eq!(ZoneStatus::from_bits(s.bits()), s);
}

#[test]
fn parse_zone_status_change_notification() {
    // §8.2.2.5.1: zone_status (u16 LE) + extended_status (u8) + zone_id (u8) + delay (u16 LE).
    // status = 0x0001 (Alarm1), extended = 0x00, zone_id = 0x0a, delay = 0x0000.
    let n = ZoneStatusChangeNotification::parse(&[0x01, 0x00, 0x00, 0x0a, 0x00, 0x00]).unwrap();
    assert!(n.zone_status.alarm1());
    assert_eq!(n.extended_status, 0x00);
    assert_eq!(n.zone_id, 0x0a);
    assert_eq!(n.delay, 0);
    // round-trip
    assert_eq!(
        ZoneStatusChangeNotification::parse(&n.encode()).unwrap(),
        n
    );
}

#[test]
fn parse_zone_status_change_notification_rejects_truncated() {
    assert!(ZoneStatusChangeNotification::parse(&[0x01, 0x00, 0x00]).is_err());
}

#[test]
fn parse_zone_enroll_request() {
    // §8.2.2.5.2: zone_type (u16 LE) + manufacturer_code (u16 LE).
    // type = 0x000d (motion), mfr = 0x100b.
    let r = ZoneEnrollRequest::parse(&[0x0d, 0x00, 0x0b, 0x10]).unwrap();
    assert_eq!(r.zone_type, ZoneType::MotionSensor);
    assert_eq!(r.manufacturer_code, 0x100b);
    assert_eq!(ZoneEnrollRequest::parse(&r.encode()).unwrap(), r);
}

#[test]
fn enroll_response_code_round_trips() {
    // §8.2.2.4.1 Table 8-8.
    for (v, e) in [
        (0x00u8, EnrollResponseCode::Success),
        (0x01, EnrollResponseCode::NotSupported),
        (0x02, EnrollResponseCode::NoEnrollPermit),
        (0x03, EnrollResponseCode::TooManyZones),
    ] {
        assert_eq!(EnrollResponseCode::from_u8(v).unwrap(), e);
        assert_eq!(e.to_u8(), v);
    }
    assert!(EnrollResponseCode::from_u8(0x04).is_err());
}

#[test]
fn parse_received_commands() {
    // Zone Enroll Response: code (u8) + zone_id (u8).
    let cmd = IasZoneCommand::parse(command_id::ZONE_ENROLL_RESPONSE, &[0x00, 0x0a]).unwrap();
    assert_eq!(
        cmd,
        IasZoneCommand::ZoneEnrollResponse {
            code: EnrollResponseCode::Success,
            zone_id: 0x0a,
        }
    );

    // Initiate Normal Operation Mode: no payload.
    assert_eq!(
        IasZoneCommand::parse(command_id::INITIATE_NORMAL_OPERATION_MODE, &[]).unwrap(),
        IasZoneCommand::InitiateNormalOperationMode
    );

    // Initiate Test Mode: duration (u8) + sensitivity (u8).
    let cmd = IasZoneCommand::parse(command_id::INITIATE_TEST_MODE, &[0x1e, 0x02]).unwrap();
    assert_eq!(
        cmd,
        IasZoneCommand::InitiateTestMode {
            test_mode_duration: 0x1e,
            current_zone_sensitivity_level: 0x02,
        }
    );
}

#[test]
fn received_command_round_trips() {
    for cmd in [
        IasZoneCommand::ZoneEnrollResponse {
            code: EnrollResponseCode::TooManyZones,
            zone_id: 5,
        },
        IasZoneCommand::InitiateNormalOperationMode,
        IasZoneCommand::InitiateTestMode {
            test_mode_duration: 60,
            current_zone_sensitivity_level: 1,
        },
    ] {
        let id = cmd.command_id();
        let payload = cmd.encode_payload();
        assert_eq!(IasZoneCommand::parse(id, &payload).unwrap(), cmd);
    }
}

#[test]
fn received_command_rejects_truncated_and_unknown() {
    assert!(IasZoneCommand::parse(command_id::ZONE_ENROLL_RESPONSE, &[0x00]).is_err());
    assert!(IasZoneCommand::parse(command_id::INITIATE_TEST_MODE, &[0x1e]).is_err());
    assert!(IasZoneCommand::parse(0x7f, &[]).is_err());
}
