//! MQTT 5.0 §2.2.2 — Properties.
//!
//! A property block is a Variable Byte Integer length (§2.2.2.1) followed
//! by zero or more properties, each a property Identifier (a Variable
//! Byte Integer, §2.2.2.2) and a value whose wire type is fixed by the
//! identifier. Most identifiers may appear at most once; only User
//! Property (0x26) and Subscription Identifier (0x0B) repeat.

use super::wire::{
    get_binary, get_string, get_u16, get_u32, get_u8, get_var_int, put_binary,
    put_string, put_u16, put_u32, put_u8, put_var_int, Error,
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
    pub fn identifier(&self) -> u8 {
        match self {
            Property::PayloadFormatIndicator(_) => 0x01,
            Property::MessageExpiryInterval(_) => 0x02,
            Property::ContentType(_) => 0x03,
            Property::ResponseTopic(_) => 0x08,
            Property::CorrelationData(_) => 0x09,
            Property::SubscriptionIdentifier(_) => 0x0B,
            Property::SessionExpiryInterval(_) => 0x11,
            Property::AssignedClientIdentifier(_) => 0x12,
            Property::ServerKeepAlive(_) => 0x13,
            Property::AuthenticationMethod(_) => 0x15,
            Property::AuthenticationData(_) => 0x16,
            Property::RequestProblemInformation(_) => 0x17,
            Property::WillDelayInterval(_) => 0x18,
            Property::RequestResponseInformation(_) => 0x19,
            Property::ResponseInformation(_) => 0x1A,
            Property::ServerReference(_) => 0x1C,
            Property::ReasonString(_) => 0x1F,
            Property::ReceiveMaximum(_) => 0x21,
            Property::TopicAliasMaximum(_) => 0x22,
            Property::TopicAlias(_) => 0x23,
            Property::MaximumQoS(_) => 0x24,
            Property::RetainAvailable(_) => 0x25,
            Property::UserProperty(..) => 0x26,
            Property::MaximumPacketSize(_) => 0x27,
            Property::WildcardSubscriptionAvailable(_) => 0x28,
            Property::SubscriptionIdentifierAvailable(_) => 0x29,
            Property::SharedSubscriptionAvailable(_) => 0x2A,
        }
    }

    /// §2.2.2 — only User Property and Subscription Identifier may repeat.
    fn repeatable(id: u8) -> bool {
        id == 0x26 || id == 0x0B
    }

    fn encode_value(&self, out: &mut BytesMut) -> Result<(), Error> {
        match self {
            Property::PayloadFormatIndicator(v)
            | Property::RequestProblemInformation(v)
            | Property::RequestResponseInformation(v)
            | Property::MaximumQoS(v)
            | Property::RetainAvailable(v)
            | Property::WildcardSubscriptionAvailable(v)
            | Property::SubscriptionIdentifierAvailable(v)
            | Property::SharedSubscriptionAvailable(v) => put_u8(*v, out),
            Property::ServerKeepAlive(v)
            | Property::ReceiveMaximum(v)
            | Property::TopicAliasMaximum(v)
            | Property::TopicAlias(v) => put_u16(*v, out),
            Property::MessageExpiryInterval(v)
            | Property::SessionExpiryInterval(v)
            | Property::WillDelayInterval(v)
            | Property::MaximumPacketSize(v) => put_u32(*v, out),
            Property::SubscriptionIdentifier(v) => put_var_int(*v, out)?,
            Property::ContentType(s)
            | Property::ResponseTopic(s)
            | Property::AssignedClientIdentifier(s)
            | Property::AuthenticationMethod(s)
            | Property::ResponseInformation(s)
            | Property::ServerReference(s)
            | Property::ReasonString(s) => put_string(s, out)?,
            Property::CorrelationData(b) | Property::AuthenticationData(b) => {
                put_binary(b, out)?
            }
            Property::UserProperty(k, v) => {
                put_string(k, out)?;
                put_string(v, out)?;
            }
        }
        Ok(())
    }

    fn decode_value(id: u8, buf: &mut &[u8]) -> Result<Property, Error> {
        Ok(match id {
            0x01 => Property::PayloadFormatIndicator(get_u8(buf)?),
            0x02 => Property::MessageExpiryInterval(get_u32(buf)?),
            0x03 => Property::ContentType(get_string(buf)?),
            0x08 => Property::ResponseTopic(get_string(buf)?),
            0x09 => Property::CorrelationData(get_binary(buf)?),
            0x0B => {
                let v = get_var_int(buf)?;
                if v == 0 {
                    return Err(Error::ZeroSubscriptionId);
                }
                Property::SubscriptionIdentifier(v)
            }
            0x11 => Property::SessionExpiryInterval(get_u32(buf)?),
            0x12 => Property::AssignedClientIdentifier(get_string(buf)?),
            0x13 => Property::ServerKeepAlive(get_u16(buf)?),
            0x15 => Property::AuthenticationMethod(get_string(buf)?),
            0x16 => Property::AuthenticationData(get_binary(buf)?),
            0x17 => Property::RequestProblemInformation(get_u8(buf)?),
            0x18 => Property::WillDelayInterval(get_u32(buf)?),
            0x19 => Property::RequestResponseInformation(get_u8(buf)?),
            0x1A => Property::ResponseInformation(get_string(buf)?),
            0x1C => Property::ServerReference(get_string(buf)?),
            0x1F => Property::ReasonString(get_string(buf)?),
            0x21 => Property::ReceiveMaximum(get_u16(buf)?),
            0x22 => Property::TopicAliasMaximum(get_u16(buf)?),
            0x23 => Property::TopicAlias(get_u16(buf)?),
            0x24 => Property::MaximumQoS(get_u8(buf)?),
            0x25 => Property::RetainAvailable(get_u8(buf)?),
            0x26 => {
                let k = get_string(buf)?;
                let v = get_string(buf)?;
                Property::UserProperty(k, v)
            }
            0x27 => Property::MaximumPacketSize(get_u32(buf)?),
            0x28 => Property::WildcardSubscriptionAvailable(get_u8(buf)?),
            0x29 => Property::SubscriptionIdentifierAvailable(get_u8(buf)?),
            0x2A => Property::SharedSubscriptionAvailable(get_u8(buf)?),
            other => return Err(Error::UnknownProperty(other)),
        })
    }
}

/// Encode a property block: a Variable Byte Integer length (§2.2.2.1)
/// followed by each property as `identifier || value`.
pub fn encode_properties(props: &[Property], out: &mut BytesMut) -> Result<(), Error> {
    let mut body = BytesMut::new();
    for p in props {
        put_u8(p.identifier(), &mut body);
        p.encode_value(&mut body)?;
    }
    put_var_int(u32::try_from(body.len()).map_err(|_| Error::VarIntTooLong)?, out)?;
    out.extend_from_slice(&body);
    Ok(())
}

/// Decode a property block from the cursor, advancing past it.
pub fn decode_properties(buf: &mut &[u8]) -> Result<Vec<Property>, Error> {
    let len = get_var_int(buf)? as usize;
    if buf.len() < len {
        return Err(Error::Underflow { needed: len - buf.len() });
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
