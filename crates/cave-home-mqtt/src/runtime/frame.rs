//! Async MQTT packet framing over byte streams (§2.1.4 Remaining Length).

use crate::v5::codec::{decode_v5, encode_v5};
use crate::v5::packet::PacketV5;
use std::io::{self, ErrorKind};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Read one whole MQTT control packet from `r`. Returns `Ok(None)` on a
/// clean end-of-stream before any byte of a new packet.
pub async fn read_packet<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Option<PacketV5>> {
    let mut first = [0u8; 1];
    match r.read_exact(&mut first).await {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    // §2.1.4 Remaining Length — up to four continuation bytes.
    let mut frame = vec![first[0]];
    let mut multiplier: u32 = 1;
    let mut remaining: u32 = 0;
    loop {
        let mut b = [0u8; 1];
        r.read_exact(&mut b).await?;
        frame.push(b[0]);
        remaining += u32::from(b[0] & 0x7f) * multiplier;
        if b[0] & 0x80 == 0 {
            break;
        }
        multiplier *= 128;
        if multiplier > 128 * 128 * 128 {
            return Err(io::Error::new(ErrorKind::InvalidData, "malformed Remaining Length"));
        }
    }

    let start = frame.len();
    frame.resize(start + remaining as usize, 0);
    r.read_exact(&mut frame[start..]).await?;

    let (packet, _used) =
        decode_v5(&frame).map_err(|e| io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(packet))
}

/// Encode and write one MQTT control packet, flushing the stream.
pub async fn write_packet<W: AsyncWrite + Unpin>(
    w: &mut W,
    packet: &PacketV5,
) -> io::Result<()> {
    let buf = encode_v5(packet).map_err(|e| io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
    w.write_all(&buf).await?;
    w.flush().await
}

/// Decode as many whole packets as are fully present at the front of
/// `buf`, removing them. Used by the WebSocket transport, where a binary
/// message may carry several or partial MQTT packets (§6 MQTT-over-WS).
pub fn drain_packets(buf: &mut Vec<u8>) -> io::Result<Vec<PacketV5>> {
    let mut out = Vec::new();
    loop {
        match decode_v5(buf) {
            Ok((packet, used)) => {
                buf.drain(..used);
                out.push(packet);
            }
            Err(crate::v5::Error::Underflow { .. }) => break, // wait for more bytes
            Err(e) => return Err(io::Error::new(ErrorKind::InvalidData, e.to_string())),
        }
    }
    Ok(out)
}
