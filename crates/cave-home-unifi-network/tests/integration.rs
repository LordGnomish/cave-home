// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// RED-phase integration tests for cave-home-unifi-network. These tests
// pin the public surface that the GREEN-phase implementation must
// satisfy. Mirrors the HA `unifi` integration shape:
//   - controller-mode API client (host / port / site / credentials)
//   - devices (switches, APs) with state + telemetry
//   - clients (wired + wireless)
//   - block-switch / outlet-switch entity surfaces
//   - WebSocket event types
//
// Upstream pin: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
//               (tag 2026.5.2) :: homeassistant/components/unifi/

use cave_home_unifi_network::{
    BlockSwitch, ClientId, ControllerConfig, ControllerEvent, DeviceId, DeviceKind,
    DeviceState, OutletSwitch, PortStat, SiteId, UnifiClient, UnifiController,
    UnifiError, friendly_device_label,
};

#[test]
fn controller_config_defaults_to_v8_https() {
    let cfg = ControllerConfig::new("udmp.local", "admin", "secret");
    assert_eq!(cfg.host, "udmp.local");
    assert_eq!(cfg.port, 8443);
    assert!(cfg.verify_ssl);
    assert_eq!(cfg.site.as_str(), "default");
}

#[test]
fn controller_config_with_site_and_port() {
    let cfg = ControllerConfig::new("udmp.local", "admin", "secret")
        .with_port(443)
        .with_site("guest")
        .with_verify_ssl(false);
    assert_eq!(cfg.port, 443);
    assert_eq!(cfg.site.as_str(), "guest");
    assert!(!cfg.verify_ssl);
}

#[test]
fn site_id_default_is_default() {
    assert_eq!(SiteId::default().as_str(), "default");
}

#[test]
fn device_state_string_matches_ha_table() {
    // Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
    // homeassistant/components/unifi/const.py :: DEVICE_STATES
    assert_eq!(DeviceState::Disconnected.as_str(), "disconnected");
    assert_eq!(DeviceState::Connected.as_str(), "connected");
    assert_eq!(DeviceState::Pending.as_str(), "pending");
    assert_eq!(DeviceState::FirmwareMismatch.as_str(), "firmware_mismatch");
    assert_eq!(DeviceState::Upgrading.as_str(), "upgrading");
    assert_eq!(DeviceState::Provisioning.as_str(), "provisioning");
    assert_eq!(DeviceState::HeartbeatMissed.as_str(), "heartbeat_missed");
    assert_eq!(DeviceState::Adopting.as_str(), "adopting");
    assert_eq!(DeviceState::Deleting.as_str(), "deleting");
    assert_eq!(DeviceState::InformError.as_str(), "inform_error");
    assert_eq!(DeviceState::AdoptionFailed.as_str(), "adoption_failed");
    assert_eq!(DeviceState::Isolated.as_str(), "isolated");
}

#[test]
fn device_state_parse_round_trip() {
    for variant in DeviceState::all() {
        let s = variant.as_str();
        assert_eq!(DeviceState::parse(s), Some(variant));
    }
    assert_eq!(DeviceState::parse("nonsense"), None);
}

#[test]
fn device_kind_renders_grandma_label() {
    // ADR-007: portal must never show MAC / vlan / port IDs as default.
    // friendly_device_label() returns Turkish home-world vocabulary.
    assert_eq!(friendly_device_label(DeviceKind::Switch), "Switch");
    assert_eq!(friendly_device_label(DeviceKind::AccessPoint), "Wi-Fi noktası");
    assert_eq!(friendly_device_label(DeviceKind::Gateway), "Yönlendirici");
    assert_eq!(friendly_device_label(DeviceKind::DreamMachine), "UniFi Dream Machine");
    assert_eq!(friendly_device_label(DeviceKind::Other), "Cihaz");
}

