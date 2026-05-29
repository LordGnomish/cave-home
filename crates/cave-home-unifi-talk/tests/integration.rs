// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// Cross-module integration tests for cave-home-unifi-talk: the call-control
// engine wired end-to-end (route → ring → answer → talk → log → label),
// exercising the public surface the way a transport adapter would.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use cave_home_unifi_talk::routing::RoutingPrefs;
use cave_home_unifi_talk::{
    BusinessHours, CallDirection, CallEvent, CallLog, CallMachine, CallRecord, CallState,
    DeviceId, DeviceKind, Disposition, Extension, ExtensionError, Lang, Minute, RingStrategy,
    RouteOutcome, TalkDevice, label, route_call,
};

#[test]
fn front_door_call_answered_and_logged_end_to_end() {
    // The household has a front-door intercom that maps to extension 200.
    let door = TalkDevice::new(DeviceId(1), "Front door", DeviceKind::Doorbell);
    let ext = Extension::new("200", "Front door", door.id()).unwrap();

    // Midday, no DND: routing rings the extension.
    let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());
    let route =
        route_call(&ext, &RoutingPrefs::default(), hours, Minute::at(12, 0).unwrap(), false);
    assert!(matches!(route, RouteOutcome::Ring(_)));

    // The intercom rings; the household sees a friendly line.
    let mut call = CallMachine::new(30, Disposition::Voicemail);
    assert_eq!(call.apply(CallEvent::Incoming, 0).unwrap(), CallState::Ringing);
    assert_eq!(
        label::incoming_from_device(door.kind(), Lang::En),
        "The front-door intercom is calling"
    );

    // Someone picks up, talks for 45 s, and hangs up.
    call.apply(CallEvent::Answer, 3).unwrap();
    call.apply(CallEvent::Connected, 4).unwrap();
    assert_eq!(call.apply(CallEvent::Hangup, 49).unwrap(), CallState::Ended);

    // The call lands in the history with the talk time.
    let mut log = CallLog::with_capacity(50);
    log.record(CallRecord::answered("Front door", "200", CallDirection::Incoming, 45, 0));
    assert_eq!(log.missed_count(), 0);
    assert_eq!(log.total_talk_time(), 45);
}

#[test]
fn after_hours_unanswered_call_rolls_to_voicemail_and_logs_history() {
    let ext = Extension::new("200", "Front door", DeviceId(1)).unwrap();
    let hours = BusinessHours::new(Minute::at(8, 0).unwrap(), Minute::at(22, 0).unwrap());

    // 23:30 is after hours: a non-emergency call does not ring.
    let route =
        route_call(&ext, &RoutingPrefs::default(), hours, Minute::at(23, 30).unwrap(), false);
    assert_eq!(route, RouteOutcome::Voicemail);

    // Model the unanswered ring rolling to voicemail.
    let mut call = CallMachine::new(20, Disposition::Voicemail);
    call.apply(CallEvent::Incoming, 1000).unwrap();
    assert_eq!(call.tick(1020), CallState::Voicemail);

    // The history shows a voicemail (not counted as a missed call), and the
    // grandma-friendly line names the caller.
    let mut log = CallLog::with_capacity(50);
    log.record(CallRecord::unanswered(
        "the gate",
        "200",
        CallDirection::Incoming,
        CallState::Voicemail,
        1000,
    ));
    assert_eq!(log.missed_count(), 0);
    assert_eq!(label::missed_from("the gate", Lang::En), "Missed call from the gate");
}

#[test]
fn dnd_routes_to_voicemail_but_emergency_rings_through() {
    let ext = Extension::new("101", "Bedroom", DeviceId(2)).unwrap();
    let open = BusinessHours::always_open();
    let noon = Minute::at(12, 0).unwrap();

    let quiet = route_call(&ext, &RoutingPrefs::dnd(), open, noon, false);
    assert_eq!(quiet, RouteOutcome::Voicemail);
    assert_eq!(label::do_not_disturb_on(Lang::En), "Do not disturb is on");

    let emergency = route_call(&ext, &RoutingPrefs::dnd(), open, noon, true);
    assert!(matches!(emergency, RouteOutcome::Ring(_)));
}

#[test]
fn bad_extension_number_is_rejected() {
    assert_eq!(
        Extension::new("not-a-number", "x", DeviceId(1)),
        Err(ExtensionError::BadNumber)
    );
}

#[test]
fn ring_strategy_is_carried_through_routing() {
    // A single-extension route is sequential by construction; this guards the
    // public RingStrategy enum stays wired through the RouteOutcome surface.
    let ext = Extension::new("101", "Kitchen", DeviceId(1)).unwrap();
    let route = route_call(
        &ext,
        &RoutingPrefs::default(),
        BusinessHours::always_open(),
        Minute::at(9, 0).unwrap(),
        false,
    );
    match route {
        RouteOutcome::Ring(plan) => assert_eq!(plan.strategy, RingStrategy::Sequential),
        other => panic!("expected ring, got {other:?}"),
    }
}
