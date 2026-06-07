//! MQTT 5.0 cursor-based wire primitives (§1.5 Data Representations).
//!
//! These read/write the four MQTT data types over a `&mut &[u8]` cursor
//! (decode) or a `BytesMut` (encode): Byte, Two/Four Byte Integer,
//! Variable Byte Integer (§1.5.5, shared with 3.1.1 via [`crate::codec`]),
//! UTF-8 Encoded String (§1.5.4) and Binary Data (§1.5.6). The String
//! Pair (§1.5.7) used by User Property is two §1.5.4 strings.

use crate::codec::{self, CodecError};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

/// Errors raised by the MQTT 5.0 codec layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("buffer underflow: need {needed} more byte(s)")]
    Underflow { needed: usize },
    #[error("variable-length integer exceeds the 4-byte / 268435455 maximum")]
    VarIntTooLong,
    #[error("UTF-8 string field is not valid UTF-8 (§1.5.4)")]
    BadUtf8,
    #[error("unknown property identifier 0x{0:02x} (§2.2.2.2)")]
    UnknownProperty(u8),
    #[error("property identifier 0x{0:02x} appears more than once but is not repeatable")]
    DuplicateProperty(u8),
    #[error("property block length {declared} disagrees with the bytes consumed {consumed}")]
    PropertyLengthMismatch { declared: usize, consumed: usize },
    #[error("Subscription Identifier value 0 is prohibited (§3.3.2.3.8)")]
    ZeroSubscriptionId,
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
    #[error("{packet} carries reserved/invalid reason code 0x{code:02x}")]
    BadReasonCode { packet: &'static str, code: u8 },
    #[error("unsupported protocol level {0} (MQTT 5.0 codec requires level 5)")]
    BadProtocolLevel(u8),
    #[error("unknown QoS value {0}")]
    BadQoS(u8),
    #[error("malformed protocol name {0:?} (expected \"MQTT\")")]
    BadProtocolName(String),
}

impl From<CodecError> for Error {
    fn from(e: CodecError) -> Self {
        match e {
            CodecError::Underflow { needed } => Error::Underflow { needed },
            CodecError::VarIntTooLong => Error::VarIntTooLong,
            CodecError::BadUtf8 => Error::BadUtf8,
            CodecError::BadQoS(q) => Error::BadQoS(q),
            other => Error::Malformed(Box::leak(other.to_string().into_boxed_str())),
        }
    }
}

pub(crate) fn get_u8(buf: &mut &[u8]) -> Result<u8, Error> {
    if buf.is_empty() {
        return Err(Error::Underflow { needed: 1 });
    }
    let v = buf[0];
    buf.advance(1);
    Ok(v)
}

pub(crate) fn get_u16(buf: &mut &[u8]) -> Result<u16, Error> {
    if buf.len() < 2 {
        return Err(Error::Underflow { needed: 2 - buf.len() });
    }
    let v = u16::from_be_bytes([buf[0], buf[1]]);
    buf.advance(2);
    Ok(v)
}

pub(crate) fn get_u32(buf: &mut &[u8]) -> Result<u32, Error> {
    if buf.len() < 4 {
        return Err(Error::Underflow { needed: 4 - buf.len() });
    }
    let v = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    buf.advance(4);
    Ok(v)
}

/// §1.5.5 Variable Byte Integer — reuses the shared 3.1.1 decoder.
pub(crate) fn get_var_int(buf: &mut &[u8]) -> Result<u32, Error> {
    let (value, consumed) = codec::decode_var_int(buf)?;
    buf.advance(consumed);
    Ok(value)
}

/// §1.5.4 UTF-8 Encoded String — 2-byte length prefix + UTF-8 bytes.
pub(crate) fn get_string(buf: &mut &[u8]) -> Result<String, Error> {
    let len = get_u16(buf)? as usize;
    if buf.len() < len {
        return Err(Error::Underflow { needed: len - buf.len() });
    }
    let s = std::str::from_utf8(&buf[..len])
        .map_err(|_| Error::BadUtf8)?
        .to_owned();
    buf.advance(len);
    Ok(s)
}

/// §1.5.6 Binary Data — 2-byte length prefix + raw bytes.
pub(crate) fn get_binary(buf: &mut &[u8]) -> Result<Bytes, Error> {
    let len = get_u16(buf)? as usize;
    if buf.len() < len {
        return Err(Error::Underflow { needed: len - buf.len() });
    }
    let b = Bytes::copy_from_slice(&buf[..len]);
    buf.advance(len);
    Ok(b)
}

pub(crate) fn put_u8(v: u8, out: &mut BytesMut) {
    out.put_u8(v);
}
pub(crate) fn put_u16(v: u16, out: &mut BytesMut) {
    out.put_u16(v);
}
pub(crate) fn put_u32(v: u32, out: &mut BytesMut) {
    out.put_u32(v);
}

pub(crate) fn put_var_int(v: u32, out: &mut BytesMut) -> Result<(), Error> {
    codec::encode_var_int(v, out).map_err(Error::from)
}

pub(crate) fn put_string(s: &str, out: &mut BytesMut) -> Result<(), Error> {
    let len = u16::try_from(s.len()).map_err(|_| Error::Malformed("string exceeds 65535 bytes"))?;
    out.put_u16(len);
    out.put_slice(s.as_bytes());
    Ok(())
}

pub(crate) fn put_binary(b: &[u8], out: &mut BytesMut) -> Result<(), Error> {
    let len = u16::try_from(b.len()).map_err(|_| Error::Malformed("binary exceeds 65535 bytes"))?;
    out.put_u16(len);
    out.put_slice(b);
    Ok(())
}
