// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/cemi/cemi_frame.py
// Source: XKNX/xknx@50fdf8af8e29b84b96de4487f5bd4f060f7c502c xknx/cemi/const.py
// Upstream license: MIT (preserved by attribution). Line-by-line port.
//
//! Common External Message Interface (cEMI) frame encode/decode.
//!
//! A cEMI frame is the container that carries a KNX telegram between the
//! network layer and the data link layer. KNX/IP `TunnellingRequest` and
//! `RoutingIndication` both ship a cEMI frame as their payload.
//!
//! cEMI message-code values are documented in the KNX Standard 03_06_03
//! (public). Frame layout used here matches the upstream xknx Python port.

use crate::address::{GroupAddress, IndividualAddress};
use crate::error::{KnxError, Result};
use crate::telegram::{Apci, Telegram, TelegramDestination, TelegramDirection};

/// cEMI message-code values (per KNX 03_06_03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CemiMessageCode {
    LDataReq = 0x11, // network → data-link
    LDataInd = 0x29, // data-link → network (telegram received)
    LDataCon = 0x2E, // data-link → network (local confirmation)
}

impl CemiMessageCode {
    pub fn from_u8(value: u8) -> Result<Self> {
        Ok(match value {
            0x11 => Self::LDataReq,
            0x29 => Self::LDataInd,
            0x2E => Self::LDataCon,
            other => {
                return Err(KnxError::UnsupportedCemi(format!(
                    "code 0x{other:02x}"
                )));
            }
        })
    }
}

/// cEMI control-field flags. We use the same constants as xknx upstream
/// (which in turn lift them from the public KNX standard).
pub mod flags {
    pub const FRAME_TYPE_STANDARD: u16 = 0x8000;
    pub const DO_NOT_REPEAT: u16 = 0x2000;
    pub const BROADCAST: u16 = 0x1000;
    pub const PRIORITY_LOW: u16 = 0x0C00;
    pub const NO_ACK_REQUESTED: u16 = 0x0000;
    pub const CONFIRM_NO_ERROR: u16 = 0x0000;
    pub const HOP_COUNT_1ST: u16 = 0x0060;
    pub const DESTINATION_GROUP_ADDRESS: u16 = 0x0080;

    /// Default control field for an outgoing group-addressed `L_Data_Req`.
    pub const DEFAULT_L_DATA_REQ_GROUP: u16 = FRAME_TYPE_STANDARD
        | DO_NOT_REPEAT
        | BROADCAST
        | PRIORITY_LOW
        | NO_ACK_REQUESTED
        | CONFIRM_NO_ERROR
        | HOP_COUNT_1ST
        | DESTINATION_GROUP_ADDRESS;
}

/// 4-bit APCI codes from KNX Application Layer table.
const APCI_GROUP_VALUE_READ: u16 = 0x000;
const APCI_GROUP_VALUE_RESPONSE: u16 = 0x040;
const APCI_GROUP_VALUE_WRITE: u16 = 0x080;
const APCI_MASK_OPCODE: u16 = 0x3C0; // top 4 bits of the 10-bit APCI

/// Encode a `Telegram` as a `L_Data_Req` cEMI frame.
///
/// Frame layout (no additional info):
/// ```text
///   [msg_code 1B][addInfo_len 1B = 0][ctrl1 1B][ctrl2 1B]
///   [src 2B][dst 2B][npdu_len 1B][tpdu 1+B]
/// ```
pub fn telegram_to_cemi(telegram: &Telegram) -> Result<Vec<u8>> {
    let TelegramDestination::Group(dst) = telegram.destination_address else {
        return Err(KnxError::UnsupportedCemi(
            "individual-address telegrams not in Phase 1 scope".into(),
        ));
    };
    let payload = telegram
        .payload
        .as_ref()
        .ok_or_else(|| KnxError::UnsupportedCemi("telegram has no APCI payload".into()))?;

    let flags_word = flags::DEFAULT_L_DATA_REQ_GROUP;
    let mut out = Vec::with_capacity(12);
    out.push(CemiMessageCode::LDataReq as u8);
    out.push(0x00); // additional info length
    out.push((flags_word >> 8) as u8);
    out.push((flags_word & 0xFF) as u8);
    out.extend_from_slice(&telegram.source_address.to_knx());
    out.extend_from_slice(&dst.to_knx());

    let (npdu_len, tpdu) = apci_to_bytes(payload)?;
    out.push(npdu_len);
    out.extend_from_slice(&tpdu);

    Ok(out)
}

