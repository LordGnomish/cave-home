//! MQTT 5.0 §2.2.2 — Properties.
//!
//! A property block is a Variable Byte Integer length (§2.2.2.1) followed
//! by zero or more properties, each a property Identifier (a Variable
//! Byte Integer, §2.2.2.2) and a value whose wire type is fixed by the
//! identifier. Most identifiers may appear at most once; only User
//! Property (0x26) and Subscription Identifier (0x0B) repeat.

use super::wire::{
    Error, get_binary, get_string, get_u8, get_u16, get_u32, get_var_int, put_binary, put_string,
    put_u8, put_u16, put_u32, put_var_int,
};
use bytes::{Bytes, BytesMut};

/// A single MQTT 5.0 property (§2.2.2.2). The enum discriminant is *not*
/// the wire identifier; [`Property::identifier`] returns that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Property {
    PayloadFormatIndicator(u8),
    MessageExpiryInterval(u32),
    ContentType(String),
    ResponseTopic(String),
    CorrelationData(Bytes),
    SubscriptionIdentifier(u32),
    SessionExpiryInterval(u32),
    AssignedClientIdentifier(String),
    ServerKeepAlive(u16),
    AuthenticationMethod(String),
    AuthenticationData(Bytes),
    RequestProblemInformation(u8),
    WillDelayInterval(u32),
    RequestResponseInformation(u8),
    ResponseInformation(String),
    ServerReference(String),
    ReasonString(String),
    ReceiveMaximum(u16),
    TopicAliasMaximum(u16),
    TopicAlias(u16),
    MaximumQoS(u8),
    RetainAvailable(u8),
    UserProperty(String, String),
    MaximumPacketSize(u32),
    WildcardSubscriptionAvailable(u8),
    SubscriptionIdentifierAvailable(u8),
    SharedSubscriptionAvailable(u8),
}

impl Property {
    /// The on-wire property identifier byte (§2.2.2.2).
    pub const fn identifier(&self) -> u8 {
        match self {
            Self::PayloadFormatIndicator(_) => 0x01,
            Self::MessageExpiryInterval(_) => 0x02,
            Self::ContentType(_) => 0x03,
            Self::ResponseTopic(_) => 0x08,
            Self::CorrelationData(_) => 0x09,
            Self::SubscriptionIdentifier(_) => 0x0B,
            Self::SessionExpiryInterval(_) => 0x11,
            Self::AssignedClientIdentifier(_) => 0x12,
            Self::ServerKeepAlive(_) => 0x13,
            Self::AuthenticationMethod(_) => 0x15,
            Self::AuthenticationData(_) => 0x16,
            Self::RequestProblemInformation(_) => 0x17,
            Self::WillDelayInterval(_) => 0x18,
            Self::RequestResponseInformation(_) => 0x19,
            Self::ResponseInformation(_) => 0x1A,
            Self::ServerReference(_) => 0x1C,
            Self::ReasonString(_) => 0x1F,
            Self::ReceiveMaximum(_) => 0x21,
            Self::TopicAliasMaximum(_) => 0x22,
            Self::TopicAlias(_) => 0x23,
            Self::MaximumQoS(_) => 0x24,
            Self::RetainAvailable(_) => 0x25,
            Self::UserProperty(..) => 0x26,
            Self::MaximumPacketSize(_) => 0x27,
            Self::WildcardSubscriptionAvailable(_) => 0x28,
            Self::SubscriptionIdentifierAvailable(_) => 0x29,
            Self::SharedSubscriptionAvailable(_) => 0x2A,
        }
    }

    /// §2.2.2 — only User Property and Subscription Identifier may repeat.
    const fn repeatable(id: u8) -> bool {
        id == 0x26 || id == 0x0B
    }

