// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/knxip_enum.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/error_code.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/header.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/hpai.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/connect_request.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/connect_response.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/connectionstate_request.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/connectionstate_response.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/disconnect_request.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/disconnect_response.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/tunnelling_request.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/tunnelling_ack.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/knxip/routing_indication.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! KNX/IP frame serialization/deserialization.
//!
//! A KNX/IP frame is 6-byte header + body. The 6-byte header carries:
//!   * `0x06` header length
//!   * `0x10` protocol version (only valid value at this time)
//!   * 2-byte big-endian service type identifier
//!   * 2-byte big-endian total frame length (header + body)

use core::net::Ipv4Addr;

use crate::address::IndividualAddress;
use crate::error::{KnxError, Result};

// ---------- enums ----------

/// KNX/IP service-type codes (subset relevant to Phase 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum KnxIpServiceType {
    // 0x02 Core services
    SearchRequest = 0x0201,
    SearchResponse = 0x0202,
    DescriptionRequest = 0x0203,
    DescriptionResponse = 0x0204,
    ConnectRequest = 0x0205,
    ConnectResponse = 0x0206,
    ConnectionStateRequest = 0x0207,
    ConnectionStateResponse = 0x0208,
    DisconnectRequest = 0x0209,
    DisconnectResponse = 0x020A,
    // 0x04 Tunnelling services
    TunnellingRequest = 0x0420,
    TunnellingAck = 0x0421,
    // 0x05 Routing services
    RoutingIndication = 0x0530,
    RoutingLostMessage = 0x0531,
    RoutingBusy = 0x0532,
}

impl KnxIpServiceType {
    pub fn from_u16(value: u16) -> Result<Self> {
        Ok(match value {
            0x0201 => Self::SearchRequest,
            0x0202 => Self::SearchResponse,
            0x0203 => Self::DescriptionRequest,
            0x0204 => Self::DescriptionResponse,
            0x0205 => Self::ConnectRequest,
            0x0206 => Self::ConnectResponse,
            0x0207 => Self::ConnectionStateRequest,
            0x0208 => Self::ConnectionStateResponse,
            0x0209 => Self::DisconnectRequest,
            0x020A => Self::DisconnectResponse,
            0x0420 => Self::TunnellingRequest,
            0x0421 => Self::TunnellingAck,
            0x0530 => Self::RoutingIndication,
            0x0531 => Self::RoutingLostMessage,
            0x0532 => Self::RoutingBusy,
            other => {
                return Err(KnxError::KnxIpParse(format!(
                    "KnxIpServiceType unknown: 0x{other:04x}"
                )));
            }
        })
    }

    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }
}

/// KNX/IP error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ErrorCode {
    NoError = 0x00,
    HostProtocolType = 0x01,
    VersionNotSupported = 0x02,
    SequenceNumber = 0x04,
    GenericError = 0x0F,
    ConnectionId = 0x21,
    ConnectionType = 0x22,
    ConnectionOption = 0x23,
    NoMoreConnections = 0x24,
    NoMoreUniqueConnections = 0x25,
    DataConnection = 0x26,
    KnxConnection = 0x27,
    AuthorisationError = 0x28,
    TunnellingLayer = 0x29,
    NoTunnellingAddress = 0x2D,
    ConnectionInUse = 0x2E,
}

impl ErrorCode {
    pub fn from_u8(value: u8) -> Result<Self> {
        Ok(match value {
            0x00 => Self::NoError,
            0x01 => Self::HostProtocolType,
            0x02 => Self::VersionNotSupported,
            0x04 => Self::SequenceNumber,
            0x0F => Self::GenericError,
            0x21 => Self::ConnectionId,
            0x22 => Self::ConnectionType,
            0x23 => Self::ConnectionOption,
            0x24 => Self::NoMoreConnections,
            0x25 => Self::NoMoreUniqueConnections,
            0x26 => Self::DataConnection,
            0x27 => Self::KnxConnection,
            0x28 => Self::AuthorisationError,
            0x29 => Self::TunnellingLayer,
            0x2D => Self::NoTunnellingAddress,
            0x2E => Self::ConnectionInUse,
            other => {
                return Err(KnxError::KnxIpParse(format!(
                    "ErrorCode unknown: 0x{other:02x}"
                )));
            }
        })
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// `ConnectRequestType` — connection types defined by the KNX/IP standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ConnectRequestType {
    DeviceMgmtConnection = 0x03,
    TunnelConnection = 0x04,
    RemLogConnection = 0x06,
    RemConfConnection = 0x07,
    ObjSvrConnection = 0x08,
}

/// `TunnellingLayer` — tunnelling layer requested in a CRI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TunnellingLayer {
    DataLinkLayer = 0x02,
    RawLayer = 0x04,
    BusmonitorLayer = 0x80,
}

