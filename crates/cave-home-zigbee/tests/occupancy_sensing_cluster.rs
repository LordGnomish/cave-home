// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee Cluster Library §3.5 (CSA public PDF) only; Z2M source NOT consulted.
//! Occupancy Sensing cluster (0x0406) — ZCL §3.5.
//!
//! RED: references `cave_home_zigbee::occupancy_sensing`, not yet implemented.

use cave_home_zigbee::occupancy_sensing::{
    attribute_id, Occupancy, OccupancySensorType, OccupancyState, SensorTypeBitmap,
    OCCUPANCY_SENSING_CLUSTER_ID,
};

#[test]
fn cluster_id_matches_spec() {
    assert_eq!(OCCUPANCY_SENSING_CLUSTER_ID, 0x0406);
}

#[test]
fn attribute_ids_match_spec() {
    // §3.5.2.2 attribute tables.
    assert_eq!(attribute_id::OCCUPANCY, 0x0000);
    assert_eq!(attribute_id::OCCUPANCY_SENSOR_TYPE, 0x0001);
    assert_eq!(attribute_id::OCCUPANCY_SENSOR_TYPE_BITMAP, 0x0002);
    assert_eq!(attribute_id::PIR_OCCUPIED_TO_UNOCCUPIED_DELAY, 0x0010);
    assert_eq!(attribute_id::PIR_UNOCCUPIED_TO_OCCUPIED_DELAY, 0x0011);
    assert_eq!(attribute_id::PIR_UNOCCUPIED_TO_OCCUPIED_THRESHOLD, 0x0012);
    assert_eq!(attribute_id::ULTRASONIC_OCCUPIED_TO_UNOCCUPIED_DELAY, 0x0020);
    assert_eq!(attribute_id::PHYSICAL_CONTACT_OCCUPIED_TO_UNOCCUPIED_DELAY, 0x0030);
}

#[test]
fn occupancy_bit0_is_occupied() {
    // §3.5.2.2.1: Occupancy bitmap8, bit 0 = occupied.
    assert!(!Occupancy::from_bits(0x00).occupied());
    assert!(Occupancy::from_bits(0x01).occupied());
    // Other bits are reserved and must not flip "occupied".
    assert!(!Occupancy::from_bits(0x02).occupied());
}

#[test]
fn occupancy_constructor_round_trips() {
    assert_eq!(Occupancy::occupied_state(true).bits(), 0x01);
    assert_eq!(Occupancy::occupied_state(false).bits(), 0x00);
    assert!(Occupancy::occupied_state(true).occupied());
}

#[test]
fn sensor_type_round_trips() {
    // §3.5.2.2.2 OccupancySensorType enum8.
    for (v, e) in [
        (0x00u8, OccupancySensorType::Pir),
        (0x01, OccupancySensorType::Ultrasonic),
        (0x02, OccupancySensorType::PirAndUltrasonic),
        (0x03, OccupancySensorType::PhysicalContact),
    ] {
        assert_eq!(OccupancySensorType::from_u8(v).unwrap(), e);
        assert_eq!(e.to_u8(), v);
    }
    assert!(OccupancySensorType::from_u8(0x04).is_err());
}

#[test]
fn sensor_type_bitmap_accessors() {
    // §3.5.2.2.3 OccupancySensorTypeBitmap: bit0 PIR, bit1 ultrasonic, bit2 contact.
    let b = SensorTypeBitmap::from_bits(0b0000_0101);
    assert!(b.pir());
    assert!(!b.ultrasonic());
    assert!(b.physical_contact());

    let b = SensorTypeBitmap::default()
        .with_pir(true)
        .with_ultrasonic(true);
    assert_eq!(b.bits(), 0b0000_0011);
    assert!(b.pir());
    assert!(b.ultrasonic());
    assert!(!b.physical_contact());
}

#[test]
fn state_defaults_to_unoccupied_pir() {
    let s = OccupancyState::new();
    assert!(!s.occupancy.occupied());
    assert_eq!(s.sensor_type, OccupancySensorType::Pir);
}

#[test]
fn state_set_occupied_tracks_attribute() {
    let mut s = OccupancyState::new();
    s.set_occupied(true);
    assert!(s.occupancy.occupied());
    assert_eq!(s.occupancy.bits(), 0x01);
    s.set_occupied(false);
    assert!(!s.occupancy.occupied());
    assert_eq!(s.occupancy.bits(), 0x00);
}
