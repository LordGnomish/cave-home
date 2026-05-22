// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
// CLEAN-ROOM: KNX/IP public specification reference only.
// Upstream knxd source NOT consulted. GPL contamination prevented by design.
//
// This module implements a KNXd-equivalent KNXnet/IP gateway surface (the
// daemon that brokers KNX/IP traffic between local clients and the KNX
// medium). KNXd itself is GPL-3.0 and license-incompatible with the
// Apache-2.0 cave-home licence; therefore this file is derived EXCLUSIVELY
// from the KNX Association's public specification material:
//
//   * KNX Standard 03_08 (KNXnet/IP) public summary pages (knx.org).
//   * The KNX/IP service-code table — public information; the same table
//     also appears in MIT-licensed xknx upstream's `knxip_enum.py` header,
//     but the relevant spec is the KNX Association document, not the xknx
//     port of it.
//   * Public registered-IANA UDP port `3671` and KNX/IP multicast group
//     `224.0.23.12` for routing.
//
// The Wireshark KNX dissector (also GPL) was NOT consulted at any point.
// No KNXd source file was opened or grep'd in producing this gateway.
//
//! KNXnet/IP gateway daemon — clean-room KNXd-equivalent.
//!
//! Bridges between an in-process bus (`tokio::sync::broadcast` queue of
//! [`crate::telegram::Telegram`] events) and the KNX/IP wire format:
//!
//! * **Routing endpoint** — UDP/multicast 224.0.23.12:3671 with
//!   `RoutingIndication` frames carrying cEMI payloads. Fire-and-forget,
//!   no per-frame ACK (routing busy / lost-message frames feed
//!   observability).
//! * **Tunnelling endpoint** — UDP point-to-point with `ConnectRequest` /
//!   `ConnectResponse` / `ConnectionStateRequest` keepalive every 60 s /
//!   `TunnellingRequest`+`TunnellingAck` and `DisconnectRequest` shutdown.
//!
//! Phase 1 Routing-only — tunnelling client implementation lands in
//! Phase 2 (KNX/IP Secure handshake is also Phase 2). Tunnelling code that
//! lives here today is the encode/decode surface plus the connect-response
//! parsing, exercised by tests. The runtime tunnelling state machine is
//! tracked in the Phase 2 backlog of ADR-011 with a dedicated test gate.

use std::sync::atomic::{AtomicU8, Ordering};

use parking_lot::Mutex;
use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::cemi::cemi_to_telegram;
use crate::error::{KnxError, Result};
use crate::knxip::{
    build_frame, ConnectionStateRequest, DisconnectRequest, ErrorCode, Hpai, KnxIpHeader,
    KnxIpServiceType, RoutingIndication, TunnellingRequest,
};
use crate::telegram::Telegram;

/// Capacity of the in-process telegram broadcast channel.
const BUS_CAPACITY: usize = 1024;

/// Logical role of a gateway peer in the routing case.
///
/// Public KNX/IP standard (KNX 03_08_03 §2) distinguishes between
/// `KNXnet/IP Server` and `KNXnet/IP Client`; for the routing surface we
/// don't need a server-vs-client distinction since routing is symmetric
/// multicast, so the runtime simply tracks our own identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayRole {
    /// We listen on the multicast group and re-emit our own outgoing
    /// telegrams onto it. The recommended residential default.
    Router,
    /// We open a unicast tunnel to a remote KNX/IP server (e.g. a
    /// commercial IP-router product). Phase 2.
    TunnelClient,
}

/// Connection state for a tunnelling endpoint.
#[derive(Debug, Default)]
struct TunnellingState {
    /// Communication channel id assigned by the server (0 = not connected).
    channel_id: AtomicU8,
    /// Outgoing sequence counter — increments on every `TunnellingRequest`.
    out_seq: AtomicU8,
    /// Highest incoming sequence counter we've ACKed.
    in_seq: AtomicU8,
}

impl TunnellingState {
    fn next_out_seq(&self) -> u8 {
        // wrapping increment per KNX 03_08_03 §5.4.
        let prev = self.out_seq.fetch_add(1, Ordering::Relaxed);
        prev.wrapping_add(0)
    }

    fn set_channel(&self, id: u8) {
        self.channel_id.store(id, Ordering::Relaxed);
    }

    fn channel(&self) -> u8 {
        self.channel_id.load(Ordering::Relaxed)
    }
}

/// Gateway runtime configuration.
#[derive(Debug, Clone, Copy)]
pub struct GatewayConfig {
    pub role: GatewayRole,
    /// Local IP we expose in HPAIs we send. `0.0.0.0` selects route-back
    /// (server replies on the UDP packet's source endpoint).
    pub local_endpoint: Hpai,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            role: GatewayRole::Router,
            local_endpoint: Hpai::new(
                core::net::Ipv4Addr::UNSPECIFIED,
                0,
                crate::knxip::HostProtocol::Ipv4Udp,
            ),
        }
    }
}

