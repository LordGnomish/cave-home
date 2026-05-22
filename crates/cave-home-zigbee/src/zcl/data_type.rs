// SPDX-License-Identifier: Apache-2.0
// CLEAN-ROOM: Zigbee 3.0 spec + zigbee-herdsman API doc reference only; Z2M source NOT consulted
//! ZCL data-type encoding — ZCL §2.5.
//!
//! Phase 1 supports the subset of data types that actually appear in
//! the Foundation Read/Write/Report payloads and in the Groups / Scenes
//! / OTA clusters: Boolean (0x10), unsigned 8/16/32 (0x20/0x21/0x23),
//! signed 8/16 (0x28/0x29), enum8 (0x30), and character-string (0x42).
//!
//! Additional types can be added in Phase 1b without breaking the
//! [`AttributeValue`] enum (we'd add new variants alongside the
//! existing ones).

use crate::error::{Result, ZigbeeError};

/// ZCL data-type identifier — ZCL §2.5.2 Table 2-10.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ZclDataType {
    /// 8-bit Boolean — ZCL §2.5.2.1.
    Boolean = 0x10,
    /// 8-bit unsigned integer.
    Uint8 = 0x20,
    /// 16-bit unsigned integer (little-endian on the wire).
    Uint16 = 0x21,
    /// 32-bit unsigned integer (little-endian on the wire).
    Uint32 = 0x23,
    /// 8-bit signed integer.
    Int8 = 0x28,
    /// 16-bit signed integer.
    Int16 = 0x29,
    /// 8-bit enumeration.
    Enum8 = 0x30,
    /// Character string — `u8` length prefix + UTF-8 payload.
    CharString = 0x42,
}

impl ZclDataType {
    /// Decode the data-type byte.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Zcl`] for an unknown type byte.
    pub fn from_byte(b: u8) -> Result<Self> {
        Ok(match b {
            0x10 => Self::Boolean,
            0x20 => Self::Uint8,
            0x21 => Self::Uint16,
            0x23 => Self::Uint32,
            0x28 => Self::Int8,
            0x29 => Self::Int16,
            0x30 => Self::Enum8,
            0x42 => Self::CharString,
            other => return Err(ZigbeeError::Zcl(format!("unknown data-type 0x{other:02x}"))),
        })
    }

    /// Encoded length, when fixed.
    #[must_use]
    pub const fn fixed_size(self) -> Option<usize> {
        Some(match self {
            Self::Boolean | Self::Uint8 | Self::Int8 | Self::Enum8 => 1,
            Self::Uint16 | Self::Int16 => 2,
            Self::Uint32 => 4,
            Self::CharString => return None,
        })
    }
}

/// Decoded ZCL attribute value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeValue {
    /// Boolean — `0` ⇒ false, `1` ⇒ true, `0xff` ⇒ invalid.
    Boolean(bool),
    /// Unsigned 8.
    Uint8(u8),
    /// Unsigned 16.
    Uint16(u16),
    /// Unsigned 32.
    Uint32(u32),
    /// Signed 8.
    Int8(i8),
    /// Signed 16.
    Int16(i16),
    /// 8-bit enumeration.
    Enum8(u8),
    /// Character string.
    CharString(String),
}

impl AttributeValue {
    /// ZCL data type tag for this value.
    #[must_use]
    pub const fn data_type(&self) -> ZclDataType {
        match self {
            Self::Boolean(_) => ZclDataType::Boolean,
            Self::Uint8(_) => ZclDataType::Uint8,
            Self::Uint16(_) => ZclDataType::Uint16,
            Self::Uint32(_) => ZclDataType::Uint32,
            Self::Int8(_) => ZclDataType::Int8,
            Self::Int16(_) => ZclDataType::Int16,
            Self::Enum8(_) => ZclDataType::Enum8,
            Self::CharString(_) => ZclDataType::CharString,
        }
    }