/// Host protocol used in an HPAI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HostProtocol {
    Ipv4Udp = 0x01,
    Ipv4Tcp = 0x02,
}

impl HostProtocol {
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0x01 => Ok(Self::Ipv4Udp),
            0x02 => Ok(Self::Ipv4Tcp),
            other => Err(KnxError::KnxIpParse(format!(
                "unsupported host protocol code: 0x{other:02x}"
            ))),
        }
    }
}

// ---------- KNX/IP header ----------

/// 6-byte KNX/IP frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnxIpHeader {
    pub service_type: KnxIpServiceType,
    pub total_length: u16,
}

impl KnxIpHeader {
    pub const LENGTH: u8 = 0x06;
    pub const PROTOCOL_VERSION: u8 = 0x10;

    /// Serialize the header to 6 bytes.
    #[must_use]
    pub fn to_knx(self) -> [u8; 6] {
        let st = self.service_type.as_u16().to_be_bytes();
        let tl = self.total_length.to_be_bytes();
        [
            Self::LENGTH,
            Self::PROTOCOL_VERSION,
            st[0],
            st[1],
            tl[0],
            tl[1],
        ]
    }

    /// Parse a 6+-byte buffer; returns parsed header + number of bytes consumed.
    pub fn from_knx(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < Self::LENGTH as usize {
            return Err(KnxError::IncompleteFrame(
                "wrong connection header length".into(),
            ));
        }
        if data[0] != Self::LENGTH {
            return Err(KnxError::KnxIpParse(
                "wrong connection header length".into(),
            ));
        }
        let total_length = u16::from_be_bytes([data[4], data[5]]);
        if data[1] != Self::PROTOCOLVERSION_CONST() {
            return Err(KnxError::KnxIpParse("wrong protocol version".into()));
        }
        let service_type =
            KnxIpServiceType::from_u16(u16::from_be_bytes([data[2], data[3]]))?;
        Ok((
            Self {
                service_type,
                total_length,
            },
            Self::LENGTH as usize,
        ))
    }

    #[allow(non_snake_case)]
    const fn PROTOCOLVERSION_CONST() -> u8 {
        Self::PROTOCOL_VERSION
    }
}

// ---------- HPAI (Host Protocol Address Information) ----------

/// HPAI — 8-byte block carrying IP + port + host protocol identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hpai {
    pub ip_addr: Ipv4Addr,
    pub port: u16,
    pub protocol: HostProtocol,
}

impl Hpai {
    pub const LENGTH: u8 = 0x08;

    #[must_use]
    pub const fn new(ip_addr: Ipv4Addr, port: u16, protocol: HostProtocol) -> Self {
        Self {
            ip_addr,
            port,
            protocol,
        }
    }

    /// `route_back` flag (0.0.0.0 sentinel per KNX/IP spec).
    #[must_use]
    pub fn is_route_back(&self) -> bool {
        self.ip_addr == Ipv4Addr::UNSPECIFIED
    }

    pub fn to_knx(self) -> [u8; 8] {
        let ip = self.ip_addr.octets();
        let p = self.port.to_be_bytes();
        [
            Self::LENGTH,
            self.protocol as u8,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            p[0],
            p[1],
        ]
    }