    fn encode_value(&self, out: &mut BytesMut) -> Result<(), Error> {
        match self {
            Self::PayloadFormatIndicator(v)
            | Self::RequestProblemInformation(v)
            | Self::RequestResponseInformation(v)
            | Self::MaximumQoS(v)
            | Self::RetainAvailable(v)
            | Self::WildcardSubscriptionAvailable(v)
            | Self::SubscriptionIdentifierAvailable(v)
            | Self::SharedSubscriptionAvailable(v) => put_u8(*v, out),
            Self::ServerKeepAlive(v)
            | Self::ReceiveMaximum(v)
            | Self::TopicAliasMaximum(v)
            | Self::TopicAlias(v) => put_u16(*v, out),
            Self::MessageExpiryInterval(v)
            | Self::SessionExpiryInterval(v)
            | Self::WillDelayInterval(v)
            | Self::MaximumPacketSize(v) => put_u32(*v, out),
            Self::SubscriptionIdentifier(v) => put_var_int(*v, out)?,
            Self::ContentType(s)
            | Self::ResponseTopic(s)
            | Self::AssignedClientIdentifier(s)
            | Self::AuthenticationMethod(s)
            | Self::ResponseInformation(s)
            | Self::ServerReference(s)
            | Self::ReasonString(s) => put_string(s, out)?,
            Self::CorrelationData(b) | Self::AuthenticationData(b) => put_binary(b, out)?,
            Self::UserProperty(k, v) => {
                put_string(k, out)?;
                put_string(v, out)?;
            }
        }
        Ok(())
    }

    fn decode_value(id: u8, buf: &mut &[u8]) -> Result<Self, Error> {
        Ok(match id {
            0x01 => Self::PayloadFormatIndicator(get_u8(buf)?),
            0x02 => Self::MessageExpiryInterval(get_u32(buf)?),
            0x03 => Self::ContentType(get_string(buf)?),
            0x08 => Self::ResponseTopic(get_string(buf)?),
            0x09 => Self::CorrelationData(get_binary(buf)?),
            0x0B => {
                let v = get_var_int(buf)?;
                if v == 0 {
                    return Err(Error::ZeroSubscriptionId);
                }
                Self::SubscriptionIdentifier(v)
            }
            0x11 => Self::SessionExpiryInterval(get_u32(buf)?),
            0x12 => Self::AssignedClientIdentifier(get_string(buf)?),
            0x13 => Self::ServerKeepAlive(get_u16(buf)?),
            0x15 => Self::AuthenticationMethod(get_string(buf)?),
            0x16 => Self::AuthenticationData(get_binary(buf)?),
            0x17 => Self::RequestProblemInformation(get_u8(buf)?),
            0x18 => Self::WillDelayInterval(get_u32(buf)?),
            0x19 => Self::RequestResponseInformation(get_u8(buf)?),
            0x1A => Self::ResponseInformation(get_string(buf)?),
            0x1C => Self::ServerReference(get_string(buf)?),
            0x1F => Self::ReasonString(get_string(buf)?),
            0x21 => Self::ReceiveMaximum(get_u16(buf)?),
            0x22 => Self::TopicAliasMaximum(get_u16(buf)?),
            0x23 => Self::TopicAlias(get_u16(buf)?),
            0x24 => Self::MaximumQoS(get_u8(buf)?),
            0x25 => Self::RetainAvailable(get_u8(buf)?),
            0x26 => {
                let k = get_string(buf)?;
                let v = get_string(buf)?;
                Self::UserProperty(k, v)
            }
            0x27 => Self::MaximumPacketSize(get_u32(buf)?),
            0x28 => Self::WildcardSubscriptionAvailable(get_u8(buf)?),
            0x29 => Self::SubscriptionIdentifierAvailable(get_u8(buf)?),
            0x2A => Self::SharedSubscriptionAvailable(get_u8(buf)?),
            other => return Err(Error::UnknownProperty(other)),
        })
    }
}

/// Encode a property block: a Variable Byte Integer length (§2.2.2.1)
/// followed by each property as `identifier || value`.
///
/// # Errors
/// Returns [`Error::Malformed`] for a string or binary value longer than
/// 65 535 bytes, and [`Error::VarIntTooLong`] for a Subscription Identifier
/// or a block length above 268 435 455; `out` is left untouched then.
pub fn encode_properties(props: &[Property], out: &mut BytesMut) -> Result<(), Error> {
    let mut body = BytesMut::new();
    for p in props {
        put_u8(p.identifier(), &mut body);
        p.encode_value(&mut body)?;
    }
    put_var_int(
        u32::try_from(body.len()).map_err(|_| Error::VarIntTooLong)?,
        out,
    )?;
    out.extend_from_slice(&body);
    Ok(())
}