/// In-process KNX bus — the bridge between cave-home automation logic
/// (which produces / consumes `Telegram`s) and the KNX/IP wire.
///
/// Phase 1 deliberately ships *only* the encode/decode boundary. The
/// runtime UDP socket plumbing is in the cave-home-binary's IO crate.
/// This separation keeps the gateway daemon spec-pure and trivially
/// testable without sockets.
#[derive(Debug)]
pub struct Gateway {
    config: GatewayConfig,
    tunnelling: Mutex<TunnellingState>,
    bus_tx: Sender<Telegram>,
}

impl Gateway {
    #[must_use]
    pub fn new(config: GatewayConfig) -> Self {
        let (bus_tx, _) = channel(BUS_CAPACITY);
        Self {
            config,
            tunnelling: Mutex::new(TunnellingState::default()),
            bus_tx,
        }
    }

    /// Subscribe to incoming telegrams (decoded from routing/tunnelling
    /// frames as they arrive).
    pub fn subscribe(&self) -> Receiver<Telegram> {
        self.bus_tx.subscribe()
    }

    /// Number of currently-subscribed bus consumers.
    pub fn subscriber_count(&self) -> usize {
        self.bus_tx.receiver_count()
    }

    /// Process a raw KNX/IP frame received from the wire.
    ///
    /// Validates the 6-byte header, dispatches by service type, and (if
    /// the frame carries a cEMI telegram) emits the parsed `Telegram` to
    /// the in-process bus.
    pub fn ingest(&self, frame: &[u8]) -> Result<KnxIpServiceType> {
        let (header, header_len) = KnxIpHeader::from_knx(frame)?;
        if (header.total_length as usize) != frame.len() {
            return Err(KnxError::KnxIpParse(format!(
                "frame length mismatch: header={} actual={}",
                header.total_length,
                frame.len()
            )));
        }
        let body = &frame[header_len..];
        match header.service_type {
            KnxIpServiceType::RoutingIndication => {
                let ri = RoutingIndication::from_knx(body);
                if let Ok(t) = cemi_to_telegram(&ri.raw_cemi) {
                    // Ignore send errors — no subscribers is non-fatal.
                    let _ = self.bus_tx.send(t);
                }
                Ok(header.service_type)
            }
            KnxIpServiceType::TunnellingRequest => {
                let tr = TunnellingRequest::from_knx(body)?;
                // Server-side: stamp sequence counter, deliver to bus.
                self.tunnelling
                    .lock()
                    .in_seq
                    .store(tr.sequence_counter, Ordering::Relaxed);
                if let Ok(t) = cemi_to_telegram(&tr.raw_cemi) {
                    let _ = self.bus_tx.send(t);
                }
                Ok(header.service_type)
            }
            other => Ok(other),
        }
    }

    /// Build a KNX/IP `RoutingIndication` frame for an outgoing telegram.
    ///
    /// The caller is expected to broadcast the returned buffer on the
    /// KNX/IP multicast group (`224.0.23.12:3671`).
    pub fn build_routing_frame(&self, telegram: &Telegram) -> Result<Vec<u8>> {
        let cemi = crate::cemi::telegram_to_cemi(telegram)?;
        let ri = RoutingIndication { raw_cemi: cemi };
        Ok(build_frame(
            KnxIpServiceType::RoutingIndication,
            &ri.to_knx(),
        ))
    }

    /// Build a KNX/IP `TunnellingRequest` frame for an outgoing telegram
    /// on the currently-established tunnel.
    ///
    /// Sequence counter is bumped per-frame (8-bit wrapping per the
    /// public KNX 03_08_03 spec §5.4).
    pub fn build_tunnelling_frame(&self, telegram: &Telegram) -> Result<Vec<u8>> {
        let state = self.tunnelling.lock();
        let channel_id = state.channel();
        if channel_id == 0 {
            return Err(KnxError::KnxIpParse("tunnel not connected".into()));
        }
        let seq = state.next_out_seq();
        let cemi = crate::cemi::telegram_to_cemi(telegram)?;
        let tr = TunnellingRequest {
            communication_channel_id: channel_id,
            sequence_counter: seq,
            raw_cemi: cemi,
        };
        Ok(build_frame(
            KnxIpServiceType::TunnellingRequest,
            &tr.to_knx(),
        ))
    }

    /// Accept a parsed `ConnectResponse` and stamp the channel id into
    /// the local tunnelling state.
    pub fn accept_connect_response(&self, channel_id: u8, status: ErrorCode) -> Result<()> {
        if status != ErrorCode::NoError {
            return Err(KnxError::KnxIpParse(format!(
                "ConnectResponse status = {status:?}"
            )));
        }
        self.tunnelling.lock().set_channel(channel_id);
        Ok(())
    }