    pub fn from_knx(raw: &[u8]) -> Result<(Self, usize)> {
        if raw.len() < Self::LENGTH as usize {
            return Err(KnxError::KnxIpParse("wrong HPAI length".into()));
        }
        if raw[0] != Self::LENGTH {
            return Err(KnxError::KnxIpParse("wrong HPAI length".into()));
        }
        let protocol = HostProtocol::from_u8(raw[1])?;
        let ip_addr = Ipv4Addr::new(raw[2], raw[3], raw[4], raw[5]);
        let port = u16::from_be_bytes([raw[6], raw[7]]);
        Ok((
            Self {
                ip_addr,
                port,
                protocol,
            },
            Self::LENGTH as usize,
        ))
    }
}

// ---------- ConnectRequest / ConnectResponse (Tunnelling subset) ----------

/// CRI — Connect Request Information (4 bytes for the Tunnel-Connection case).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectRequestInformation {
    pub connection_type: ConnectRequestType,
    pub knx_layer: TunnellingLayer,
}

impl ConnectRequestInformation {
    pub const CRI_TUNNEL_LENGTH: u8 = 4;

    #[must_use]
    pub const fn tunnel(knx_layer: TunnellingLayer) -> Self {
        Self {
            connection_type: ConnectRequestType::TunnelConnection,
            knx_layer,
        }
    }

    pub fn to_knx(self) -> Vec<u8> {
        vec![
            Self::CRI_TUNNEL_LENGTH,
            self.connection_type as u8,
            self.knx_layer as u8,
            0x00, // reserved
        ]
    }
}

/// KNX/IP `ConnectRequest` body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectRequest {
    pub control_endpoint: Hpai,
    pub data_endpoint: Hpai,
    pub cri: ConnectRequestInformation,
}

impl ConnectRequest {
    /// Total length: 2 × HPAI (8 each) + CRI (4) = 20 bytes.
    pub fn calculated_length(&self) -> u16 {
        u16::from(Hpai::LENGTH) * 2 + u16::from(ConnectRequestInformation::CRI_TUNNEL_LENGTH)
    }

    pub fn to_knx(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.calculated_length() as usize);
        out.extend_from_slice(&self.control_endpoint.to_knx());
        out.extend_from_slice(&self.data_endpoint.to_knx());
        out.extend_from_slice(&self.cri.to_knx());
        out
    }
}

/// KNX/IP `ConnectResponse` body (Tunnel-Connection variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectResponse {
    pub communication_channel: u8,
    pub status: ErrorCode,
    pub data_endpoint: Hpai,
    pub assigned_address: IndividualAddress,
}

impl ConnectResponse {
    pub fn from_knx(raw: &[u8]) -> Result<Self> {
        if raw.len() < 2 {
            return Err(KnxError::KnxIpParse("ConnectResponse too short".into()));
        }
        let communication_channel = raw[0];
        let status = ErrorCode::from_u8(raw[1])?;
        if status != ErrorCode::NoError {
            // success-only Phase 1 — keep response intact for diagnostics.
            return Ok(Self {
                communication_channel,
                status,
                data_endpoint: Hpai::new(Ipv4Addr::UNSPECIFIED, 0, HostProtocol::Ipv4Udp),
                assigned_address: IndividualAddress::from_raw(0),
            });
        }
        let (data_endpoint, used) = Hpai::from_knx(&raw[2..])?;
        let off = 2 + used;
        if raw.len() < off + 4 {
            return Err(KnxError::KnxIpParse("ConnectResponse CRD too short".into()));
        }
        let crd_length = raw[off];
        if crd_length != 4 {
            return Err(KnxError::KnxIpParse("CRD has wrong length".into()));
        }
        let assigned_address =
            IndividualAddress::from_knx([raw[off + 2], raw[off + 3]]);
        Ok(Self {
            communication_channel,
            status,
            data_endpoint,
            assigned_address,
        })
    }
}

// ---------- ConnectionState (heartbeat) ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionStateRequest {
    pub communication_channel_id: u8,
    pub control_endpoint: Hpai,
}

impl ConnectionStateRequest {
    pub fn calculated_length(&self) -> u16 {
        2 + u16::from(Hpai::LENGTH)
    }