/// Decode a `L_Data_Ind` (or `L_Data_Req`) cEMI frame into a `Telegram`.
pub fn cemi_to_telegram(raw: &[u8]) -> Result<Telegram> {
    if raw.len() < 11 {
        return Err(KnxError::CemiParse(format!(
            "frame too short: {} bytes",
            raw.len()
        )));
    }
    let msg_code = CemiMessageCode::from_u8(raw[0])?;
    let add_info_len = raw[1] as usize;
    let off = 2 + add_info_len;
    if raw.len() < off + 9 {
        return Err(KnxError::CemiParse("frame too short after addInfo".into()));
    }
    let _ctrl1 = raw[off];
    let ctrl2 = raw[off + 1];
    let src = IndividualAddress::from_knx([raw[off + 2], raw[off + 3]]);
    let dst_bytes = [raw[off + 4], raw[off + 5]];
    let npdu_len = raw[off + 6] as usize;
    if raw.len() < off + 7 + 1 + npdu_len {
        return Err(KnxError::CemiParse("NPDU length exceeds frame".into()));
    }
    let tpdu = &raw[off + 7..off + 7 + 1 + npdu_len];
    let apci = bytes_to_apci(tpdu, npdu_len)?;

    // Destination-address-type bit lives in ctrl2's high bit (KNX 03_06_03).
    // `flags::DESTINATION_GROUP_ADDRESS` is the low-byte mask = 0x80.
    let dst = if ctrl2 & (flags::DESTINATION_GROUP_ADDRESS as u8) != 0 {
        TelegramDestination::Group(GroupAddress::from_knx(dst_bytes))
    } else {
        TelegramDestination::Individual(IndividualAddress::from_knx(dst_bytes))
    };

    Ok(Telegram {
        destination_address: dst,
        direction: match msg_code {
            CemiMessageCode::LDataReq => TelegramDirection::Outgoing,
            CemiMessageCode::LDataInd | CemiMessageCode::LDataCon => {
                TelegramDirection::Incoming
            }
        },
        payload: Some(apci),
        source_address: src,
    })
}

/// Convert an APCI payload to `(npdu_len, tpdu_bytes)`.
///
/// TPCI (transport control information) is hard-coded as `T_DataGroup`
/// (`tpci = 0x00`) for group telegrams, per KNX 03_03_04.
fn apci_to_bytes(apci: &Apci) -> Result<(u8, Vec<u8>)> {
    match apci {
        Apci::GroupValueRead => {
            // APCI 0b0000_000000, npdu_len = 1
            Ok((1, vec![0x00, 0x00]))
        }
        Apci::GroupValueWrite(data) => {
            if data.len() == 1 && data[0] < 0x40 {
                // 6-bit small data lives in the low 6 bits of byte 2.
                let b0 = (APCI_GROUP_VALUE_WRITE >> 8) as u8;
                let b1 = (APCI_GROUP_VALUE_WRITE & 0xFF) as u8 | (data[0] & 0x3F);
                Ok((1, vec![b0, b1]))
            } else {
                let b0 = (APCI_GROUP_VALUE_WRITE >> 8) as u8;
                let b1 = (APCI_GROUP_VALUE_WRITE & 0xFF) as u8;
                let mut tpdu = vec![b0, b1];
                tpdu.extend_from_slice(data);
                Ok((1 + data.len() as u8, tpdu))
            }
        }
        Apci::GroupValueResponse(data) => {
            if data.len() == 1 && data[0] < 0x40 {
                let b0 = (APCI_GROUP_VALUE_RESPONSE >> 8) as u8;
                let b1 = (APCI_GROUP_VALUE_RESPONSE & 0xFF) as u8 | (data[0] & 0x3F);
                Ok((1, vec![b0, b1]))
            } else {
                let b0 = (APCI_GROUP_VALUE_RESPONSE >> 8) as u8;
                let b1 = (APCI_GROUP_VALUE_RESPONSE & 0xFF) as u8;
                let mut tpdu = vec![b0, b1];
                tpdu.extend_from_slice(data);
                Ok((1 + data.len() as u8, tpdu))
            }
        }
    }
}