    /// Build the periodic `ConnectionStateRequest` (heartbeat).
    ///
    /// KNX 03_08_03 §5.4: server expects one within 60 s; we recommend
    /// callers tick this at 30 s intervals.
    pub fn build_heartbeat_frame(&self) -> Result<Vec<u8>> {
        let channel_id = self.tunnelling.lock().channel();
        if channel_id == 0 {
            return Err(KnxError::KnxIpParse("tunnel not connected".into()));
        }
        let csr = ConnectionStateRequest {
            communication_channel_id: channel_id,
            control_endpoint: self.config.local_endpoint,
        };
        Ok(build_frame(
            KnxIpServiceType::ConnectionStateRequest,
            &csr.to_knx(),
        ))
    }

    /// Build the `DisconnectRequest` for the currently-open tunnel.
    pub fn build_disconnect_frame(&self) -> Result<Vec<u8>> {
        let channel_id = self.tunnelling.lock().channel();
        if channel_id == 0 {
            return Err(KnxError::KnxIpParse("tunnel not connected".into()));
        }
        let dr = DisconnectRequest {
            communication_channel_id: channel_id,
            control_endpoint: self.config.local_endpoint,
        };
        Ok(build_frame(
            KnxIpServiceType::DisconnectRequest,
            &dr.to_knx(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::GroupAddress;
    use crate::knxip::HostProtocol;
    use crate::telegram::{Apci, TelegramDestination};

    fn telegram_switch_on() -> Telegram {
        Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("1/2/3").unwrap()),
            Some(Apci::GroupValueWrite(vec![0x01])),
        )
    }

    #[test]
    fn default_gateway_is_router() {
        let g = Gateway::new(GatewayConfig::default());
        assert_eq!(g.config.role, GatewayRole::Router);
    }

    #[test]
    fn build_routing_frame_has_correct_header() {
        let g = Gateway::new(GatewayConfig::default());
        let frame = g.build_routing_frame(&telegram_switch_on()).unwrap();
        let (h, _) = KnxIpHeader::from_knx(&frame).unwrap();
        assert_eq!(h.service_type, KnxIpServiceType::RoutingIndication);
        assert_eq!(h.total_length as usize, frame.len());
    }

    #[test]
    fn tunnelling_frame_requires_open_tunnel() {
        let g = Gateway::new(GatewayConfig::default());
        assert!(g.build_tunnelling_frame(&telegram_switch_on()).is_err());
    }

    #[test]
    fn tunnelling_state_machine_lifecycle() {
        let g = Gateway::new(GatewayConfig {
            role: GatewayRole::TunnelClient,
            local_endpoint: Hpai::new(
                core::net::Ipv4Addr::new(192, 168, 1, 100),
                3671,
                HostProtocol::Ipv4Udp,
            ),
        });
        // Heartbeat / disconnect fail before connect.
        assert!(g.build_heartbeat_frame().is_err());
        assert!(g.build_disconnect_frame().is_err());

        // Accept a synthetic ConnectResponse on channel 7.
        g.accept_connect_response(7, ErrorCode::NoError).unwrap();

        // Now we can build heartbeat + disconnect frames.
        let hb = g.build_heartbeat_frame().unwrap();
        let dc = g.build_disconnect_frame().unwrap();
        let (hb_h, _) = KnxIpHeader::from_knx(&hb).unwrap();
        let (dc_h, _) = KnxIpHeader::from_knx(&dc).unwrap();
        assert_eq!(hb_h.service_type, KnxIpServiceType::ConnectionStateRequest);
        assert_eq!(dc_h.service_type, KnxIpServiceType::DisconnectRequest);

        // Tunnelling frame increments sequence counter.
        let f1 = g.build_tunnelling_frame(&telegram_switch_on()).unwrap();
        let f2 = g.build_tunnelling_frame(&telegram_switch_on()).unwrap();
        // Body starts at offset 6 (header) + first byte is HEADER_LENGTH(4),
        // then channel(7), then seq.
        assert_eq!(f1[6 + 2], 0); // first seq
        assert_eq!(f2[6 + 2], 1); // second seq
    }

    #[test]
    fn accept_connect_response_rejects_errors() {
        let g = Gateway::new(GatewayConfig::default());
        assert!(g
            .accept_connect_response(7, ErrorCode::NoMoreConnections)
            .is_err());
    }

    #[test]
    fn ingest_routing_frame_emits_to_bus() {
        let g = Gateway::new(GatewayConfig::default());
        let mut rx = g.subscribe();
        let frame = g.build_routing_frame(&telegram_switch_on()).unwrap();
        assert_eq!(
            g.ingest(&frame).unwrap(),
            KnxIpServiceType::RoutingIndication
        );
        let t = rx.try_recv().expect("bus should have a telegram");
        match t.destination_address {
            TelegramDestination::Group(g) => assert_eq!(g.raw(), 0x0A03),
            other => panic!("wrong destination: {other:?}"),
        }
    }

    #[test]
    fn ingest_rejects_length_mismatch() {
        let g = Gateway::new(GatewayConfig::default());
        // Header claims 10 bytes, give 8.
        let bad = [0x06_u8, 0x10, 0x05, 0x30, 0x00, 0x0A, 0x00, 0x00];
        assert!(g.ingest(&bad).is_err());
    }
}