/// Decode a property block from the cursor, advancing past it.
///
/// # Errors
/// Returns [`Error::Underflow`] for a truncated block or value,
/// [`Error::UnknownProperty`] or [`Error::DuplicateProperty`] for an
/// identifier that is not assigned or must not repeat, and
/// [`Error::ZeroSubscriptionId`], [`Error::BadUtf8`] or
/// [`Error::VarIntTooLong`] for a malformed value; the block is not
/// consumed then.
pub fn decode_properties(buf: &mut &[u8]) -> Result<Vec<Property>, Error> {
    let len = get_var_int(buf)? as usize;
    if buf.len() < len {
        return Err(Error::Underflow {
            needed: len - buf.len(),
        });
    }
    let (mut block, rest) = buf.split_at(len);
    let mut props = Vec::new();
    let mut seen = [false; 0x2B];
    while !block.is_empty() {
        let id = get_var_int(&mut block)?;
        let id = u8::try_from(id).map_err(|_| Error::UnknownProperty(0xff))?;
        if !Property::repeatable(id) {
            if (id as usize) < seen.len() && seen[id as usize] {
                return Err(Error::DuplicateProperty(id));
            }
            if (id as usize) < seen.len() {
                seen[id as usize] = true;
            }
        }
        props.push(Property::decode_value(id, &mut block)?);
    }
    *buf = rest;
    Ok(props)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Bytes, BytesMut};

    fn round_trip(props: &[Property]) -> Vec<Property> {
        let mut buf = BytesMut::new();
        encode_properties(props, &mut buf).expect("encode");
        let mut cursor: &[u8] = &buf;
        let decoded = decode_properties(&mut cursor).expect("decode");
        assert!(cursor.is_empty(), "decoder must consume the whole block");
        decoded
    }

    #[test]
    fn empty_property_set_is_a_single_zero_length_byte() {
        // §2.2.1.1: the Property Length is a Variable Byte Integer; an
        // empty set is encoded as the single byte 0x00.
        let mut buf = BytesMut::new();
        encode_properties(&[], &mut buf).expect("encode");
        assert_eq!(&buf[..], &[0x00]);
        assert_eq!(round_trip(&[]), Vec::<Property>::new());
    }

    #[test]
    fn mixed_property_set_round_trips_in_order() {
        // One of every wire data-type: byte, u32, utf8, binary, var-int,
        // and a repeated UTF-8 string pair (§2.2.2.2 User Property).
        let props = vec![
            Property::PayloadFormatIndicator(1),
            Property::MessageExpiryInterval(3600),
            Property::ContentType("application/json".into()),
            Property::ResponseTopic("home/reply".into()),
            Property::CorrelationData(Bytes::from_static(b"\x00\x01\xff")),
            Property::SubscriptionIdentifier(268_435_455),
            Property::UserProperty("region".into(), "loft".into()),
            Property::UserProperty("region".into(), "loft".into()),
        ];
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn connect_scoped_properties_round_trip() {
        let props = vec![
            Property::SessionExpiryInterval(120),
            Property::ReceiveMaximum(20),
            Property::MaximumPacketSize(1 << 20),
            Property::TopicAliasMaximum(10),
            Property::RequestResponseInformation(1),
            Property::RequestProblemInformation(0),
            Property::AuthenticationMethod("SCRAM-SHA-256".into()),
            Property::AuthenticationData(Bytes::from_static(b"nonce")),
        ];
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn length_prefix_is_a_variable_byte_integer() {
        // A reason string >127 bytes forces a 2-byte property-length prefix.
        let long = "x".repeat(300);
        let props = vec![Property::ReasonString(long.clone())];
        let mut buf = BytesMut::new();
        encode_properties(&props, &mut buf).expect("encode");
        // 0xAD 0x02 == 301 (300 payload + 1 id byte... 1 id + 2 strlen + 300).
        // Just assert it round-trips and the first length byte has the
        // continuation bit set (value > 127).
        assert!(buf[0] & 0x80 != 0, "multi-byte var-int length expected");
        assert_eq!(round_trip(&props), props);
    }

    #[test]
    fn decode_rejects_unknown_property_identifier() {
        // 0x07 is not an assigned property identifier in §2.2.2.2.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0x02, 0x07, 0x00]); // len=2, id=0x07, ...
        let mut cursor: &[u8] = &buf;
        assert!(matches!(
            decode_properties(&mut cursor),
            Err(crate::v5::Error::UnknownProperty(0x07))
        ));
    }

    #[test]
    fn decode_rejects_truncated_property_block() {
        // Declares 10 bytes of properties but supplies none.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0x0a]);
        let mut cursor: &[u8] = &buf;
        assert!(matches!(
            decode_properties(&mut cursor),
            Err(crate::v5::Error::Underflow { .. })
        ));
    }
}