fn bytes_to_apci(tpdu: &[u8], npdu_len: usize) -> Result<Apci> {
    if tpdu.len() < 2 {
        return Err(KnxError::CemiParse("TPDU shorter than APCI header".into()));
    }
    let apci_word = ((u16::from(tpdu[0])) << 8) | u16::from(tpdu[1]);
    let opcode = apci_word & APCI_MASK_OPCODE;
    match opcode {
        APCI_GROUP_VALUE_READ => Ok(Apci::GroupValueRead),
        APCI_GROUP_VALUE_WRITE => {
            if npdu_len == 1 {
                Ok(Apci::GroupValueWrite(vec![(tpdu[1] & 0x3F)]))
            } else {
                Ok(Apci::GroupValueWrite(tpdu[2..].to_vec()))
            }
        }
        APCI_GROUP_VALUE_RESPONSE => {
            if npdu_len == 1 {
                Ok(Apci::GroupValueResponse(vec![(tpdu[1] & 0x3F)]))
            } else {
                Ok(Apci::GroupValueResponse(tpdu[2..].to_vec()))
            }
        }
        other => Err(KnxError::CemiParse(format!(
            "unsupported APCI opcode 0x{other:03x}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::GroupAddress;

    #[test]
    fn group_value_write_small_roundtrip() {
        // Switch ON to GA 1/2/3
        let t = Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("1/2/3").unwrap()),
            Some(Apci::GroupValueWrite(vec![0x01])),
        );
        let cemi = telegram_to_cemi(&t).unwrap();
        // Frame: code(1) ai(1) ctrl1(1) ctrl2(1) src(2) dst(2) npdu_len(1) tpdu(2)
        assert_eq!(cemi.len(), 11);
        assert_eq!(cemi[0], 0x11); // L_Data_Req
        assert_eq!(cemi[1], 0x00); // no additional info
        assert_eq!(cemi[6..8], [0x0A, 0x03]); // GA 1/2/3 = (1<<11)|(2<<8)|3 = 0x0A03
        assert_eq!(cemi[8], 0x01); // npdu_len
        assert_eq!(cemi[9], 0x00); // TPCI/APCI high
        assert_eq!(cemi[10] & 0x3F, 0x01); // value
    }

    #[test]
    fn group_value_read_encodes_zero_data() {
        let t = Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("0/0/1").unwrap()),
            Some(Apci::GroupValueRead),
        );
        let cemi = telegram_to_cemi(&t).unwrap();
        assert_eq!(cemi[8], 0x01); // npdu_len
        assert_eq!(cemi[9..11], [0x00, 0x00]);
    }

    #[test]
    fn group_value_write_temperature_roundtrip() {
        // DPT 9.001 — 21.5 °C
        let v: f64 = 21.5;
        let payload = crate::dpt::dpt_9::to_knx(v).unwrap().to_vec();
        let t = Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("4/2/16").unwrap()),
            Some(Apci::GroupValueWrite(payload.clone())),
        );
        let cemi = telegram_to_cemi(&t).unwrap();
        let back = cemi_to_telegram(&cemi).unwrap();
        match back.payload.unwrap() {
            Apci::GroupValueWrite(data) => assert_eq!(data, payload),
            other => panic!("wrong APCI variant: {other:?}"),
        }
    }

    #[test]
    fn cemi_to_telegram_group_addr_is_set() {
        let t = Telegram::new(
            TelegramDestination::Group(GroupAddress::parse("1/2/3").unwrap()),
            Some(Apci::GroupValueWrite(vec![0x01])),
        );
        let cemi = telegram_to_cemi(&t).unwrap();
        let back = cemi_to_telegram(&cemi).unwrap();
        match back.destination_address {
            TelegramDestination::Group(g) => assert_eq!(g.raw(), 0x0A03),
            other => panic!("expected group destination, got {other:?}"),
        }
    }

    #[test]
    fn cemi_rejects_short_frame() {
        assert!(cemi_to_telegram(&[0x29, 0x00]).is_err());
    }
}