    pub fn to_knx(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.calculated_length() as usize);
        out.push(self.communication_channel_id);
        out.push(0x00); // reserved
        out.extend_from_slice(&self.control_endpoint.to_knx());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionStateResponse {
    pub communication_channel_id: u8,
    pub status: ErrorCode,
}

impl ConnectionStateResponse {
    pub fn from_knx(raw: &[u8]) -> Result<Self> {
        if raw.len() < 2 {
            return Err(KnxError::KnxIpParse(
                "ConnectionStateResponse too short".into(),
            ));
        }
        Ok(Self {
            communication_channel_id: raw[0],
            status: ErrorCode::from_u8(raw[1])?,
        })
    }

    pub fn to_knx(&self) -> [u8; 2] {
        [self.communication_channel_id, self.status.as_u8()]
    }
}

// ---------- Disconnect ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectRequest {
    pub communication_channel_id: u8,
    pub control_endpoint: Hpai,
}

impl DisconnectRequest {
    pub fn calculated_length(&self) -> u16 {
        2 + u16::from(Hpai::LENGTH)
    }

    pub fn to_knx(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.calculated_length() as usize);
        out.push(self.communication_channel_id);
        out.push(0x00); // reserved
        out.extend_from_slice(&self.control_endpoint.to_knx());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectResponse {
    pub communication_channel_id: u8,
    pub status: ErrorCode,
}

impl DisconnectResponse {
    pub const LENGTH: u8 = 2;

    pub fn from_knx(raw: &[u8]) -> Result<Self> {
        if raw.len() < Self::LENGTH as usize {
            return Err(KnxError::KnxIpParse("DisconnectResponse too short".into()));
        }
        Ok(Self {
            communication_channel_id: raw[0],
            status: ErrorCode::from_u8(raw[1])?,
        })
    }

    pub fn to_knx(&self) -> [u8; 2] {
        [self.communication_channel_id, self.status.as_u8()]
    }
}

// ---------- Tunnelling ----------

/// `TunnellingRequest` body — 4-byte connection header + cEMI payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnellingRequest {
    pub communication_channel_id: u8,
    pub sequence_counter: u8,
    pub raw_cemi: Vec<u8>,
}

impl TunnellingRequest {
    pub const HEADER_LENGTH: u8 = 4;

    pub fn calculated_length(&self) -> u16 {
        u16::from(Self::HEADER_LENGTH) + (self.raw_cemi.len() as u16)
    }

    pub fn to_knx(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.calculated_length() as usize);
        out.push(Self::HEADER_LENGTH);
        out.push(self.communication_channel_id);
        out.push(self.sequence_counter);
        out.push(0x00); // reserved
        out.extend_from_slice(&self.raw_cemi);
        out
    }

    pub fn from_knx(raw: &[u8]) -> Result<Self> {
        if raw.len() < Self::HEADER_LENGTH as usize {
            return Err(KnxError::KnxIpParse("connection header wrong length".into()));
        }
        if raw[0] != Self::HEADER_LENGTH {
            return Err(KnxError::KnxIpParse("connection header wrong length".into()));
        }
        Ok(Self {
            communication_channel_id: raw[1],
            sequence_counter: raw[2],
            raw_cemi: raw[Self::HEADER_LENGTH as usize..].to_vec(),
        })
    }
}

/// `TunnellingAck` body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunnellingAck {
    pub communication_channel_id: u8,
    pub sequence_counter: u8,
    pub status: ErrorCode,
}

impl TunnellingAck {
    pub const BODY_LENGTH: u8 = 4;

    pub fn to_knx(&self) -> [u8; 4] {
        [
            Self::BODY_LENGTH,
            self.communication_channel_id,
            self.sequence_counter,
            self.status.as_u8(),
        ]
    }

    pub fn from_knx(raw: &[u8]) -> Result<Self> {
        if raw.len() != Self::BODY_LENGTH as usize {
            return Err(KnxError::KnxIpParse(
                "TunnellingAck body has wrong length".into(),
            ));
        }
        if raw[0] != Self::BODY_LENGTH {
            return Err(KnxError::KnxIpParse(
                "TunnellingAck body has invalid length".into(),
            ));
        }
        Ok(Self {
            communication_channel_id: raw[1],
            sequence_counter: raw[2],
            status: ErrorCode::from_u8(raw[3])?,
        })
    }
}

