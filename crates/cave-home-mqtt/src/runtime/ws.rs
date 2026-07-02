//! MQTT-over-WebSocket transport (§6). A binary WebSocket message may
//! carry several whole MQTT packets or a partial one, so inbound bytes
//! are buffered and drained packet-by-packet.

use super::{ConnMsg, Hub};
use crate::runtime::frame::drain_packets;
use crate::v5::codec::encode_v5;
use crate::v5::packet::PacketV5;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

pub(super) async fn serve_ws(hub: Arc<Hub>, sock: TcpStream) {
    let ws = match tokio_tungstenite::accept_async(sock).await {
        Ok(w) => w,
        Err(_) => return,
    };
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ConnMsg>();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                ConnMsg::Packet(p) => match encode_v5(&p) {
                    Ok(buf) => {
                        if sink.send(Message::Binary(buf.to_vec())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                ConnMsg::Close => {
                    let _ = sink.send(Message::Close(None)).await;
                    let _ = sink.close().await;
                    break;
                }
            }
        }
    });

    let mut buf: Vec<u8> = Vec::new();
    let mut client: Option<(String, u64)> = None;
    'outer: loop {
        let packets = match drain_packets(&mut buf) {
            Ok(p) => p,
            Err(_) => break,
        };
        for pkt in packets {
            match client.as_ref() {
                None => match pkt {
                    PacketV5::Connect(c) => match hub.on_connect(c, &tx).await {
                        Some(id) => client = Some(id),
                        None => break 'outer,
                    },
                    _ => {
                        let _ = tx.send(ConnMsg::Close);
                        break 'outer;
                    }
                },
                Some((id, _)) => match pkt {
                    PacketV5::Disconnect(d) => {
                        hub.handle(id, PacketV5::Disconnect(d)).await;
                        break 'outer;
                    }
                    other => hub.handle(id, other).await,
                },
            }
        }

        match stream.next().await {
            Some(Ok(Message::Binary(b))) => buf.extend_from_slice(&b),
            Some(Ok(Message::Text(t))) => buf.extend_from_slice(t.as_bytes()),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
        }
    }

    if let Some((id, conn_id)) = client {
        hub.disconnect(&id, conn_id).await;
    }
    let _ = tx.send(ConnMsg::Close);
    let _ = writer.await;
}
