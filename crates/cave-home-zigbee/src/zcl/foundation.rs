// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! ZCL Foundation profile-wide commands — ZCL §2.4.
//!
//! Phase 1 implements:
//! - Read Attributes (0x00) — §2.4.1
//! - Read Attributes Response (0x01) — §2.4.2
//! - Write Attributes (0x02) — §2.4.3
//! - Write Attributes Response (0x04) — §2.4.5
//! - Configure Reporting (0x06) — §2.4.7
//! - Report Attributes (0x0a) — §2.4.11

use super::data_type::{AttributeValue, ZclDataType};
use crate::error::{Result, ZigbeeError};

/// Foundation command identifiers — ZCL §2.4 (Table 2-3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FoundationCommandId {
    /// 0x00 — Read Attributes.
    ReadAttributes = 0x00,
    /// 0x01 — Read Attributes Response.
    ReadAttributesResponse = 0x01,
    /// 0x02 — Write Attributes.
    WriteAttributes = 0x02,
    /// 0x04 — Write Attributes Response.
    WriteAttributesResponse = 0x04,
    /// 0x06 — Configure Reporting.
    ConfigureReporting = 0x06,
    /// 0x0a — Report Attributes.
    ReportAttributes = 0x0a,
}

/// ZCL §2.4.1 Read Attributes payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadAttributes {
    /// Attribute IDs the client wants.
    pub ids: Vec<u16>,
}

impl ReadAttributes {
    /// Encode as the ZCL frame payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.ids.len() * 2);
        for id in &self.ids {
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }

    /// Decode the ZCL frame payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if the payload is not 2-byte aligned.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        if payload.len() % 2 != 0 {
            return Err(ZigbeeError::Zcl(
                "Read Attributes payload not 2-byte aligned".into(),
            ));
        }
        let ids = payload
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(Self { ids })
    }
}

/// One record inside a Read Attributes Response — ZCL §2.4.2.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeRecord {
    /// Attribute ID.
    pub id: u16,
    /// 0x00 ⇒ success (value follows); non-zero ⇒ ZCL status, no value.
    pub status: u8,
    /// Value (only meaningful when `status == 0`).
    pub value: Option<AttributeValue>,
}

/// ZCL §2.4.2 Read Attributes Response payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadAttributesResponse {
    /// Per-attribute records.
    pub records: Vec<AttributeRecord>,
}

impl ReadAttributesResponse {
    /// Encode as the ZCL frame payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for r in &self.records {
            out.extend_from_slice(&r.id.to_le_bytes());
            out.push(r.status);
            if r.status == 0x00 {
                if let Some(v) = &r.value {
                    out.push(v.data_type() as u8);
                    out.extend_from_slice(&v.encode_value());
                }
            }
        }
        out
    }

    /// Decode the ZCL frame payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] or [`ZigbeeError::Zcl`].
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut i = 0;
        let mut records = Vec::new();
        while i < payload.len() {
            if payload.len() < i + 3 {
                return Err(ZigbeeError::Truncated {
                    need: i + 3,
                    have: payload.len(),
                });
            }
            let id = u16::from_le_bytes([payload[i], payload[i + 1]]);
            let status = payload[i + 2];
            i += 3;
            let value = if status == 0x00 {
                if i >= payload.len() {
                    return Err(ZigbeeError::Truncated {
                        need: i + 1,
                        have: payload.len(),
                    });
                }
                let dt = ZclDataType::from_byte(payload[i])?;
                i += 1;
                let (v, used) = AttributeValue::decode(dt, &payload[i..])?;
                i += used;
                Some(v)
            } else {
                None
            };
            records.push(AttributeRecord { id, status, value });
        }
        Ok(Self { records })
    }
}

/// ZCL §2.4.3 Write Attributes payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteAttributes {
    /// (Attribute ID, value) tuples.
    pub writes: Vec<(u16, AttributeValue)>,
}

impl WriteAttributes {
    /// Encode as the ZCL frame payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, v) in &self.writes {
            out.extend_from_slice(&id.to_le_bytes());
            out.push(v.data_type() as u8);
            out.extend_from_slice(&v.encode_value());
        }
        out
    }

    /// Decode the ZCL frame payload.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] or [`ZigbeeError::Zcl`].
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut i = 0;
        let mut writes = Vec::new();
        while i < payload.len() {
            if payload.len() < i + 3 {
                return Err(ZigbeeError::Truncated {
                    need: i + 3,
                    have: payload.len(),
                });
            }
            let id = u16::from_le_bytes([payload[i], payload[i + 1]]);
            let dt = ZclDataType::from_byte(payload[i + 2])?;
            i += 3;
            let (v, used) = AttributeValue::decode(dt, &payload[i..])?;
            i += used;
            writes.push((id, v));
        }
        Ok(Self { writes })
    }
}

