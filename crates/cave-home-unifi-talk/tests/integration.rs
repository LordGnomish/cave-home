// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// RED-phase integration tests for cave-home-unifi-talk.
//
// HA core has no `unifi_talk` integration as of 2026.5.2; this crate
// ports against the public Ubiquiti Talk REST surface only. ADR-009
// caps parity at whatever Ubiquiti documents publicly.
//
// Upstream pin: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//               (tag 2026.5.2) — no homeassistant/components/unifi_talk
//               exists; this is the boundary of HA upstream parity.

use cave_home_unifi_talk::{
    CallControlVerb, CallEvent, CallEventKind, CallId, IncomingCall, PhoneId, PhoneRoster,
    TalkClient, TalkConfig, TalkError, TalkPhone, friendly_phone_label,
};

#[test]
fn talk_config_uses_api_token_auth() {
    // UniFi Talk REST surface uses API token auth like UniFi Access.
    let cfg = TalkConfig::new("talk.local", "tok123");
    assert_eq!(cfg.host, "talk.local");
    assert_eq!(cfg.api_token, "tok123");
    assert_eq!(cfg.port, 443);
    assert!(!cfg.verify_ssl);
}

#[test]
fn talk_phone_construction() {
    let p = TalkPhone::new(PhoneId::new("phone-1"), "Mutfak interkomu", "+90 ...");
    assert_eq!(p.label, "Mutfak interkomu");
    assert_eq!(p.extension, "+90 ...");
    assert!(!p.is_busy);
}

#[test]
fn phone_roster_add_and_lookup() {
    let mut r = PhoneRoster::new();
    r.add(TalkPhone::new(PhoneId::new("p1"), "Mutfak", "100"));
    r.add(TalkPhone::new(PhoneId::new("p2"), "Salon", "101"));
    assert_eq!(r.len(), 2);
    let p = r.get(&PhoneId::new("p1")).unwrap();
    assert_eq!(p.extension, "100");
    assert!(r.get(&PhoneId::new("missing")).is_none());
}

#[test]
fn incoming_call_construction() {
    let c = IncomingCall {
        id: CallId::new("call-1"),
        from_extension: "200".into(),
        to_phone: PhoneId::new("p1"),
        from_display_name: Some("Komşu".into()),
    };
    assert_eq!(c.from_extension, "200");
    assert_eq!(c.from_display_name.as_deref(), Some("Komşu"));
}

#[test]
fn call_event_kind_strings() {
    assert_eq!(CallEventKind::Incoming.as_str(), "incoming");
    assert_eq!(CallEventKind::Answered.as_str(), "answered");
    assert_eq!(CallEventKind::Declined.as_str(), "declined");
    assert_eq!(CallEventKind::Ended.as_str(), "ended");
    assert_eq!(CallEventKind::Missed.as_str(), "missed");
    assert_eq!(CallEventKind::Transferred.as_str(), "transferred");
}

#[test]
fn call_event_kind_parse_round_trip() {
    for v in CallEventKind::all() {
        assert_eq!(CallEventKind::parse(v.as_str()), Some(v));
    }
    assert_eq!(CallEventKind::parse("nope"), None);
}

#[test]
fn call_event_construction() {
    let e = CallEvent {
        call: CallId::new("c1"),
        phone: PhoneId::new("p1"),
        kind: CallEventKind::Incoming,
    };
    assert_eq!(e.kind, CallEventKind::Incoming);
}

#[test]
fn call_control_verbs() {
    // The Phase 1 surface is "answer / decline / transfer / end" —
    // those four verbs are the ones the portal mounts as buttons on
    // the incoming-call tile.
    assert_eq!(CallControlVerb::Answer.as_str(), "answer");
    assert_eq!(CallControlVerb::Decline.as_str(), "decline");
    assert_eq!(CallControlVerb::Transfer.as_str(), "transfer");
    assert_eq!(CallControlVerb::End.as_str(), "end");
}

#[test]
fn friendly_phone_label_appends_interkomu() {
    assert_eq!(friendly_phone_label("Mutfak"), "Mutfak interkomu");
    assert_eq!(friendly_phone_label(""), "Adsız interkom");
}

#[test]
fn talk_client_unauthenticated_initially() {
    let c = TalkClient::new(TalkConfig::new("h", "tok"));
    assert!(!c.is_authenticated());
}

#[tokio::test]
async fn talk_client_login_against_offline_host_errors() {
    let cfg = TalkConfig::new("127.0.0.1", "tok").with_port(1);
    let mut c = TalkClient::new(cfg);
    let err = c.login().await.unwrap_err();
    assert!(matches!(err, TalkError::Connect(_) | TalkError::Timeout));
}
