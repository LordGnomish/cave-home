// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// RED-phase integration tests for cave-home-unifi-access.
//
// Upstream pin: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//               (tag 2026.5.2) :: homeassistant/components/unifi_access/

use cave_home_unifi_access::{
    AccessClient, AccessConfig, AccessError, Door, DoorEvent, DoorEventCategory,
    DoorEventKind, DoorId, DoorLockRule, DoorLockRuleType, DoorPositionStatus,
    EmergencyStatus, LockRelayStatus, friendly_door_label,
    DEFAULT_LOCK_RULE_INTERVAL, MAX_LOCK_RULE_INTERVAL, MIN_LOCK_RULE_INTERVAL,
};

#[test]
fn access_config_uses_api_token() {
    // Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
    // unifi_access/__init__.py — auth uses CONF_API_TOKEN, not user/pass.
    let cfg = AccessConfig::new("access.local", "tok123");
    assert_eq!(cfg.host, "access.local");
    assert_eq!(cfg.api_token, "tok123");
    assert!(!cfg.verify_ssl); // default: skip self-signed verification
}

#[test]
fn lock_rule_interval_bounds() {
    // Source: unifi_access/const.py
    assert_eq!(DEFAULT_LOCK_RULE_INTERVAL, 10);
    assert_eq!(MIN_LOCK_RULE_INTERVAL, 1);
    assert_eq!(MAX_LOCK_RULE_INTERVAL, 480);
}

#[test]
fn lock_rule_normalises_clamped() {
    // HA coordinator._normalize_interval(): clamp to [MIN, MAX], round.
    assert_eq!(DoorLockRule::normalise_interval(None), DEFAULT_LOCK_RULE_INTERVAL);
    assert_eq!(DoorLockRule::normalise_interval(Some(0.0)), MIN_LOCK_RULE_INTERVAL);
    assert_eq!(DoorLockRule::normalise_interval(Some(500.0)), MAX_LOCK_RULE_INTERVAL);
    assert_eq!(DoorLockRule::normalise_interval(Some(15.4)), 15);
    assert_eq!(DoorLockRule::normalise_interval(Some(15.6)), 16);
}

#[test]
fn lock_rule_type_strings() {
    // Source: unifi_access_api.DoorLockRuleType enum.
    assert_eq!(DoorLockRuleType::Lock.as_str(), "lock");
    assert_eq!(DoorLockRuleType::Unlock.as_str(), "unlock");
    assert_eq!(DoorLockRuleType::Reset.as_str(), "reset");
    assert_eq!(DoorLockRuleType::None.as_str(), "none");
}

#[test]
fn lock_rule_type_parse_round_trip() {
    for v in DoorLockRuleType::all() {
        assert_eq!(DoorLockRuleType::parse(v.as_str()), Some(v));
    }
    assert_eq!(DoorLockRuleType::parse("nonsense"), None);
}

#[test]
fn door_constructs_with_locked_default() {
    let d = Door::new(DoorId::new("d1"), "Ön kapı");
    assert_eq!(d.label, "Ön kapı");
    assert_eq!(d.lock_relay, LockRelayStatus::Lock);
    assert_eq!(d.position, DoorPositionStatus::Unknown);
}

#[test]
fn lock_relay_status_strings() {
    assert_eq!(LockRelayStatus::Lock.as_str(), "locked");
    assert_eq!(LockRelayStatus::Unlock.as_str(), "unlocked");
}

#[test]
fn door_position_status_strings() {
    assert_eq!(DoorPositionStatus::Open.as_str(), "open");
    assert_eq!(DoorPositionStatus::Close.as_str(), "close");
    assert_eq!(DoorPositionStatus::Unknown.as_str(), "unknown");
}

#[test]
fn emergency_status_default_clear() {
    let e = EmergencyStatus::default();
    assert!(!e.evacuation);
    assert!(!e.lockdown);
    assert!(e.is_clear());
}

#[test]
fn emergency_lockdown_is_critical() {
    let e = EmergencyStatus {
        evacuation: false,
        lockdown: true,
    };
    assert!(!e.is_clear());
    assert!(e.is_lockdown());
}

#[test]
fn door_event_kind_strings() {
    // Source: unifi_access/coordinator.py _handle_doorbell / _handle_insights_add
    assert_eq!(DoorEventKind::DoorbellRing.as_str(), "ring");
    assert_eq!(DoorEventKind::AccessGranted.as_str(), "access_granted");
    assert_eq!(DoorEventKind::AccessDenied.as_str(), "access_denied");
}

#[test]
fn door_event_category_strings() {
    assert_eq!(DoorEventCategory::Doorbell.as_str(), "doorbell");
    assert_eq!(DoorEventCategory::Access.as_str(), "access");
}

#[test]
fn door_event_construction() {
    let e = DoorEvent {
        door: DoorId::new("d1"),
        category: DoorEventCategory::Doorbell,
        kind: DoorEventKind::DoorbellRing,
        actor: None,
        authentication: None,
    };
    assert_eq!(e.category, DoorEventCategory::Doorbell);
}

#[test]
fn friendly_door_label_appends_kapi() {
    // ADR-007 — portal says "Ön kapı", never the door GUID.
    assert_eq!(friendly_door_label("Ön"), "Ön kapı");
    assert_eq!(friendly_door_label("Salon"), "Salon kapı");
    assert_eq!(friendly_door_label(""), "Adsız kapı");
}

#[test]
fn access_client_unauthenticated_initially() {
    let cfg = AccessConfig::new("access.local", "tok");
    let c = AccessClient::new(cfg);
    assert!(!c.is_authenticated());
}

#[tokio::test]
async fn access_client_login_against_offline_host_errors() {
    let cfg = AccessConfig::new("127.0.0.1", "tok").with_port(1);
    let mut c = AccessClient::new(cfg);
    let err = c.login().await.unwrap_err();
    assert!(matches!(err, AccessError::Connect(_) | AccessError::Timeout));
}