/// Reporting direction — ZCL §2.4.7.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReportingDirection {
    /// 0x00 — the device reports to its bound peer.
    Reported = 0x00,
    /// 0x01 — the device expects to receive reports for the attribute.
    Received = 0x01,
}

/// One row of a Configure Reporting payload — ZCL §2.4.7.1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigureReporting {
    /// Direction byte.
    pub direction: ReportingDirection,
    /// Attribute id.
    pub attribute_id: u16,
    /// Attribute data type (only for `Reported`).
    pub data_type: ZclDataType,
    /// Minimum reporting interval (seconds).
    pub min_interval: u16,
    /// Maximum reporting interval (seconds).
    pub max_interval: u16,
}

impl ConfigureReporting {
    /// Encode this single record.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        out.push(self.direction as u8);
        out.extend_from_slice(&self.attribute_id.to_le_bytes());
        out.push(self.data_type as u8);
        out.extend_from_slice(&self.min_interval.to_le_bytes());
        out.extend_from_slice(&self.max_interval.to_le_bytes());
        out
    }

    /// Decode a single record from the head of `payload`.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if `payload` is too short.
    pub fn decode(payload: &[u8]) -> Result<(Self, usize)> {
        if payload.len() < 8 {
            return Err(ZigbeeError::Truncated {
                need: 8,
                have: payload.len(),
            });
        }
        let direction = match payload[0] {
            0x00 => ReportingDirection::Reported,
            0x01 => ReportingDirection::Received,
            other => {
                return Err(ZigbeeError::Zcl(format!(
                    "reserved reporting direction 0x{other:02x}"
                )));
            }
        };
        let attribute_id = u16::from_le_bytes([payload[1], payload[2]]);
        let data_type = ZclDataType::from_byte(payload[3])?;
        let min_interval = u16::from_le_bytes([payload[4], payload[5]]);
        let max_interval = u16::from_le_bytes([payload[6], payload[7]]);
        Ok((
            Self {
                direction,
                attribute_id,
                data_type,
                min_interval,
                max_interval,
            },
            8,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_attributes_round_trip() {
        let r = ReadAttributes {
            ids: vec![0x0000, 0x0004, 0x0005],
        };
        let bytes = r.encode();
        assert_eq!(bytes, vec![0x00, 0x00, 0x04, 0x00, 0x05, 0x00]);
        let decoded = ReadAttributes::decode(&bytes).unwrap();
        assert_eq!(decoded, r);
    }

    #[test]
    fn read_attributes_decode_rejects_odd_length() {
        assert!(matches!(
            ReadAttributes::decode(&[0x01]),
            Err(ZigbeeError::Zcl(_))
        ));
    }

    #[test]
    fn read_attributes_response_success_record_round_trip() {
        let resp = ReadAttributesResponse {
            records: vec![
                AttributeRecord {
                    id: 0x0000,
                    status: 0x00,
                    value: Some(AttributeValue::Uint8(7)),
                },
                AttributeRecord {
                    id: 0x0001,
                    status: 0x86, // UNSUPPORTED_ATTRIBUTE
                    value: None,
                },
            ],
        };
        let bytes = resp.encode();
        let decoded = ReadAttributesResponse::decode(&bytes).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn write_attributes_round_trip() {
        let w = WriteAttributes {
            writes: vec![
                (0x0000, AttributeValue::Boolean(true)),
                (0x0010, AttributeValue::Uint16(0xabcd)),
                (0x0020, AttributeValue::CharString("ok".into())),
            ],
        };
        let bytes = w.encode();
        let decoded = WriteAttributes::decode(&bytes).unwrap();
        assert_eq!(decoded, w);
    }

    #[test]
    fn configure_reporting_round_trip() {
        let c = ConfigureReporting {
            direction: ReportingDirection::Reported,
            attribute_id: 0x0021,
            data_type: ZclDataType::Uint8,
            min_interval: 1,
            max_interval: 60,
        };
        let bytes = c.encode();
        assert_eq!(bytes.len(), 8);
        let (decoded, used) = ConfigureReporting::decode(&bytes).unwrap();
        assert_eq!(decoded, c);
        assert_eq!(used, 8);
    }
}