#[test]
fn port_stat_zero_when_disabled() {
    let p = PortStat::idle(1);
    assert_eq!(p.port_idx, 1);
    assert_eq!(p.rx_bytes, 0);
    assert_eq!(p.tx_bytes, 0);
    assert!(!p.poe_enabled);
    assert!(!p.is_uplink);
}

#[test]
fn port_stat_total_bytes() {
    let mut p = PortStat::idle(2);
    p.rx_bytes = 1024;
    p.tx_bytes = 2048;
    assert_eq!(p.total_bytes(), 3072);
}

#[test]
fn block_switch_state_toggle() {
    // Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
    // homeassistant/components/unifi/switch.py :: UnifiBlockClientSwitch
    let id = ClientId::new("aa:bb:cc:dd:ee:ff");
    let mut sw = BlockSwitch::new(id.clone(), "Çocuk telefonu");
    assert_eq!(sw.client.as_str(), "aa:bb:cc:dd:ee:ff");
    assert_eq!(sw.label, "Çocuk telefonu");
    assert!(!sw.blocked);
    sw.set_blocked(true);
    assert!(sw.blocked);
}

#[test]
fn outlet_switch_state_toggle() {
    // Source: home-assistant/core@456202325ac48549bd3c895dc3e69ecd3e2ba6a4
    // homeassistant/components/unifi/switch.py :: UnifiOutletSwitch
    let dev = DeviceId::new("aa:bb:cc:dd:ee:ff");
    let mut o = OutletSwitch::new(dev, 1, "Salon PoE outlet");
    assert!(!o.relay_state);
    o.set_relay_state(true);
    assert!(o.relay_state);
    assert_eq!(o.outlet_idx, 1);
}

#[test]
fn unifi_controller_constructs_with_config() {
    let cfg = ControllerConfig::new("udmp.local", "admin", "secret");
    let _c = UnifiController::new(cfg);
}

#[test]
fn unifi_controller_unauthenticated_when_built() {
    let cfg = ControllerConfig::new("udmp.local", "admin", "secret");
    let c = UnifiController::new(cfg);
    assert!(!c.is_authenticated());
}

#[tokio::test]
async fn unifi_controller_login_against_offline_host_errors() {
    let cfg = ControllerConfig::new("127.0.0.1", "admin", "secret").with_port(1);
    let mut c = UnifiController::new(cfg);
    let err = c.login().await.unwrap_err();
    assert!(matches!(err, UnifiError::Connect(_) | UnifiError::Timeout));
}

#[test]
fn controller_event_websocket_variants() {
    let _ = ControllerEvent::ClientConnected {
        client: ClientId::new("aa:bb:cc:dd:ee:ff"),
    };
    let _ = ControllerEvent::ClientDisconnected {
        client: ClientId::new("aa:bb:cc:dd:ee:ff"),
    };
    let _ = ControllerEvent::DeviceUpgrade {
        device: DeviceId::new("aa:bb:cc:dd:ee:00"),
        from: "1.0".into(),
        to: "1.1".into(),
    };
    let _ = ControllerEvent::PortPoeChange {
        device: DeviceId::new("aa:bb:cc:dd:ee:00"),
        port: 4,
        enabled: true,
    };
}

#[test]
fn client_id_normalises_to_lower_hex() {
    let id = ClientId::new("AA:BB:CC:DD:EE:FF");
    assert_eq!(id.as_str(), "aa:bb:cc:dd:ee:ff");
}

#[test]
fn device_id_normalises_to_lower_hex() {
    let id = DeviceId::new("AA:BB:CC:DD:EE:00");
    assert_eq!(id.as_str(), "aa:bb:cc:dd:ee:00");
}

#[test]
fn unifi_client_constructs() {
    // A wireless client (HA: Client.is_wired == false)
    let c = UnifiClient::new(ClientId::new("aa:bb:cc:dd:ee:ff"), "Anne iPhone", false);
    assert_eq!(c.label, "Anne iPhone");
    assert!(!c.is_wired);
    assert!(!c.blocked);
}