    /// Encode the value (data-type byte NOT included — callers prepend it).
    #[must_use]
    pub fn encode_value(&self) -> Vec<u8> {
        match self {
            Self::Boolean(b) => vec![u8::from(*b)],
            Self::Uint8(v) => vec![*v],
            Self::Uint16(v) => v.to_le_bytes().to_vec(),
            Self::Uint32(v) => v.to_le_bytes().to_vec(),
            Self::Int8(v) => vec![*v as u8],
            Self::Int16(v) => v.to_le_bytes().to_vec(),
            Self::Enum8(v) => vec![*v],
            Self::CharString(s) => {
                let bytes = s.as_bytes();
                let len = u8::try_from(bytes.len()).unwrap_or(u8::MAX);
                let mut out = Vec::with_capacity(1 + bytes.len());
                out.push(len);
                out.extend_from_slice(&bytes[..usize::from(len)]);
                out
            }
        }
    }

    /// Decode a value of `dt` from `bytes`. Returns the value and the
    /// number of bytes consumed.
    ///
    /// # Errors
    /// Returns [`ZigbeeError::Truncated`] if `bytes` is too short.
    pub fn decode(dt: ZclDataType, bytes: &[u8]) -> Result<(Self, usize)> {
        if let Some(sz) = dt.fixed_size() {
            if bytes.len() < sz {
                return Err(ZigbeeError::Truncated {
                    need: sz,
                    have: bytes.len(),
                });
            }
        }
        Ok(match dt {
            ZclDataType::Boolean => (Self::Boolean(bytes[0] != 0), 1),
            ZclDataType::Uint8 => (Self::Uint8(bytes[0]), 1),
            ZclDataType::Uint16 => (
                Self::Uint16(u16::from_le_bytes([bytes[0], bytes[1]])),
                2,
            ),
            ZclDataType::Uint32 => (
                Self::Uint32(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
                4,
            ),
            ZclDataType::Int8 => (Self::Int8(bytes[0] as i8), 1),
            ZclDataType::Int16 => (
                Self::Int16(i16::from_le_bytes([bytes[0], bytes[1]])),
                2,
            ),
            ZclDataType::Enum8 => (Self::Enum8(bytes[0]), 1),
            ZclDataType::CharString => {
                if bytes.is_empty() {
                    return Err(ZigbeeError::Truncated { need: 1, have: 0 });
                }
                let len = usize::from(bytes[0]);
                if bytes.len() < 1 + len {
                    return Err(ZigbeeError::Truncated {
                        need: 1 + len,
                        have: bytes.len(),
                    });
                }
                let s = std::str::from_utf8(&bytes[1..1 + len])
                    .map_err(|e| ZigbeeError::Zcl(format!("string not utf8: {e}")))?
                    .to_owned();
                (Self::CharString(s), 1 + len)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_round_trip() {
        for b in [true, false] {
            let v = AttributeValue::Boolean(b);
            let bytes = v.encode_value();
            let (decoded, used) = AttributeValue::decode(ZclDataType::Boolean, &bytes).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(used, 1);
        }
    }

    #[test]
    fn uint16_le_round_trip() {
        let v = AttributeValue::Uint16(0xbeef);
        let bytes = v.encode_value();
        assert_eq!(bytes, vec![0xef, 0xbe]);
        let (decoded, used) = AttributeValue::decode(ZclDataType::Uint16, &bytes).unwrap();
        assert_eq!(decoded, v);
        assert_eq!(used, 2);
    }

    #[test]
    fn int16_negative_round_trip() {
        let v = AttributeValue::Int16(-42);
        let bytes = v.encode_value();
        let (decoded, used) = AttributeValue::decode(ZclDataType::Int16, &bytes).unwrap();
        assert_eq!(decoded, v);
        assert_eq!(used, 2);
    }

    #[test]
    fn char_string_round_trip() {
        let v = AttributeValue::CharString("hello".into());
        let bytes = v.encode_value();
        assert_eq!(bytes, b"\x05hello");
        let (decoded, used) = AttributeValue::decode(ZclDataType::CharString, &bytes).unwrap();
        assert_eq!(decoded, v);
        assert_eq!(used, 6);
    }

    #[test]
    fn truncated_uint32_rejected() {
        let err = AttributeValue::decode(ZclDataType::Uint32, &[0x01, 0x02]).unwrap_err();
        assert!(matches!(err, ZigbeeError::Truncated { .. }));
    }

    #[test]
    fn unknown_data_type_byte_rejected() {
        let err = ZclDataType::from_byte(0xab).unwrap_err();
        assert!(matches!(err, ZigbeeError::Zcl(_)));
    }
}