// ---------- Routing (multicast) ----------

/// `RoutingIndication` — KNX/IP multicast frame carrying a cEMI payload.
///
/// The canonical multicast endpoint is `224.0.23.12:3671` per the
/// KNX Association public spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingIndication {
    pub raw_cemi: Vec<u8>,
}

impl RoutingIndication {
    /// KNX/IP routing multicast group (KNX standard 03_08).
    pub const MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(224, 0, 23, 12);
    /// KNX/IP UDP port (registered with IANA).
    pub const PORT: u16 = 3671;

    pub fn calculated_length(&self) -> u16 {
        self.raw_cemi.len() as u16
    }

    pub fn to_knx(&self) -> Vec<u8> {
        self.raw_cemi.clone()
    }

    pub fn from_knx(raw: &[u8]) -> Self {
        Self {
            raw_cemi: raw.to_vec(),
        }
    }
}

// ---------- Frame helpers ----------

/// Build a full KNX/IP frame (header + body) for a service that ships its
/// own body bytes via the `body` slice.
#[must_use]
pub fn build_frame(service: KnxIpServiceType, body: &[u8]) -> Vec<u8> {
    let total = KnxIpHeader::LENGTH as usize + body.len();
    let header = KnxIpHeader {
        service_type: service,
        total_length: total as u16,
    };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&header.to_knx());
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = KnxIpHeader {
            service_type: KnxIpServiceType::TunnellingRequest,
            total_length: 0x14,
        };
        let bytes = h.to_knx();
        assert_eq!(bytes, [0x06, 0x10, 0x04, 0x20, 0x00, 0x14]);
        let (parsed, used) = KnxIpHeader::from_knx(&bytes).unwrap();
        assert_eq!(used, 6);
        assert_eq!(parsed, h);
    }

    #[test]
    fn header_rejects_wrong_version() {
        let bytes = [0x06_u8, 0x20, 0x05, 0x30, 0x00, 0x06];
        assert!(KnxIpHeader::from_knx(&bytes).is_err());
    }

    #[test]
    fn header_rejects_unknown_service() {
        let bytes = [0x06_u8, 0x10, 0xFF, 0xFF, 0x00, 0x06];
        assert!(KnxIpHeader::from_knx(&bytes).is_err());
    }

    #[test]
    fn hpai_roundtrip() {
        let h = Hpai::new(Ipv4Addr::new(192, 168, 1, 50), 3671, HostProtocol::Ipv4Udp);
        let bytes = h.to_knx();
        assert_eq!(
            bytes,
            [0x08, 0x01, 192, 168, 1, 50, 0x0E, 0x57]
        );
        let (parsed, used) = Hpai::from_knx(&bytes).unwrap();
        assert_eq!(used, 8);
        assert_eq!(parsed, h);
    }

    #[test]
    fn hpai_route_back_sentinel() {
        let h = Hpai::new(Ipv4Addr::UNSPECIFIED, 0, HostProtocol::Ipv4Udp);
        assert!(h.is_route_back());
    }

    #[test]
    fn connect_request_to_knx_layout() {
        let cr = ConnectRequest {
            control_endpoint: Hpai::new(
                Ipv4Addr::new(192, 168, 1, 100),
                3671,
                HostProtocol::Ipv4Udp,
            ),
            data_endpoint: Hpai::new(
                Ipv4Addr::new(192, 168, 1, 100),
                3672,
                HostProtocol::Ipv4Udp,
            ),
            cri: ConnectRequestInformation::tunnel(TunnellingLayer::DataLinkLayer),
        };
        let bytes = cr.to_knx();
        assert_eq!(bytes.len(), cr.calculated_length() as usize);
        // CRI bytes at offset 16: length=4, type=0x04, layer=0x02, reserved=0.
        assert_eq!(&bytes[16..20], &[0x04, 0x04, 0x02, 0x00]);
    }

    #[test]
    fn connect_response_roundtrip_success() {
        // Server assigns channel 7, IA 1.1.5.
        let mut raw = vec![
            0x07, 0x00, // channel + NoError
            0x08, 0x01, 192, 168, 1, 50, 0x0E, 0x57, // data HPAI
            0x04, 0x04, // CRD length + type
            0x11, 0x05, // IA 1.1.5
        ];
        let parsed = ConnectResponse::from_knx(&raw).unwrap();
        assert_eq!(parsed.communication_channel, 7);
        assert_eq!(parsed.status, ErrorCode::NoError);
        assert_eq!(parsed.assigned_address.to_string(), "1.1.5");
        // truncate to test failure path
        raw.truncate(1);
        assert!(ConnectResponse::from_knx(&raw).is_err());
    }

    #[test]
    fn connectionstate_request_to_knx() {
        let csr = ConnectionStateRequest {
            communication_channel_id: 7,
            control_endpoint: Hpai::new(
                Ipv4Addr::new(192, 168, 1, 100),
                3671,
                HostProtocol::Ipv4Udp,
            ),
        };
        let bytes = csr.to_knx();
        assert_eq!(bytes.len(), 10);
        assert_eq!(bytes[0], 7);
        assert_eq!(bytes[1], 0x00);
    }

    #[test]
    fn tunnelling_request_roundtrip() {
        let cemi = vec![0x11, 0x00, 0xBC, 0xE0, 0x11, 0x05, 0x0A, 0x01, 0x01, 0x00, 0x81];
        let tr = TunnellingRequest {
            communication_channel_id: 1,
            sequence_counter: 42,
            raw_cemi: cemi.clone(),
        };
        let bytes = tr.to_knx();
        assert_eq!(bytes[0], 4); // header length
        assert_eq!(bytes[1], 1);
        assert_eq!(bytes[2], 42);
        assert_eq!(bytes[3], 0x00); // reserved
        assert_eq!(&bytes[4..], cemi.as_slice());
        let parsed = TunnellingRequest::from_knx(&bytes).unwrap();
        assert_eq!(parsed, tr);
    }

    #[test]
    fn tunnelling_ack_roundtrip() {
        let ack = TunnellingAck {
            communication_channel_id: 7,
            sequence_counter: 42,
            status: ErrorCode::NoError,
        };
        let bytes = ack.to_knx();
        assert_eq!(bytes, [4, 7, 42, 0]);
        let parsed = TunnellingAck::from_knx(&bytes).unwrap();
        assert_eq!(parsed, ack);
    }

    #[test]
    fn routing_indication_endpoint_is_knx_multicast() {
        assert_eq!(
            RoutingIndication::MULTICAST_ADDRESS,
            Ipv4Addr::new(224, 0, 23, 12)
        );
        assert_eq!(RoutingIndication::PORT, 3671);
    }

    #[test]
    fn routing_indication_roundtrip() {
        let cemi = vec![0x29, 0x00, 0xBC, 0xD0, 0x11, 0x01, 0x09, 0x01, 0x01, 0x00, 0x81];
        let ri = RoutingIndication {
            raw_cemi: cemi.clone(),
        };
        assert_eq!(ri.to_knx(), cemi);
        assert_eq!(RoutingIndication::from_knx(&cemi), ri);
    }

    #[test]
    fn build_frame_wraps_body() {
        let body = TunnellingAck {
            communication_channel_id: 1,
            sequence_counter: 0,
            status: ErrorCode::NoError,
        };
        let frame = build_frame(KnxIpServiceType::TunnellingAck, &body.to_knx());
        assert_eq!(frame.len(), 10);
        // header is 6 bytes
        let (h, _) = KnxIpHeader::from_knx(&frame).unwrap();
        assert_eq!(h.service_type, KnxIpServiceType::TunnellingAck);
        assert_eq!(h.total_length, 10);
    }

    #[test]
    fn disconnect_response_roundtrip() {
        let dr = DisconnectResponse {
            communication_channel_id: 5,
            status: ErrorCode::NoError,
        };
        let bytes = dr.to_knx();
        assert_eq!(DisconnectResponse::from_knx(&bytes).unwrap(), dr);
    }
}
