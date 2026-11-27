//! Pinning tests for the async runtime: the packet framing over byte
//! streams and what a client observes on a real TCP or WebSocket
//! connection when the broker accepts, refuses or closes it. Compiled only
//! with `--features runtime`.
//!
//! Section numbers refer to OASIS MQTT 5.0 (§2.1.4 Remaining Length, §6
//! MQTT over WebSocket).
#![cfg(feature = "runtime")]

use bytes::Bytes;
use cave_home_mqtt::broker::auth::Authenticator;
use cave_home_mqtt::broker::{Broker, BrokerConfig};
use cave_home_mqtt::packet::QoS;
use cave_home_mqtt::runtime::Server;
use cave_home_mqtt::runtime::frame::{drain_packets, read_packet, write_packet};
use cave_home_mqtt::v5::codec::{decode_v5, encode_v5};
use cave_home_mqtt::v5::packet::{
    ConnAckV5, ConnectV5, DisconnectV5, PacketV5, PubAckV5, PublishV5, RetainHandling, SubAckV5,
    SubscribeV5, SubscriptionV5, Will,
};
use cave_home_mqtt::v5::reason::ReasonCode;
use futures_util::{SinkExt, StreamExt};
use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;

/// Upper bound for any single step; a broker that stops answering fails the
/// test instead of hanging it.
const WAIT: Duration = Duration::from_secs(10);

// ---- builders -------------------------------------------------------------------

fn open_broker() -> Broker {
    let mut auth = Authenticator::default();
    auth.set_anonymous(true);
    auth.set_default_allow(true);
    Broker::new(BrokerConfig::default(), auth)
}

/// A broker that knows no user and refuses anonymous clients.
fn closed_broker() -> Broker {
    Broker::new(BrokerConfig::default(), Authenticator::default())
}

fn connect(client_id: &str) -> PacketV5 {
    PacketV5::Connect(ConnectV5 {
        client_id: client_id.to_owned(),
        clean_start: true,
        keep_alive_secs: 0,
        properties: vec![],
        will: None,
        username: None,
        password: None,
    })
}

const fn connack(reason_code: ReasonCode) -> PacketV5 {
    PacketV5::ConnAck(ConnAckV5 {
        session_present: false,
        reason_code,
        properties: vec![],
    })
}

fn subscribe(filter: &str, packet_id: u16) -> PacketV5 {
    PacketV5::Subscribe(SubscribeV5 {
        packet_id,
        properties: vec![],
        subscriptions: vec![SubscriptionV5 {
            topic_filter: filter.to_owned(),
            qos: QoS::AtMostOnce,
            no_local: false,
            retain_as_published: false,
            retain_handling: RetainHandling::SendOnSubscribe,
        }],
    })
}

fn suback(packet_id: u16) -> PacketV5 {
    PacketV5::SubAck(SubAckV5 {
        packet_id,
        properties: vec![],
        reason_codes: vec![ReasonCode::Success],
    })
}

fn publish(topic: &str, payload: Vec<u8>) -> PacketV5 {
    PacketV5::Publish(PublishV5 {
        topic: topic.to_owned(),
        qos: QoS::AtMostOnce,
        retain: false,
        dup: false,
        packet_id: None,
        properties: vec![],
        payload: Bytes::from(payload),
    })
}

const fn puback(packet_id: u16) -> PacketV5 {
    PacketV5::PubAck(PubAckV5 {
        packet_id,
        reason_code: ReasonCode::Success,
        properties: vec![],
    })
}

fn frame(packet: &PacketV5) -> Result<Vec<u8>, cave_home_mqtt::v5::Error> {
    Ok(encode_v5(packet)?.to_vec())
}

// ---- §2.1.4 framing over byte streams ---------------------------------------------

#[tokio::test]
async fn read_packet_takes_one_frame_at_a_time() -> TestResult {
    let mut stream: &[u8] = &[0xc0, 0x00, 0x40, 0x02, 0x00, 0x07, 0xd0, 0x00];
    assert_eq!(read_packet(&mut stream).await?, Some(PacketV5::PingReq));
    assert_eq!(stream, [0x40, 0x02, 0x00, 0x07, 0xd0, 0x00]);
    assert_eq!(read_packet(&mut stream).await?, Some(puback(7)));
    assert_eq!(read_packet(&mut stream).await?, Some(PacketV5::PingResp));
    // A clean end of stream between two packets is not an error.
    assert_eq!(read_packet(&mut stream).await?, None);
    assert_eq!(read_packet(&mut stream).await?, None);
    Ok(())
}

#[tokio::test]
async fn read_packet_follows_a_multi_byte_remaining_length() -> TestResult {
    let packet = publish("t", vec![0x5a; 300]);
    let mut wire = Vec::new();
    write_packet(&mut wire, &packet).await?;
    // 2 + 1 topic, 1 property length, 300 payload = 304 = 0xb0 0x02.
    assert_eq!(wire.get(..3), Some(&[0x30, 0xb0, 0x02][..]));
    assert_eq!(wire.len(), 3 + 304);
    let mut stream: &[u8] = &wire;
    assert_eq!(read_packet(&mut stream).await?, Some(packet));
    assert_eq!(stream, [0_u8; 0]);
    Ok(())
}

#[tokio::test]
async fn read_packet_reports_a_stream_that_ends_inside_a_packet() {
    let cut_short: [&[u8]; 4] = [
        // After the first header byte.
        &[0xc0],
        // Inside the Remaining Length.
        &[0x30, 0x80],
        // Inside the body.
        &[0x40, 0x02, 0x00],
        &[0x30, 0x80, 0x80, 0x80, 0x01, 0x00],
    ];
    for bytes in cut_short {
        let mut stream = bytes;
        let kind = read_packet(&mut stream).await.map_err(|e| e.kind());
        assert_eq!(kind, Err(ErrorKind::UnexpectedEof), "{bytes:02x?}");
    }
}

#[tokio::test]
async fn read_packet_rejects_malformed_frames_as_invalid_data() {
    let cases: [(&[u8], &str); 4] = [
        // A continuation bit on the fourth Remaining Length byte.
        (
            &[0x30, 0x80, 0x80, 0x80, 0x80, 0x01],
            "malformed Remaining Length",
        ),
        (
            &[0x30, 0xff, 0xff, 0xff, 0xff],
            "malformed Remaining Length",
        ),
        // Whole frames the v5 decoder refuses keep its message.
        (
            &[0x00, 0x00],
            "malformed packet: unknown MQTT control packet type",
        ),
        (
            &[0x62, 0x03, 0x00, 0x09, 0x03],
            "PUBREL carries reserved/invalid reason code 0x03",
        ),
    ];
    for (bytes, message) in cases {
        let mut stream = bytes;
        let error = read_packet(&mut stream)
            .await
            .map_err(|e| (e.kind(), e.to_string()));
        assert_eq!(
            error,
            Err((ErrorKind::InvalidData, message.to_owned())),
            "{bytes:02x?}"
        );
    }
}

#[tokio::test]
async fn write_packet_appends_the_encoded_frame() -> TestResult {
    let mut wire = Vec::new();
    write_packet(&mut wire, &PacketV5::PingReq).await?;
    assert_eq!(wire, [0xc0, 0x00]);
    write_packet(&mut wire, &puback(7)).await?;
    assert_eq!(wire, [0xc0, 0x00, 0x40, 0x02, 0x00, 0x07]);

    // A packet that cannot be encoded writes nothing.
    let error = write_packet(&mut wire, &publish(&"t".repeat(65_536), vec![]))
        .await
        .map_err(|e| (e.kind(), e.to_string()));
    assert_eq!(
        error,
        Err((
            ErrorKind::InvalidData,
            "malformed packet: string exceeds 65535 bytes".to_owned()
        ))
    );
    assert_eq!(wire.len(), 6);
    Ok(())
}

#[test]
fn drain_packets_removes_whole_frames_and_keeps_a_partial_one() -> TestResult {
    let mut buf = vec![0xc0, 0x00, 0x40, 0x02, 0x00, 0x07, 0x40, 0x02];
    assert_eq!(drain_packets(&mut buf)?, [PacketV5::PingReq, puback(7)]);
    assert_eq!(buf, [0x40, 0x02]);
    // Nothing whole yet: the buffer is left alone.
    assert_eq!(drain_packets(&mut buf)?, []);
    assert_eq!(buf, [0x40, 0x02]);
    buf.extend_from_slice(&[0x00, 0x09]);
    assert_eq!(drain_packets(&mut buf)?, [puback(9)]);
    assert_eq!(buf, [0_u8; 0]);
    assert_eq!(drain_packets(&mut buf)?, []);
    Ok(())
}

#[test]
fn drain_packets_fails_on_a_malformed_frame_after_removing_the_ones_before_it() {
    let mut buf = vec![0xc0, 0x00, 0x00, 0x00, 0xd0, 0x00];
    let error = drain_packets(&mut buf).map_err(|e| (e.kind(), e.to_string()));
    assert_eq!(
        error,
        Err((
            ErrorKind::InvalidData,
            "malformed packet: unknown MQTT control packet type".to_owned()
        ))
    );
    assert_eq!(buf, [0x00, 0x00, 0xd0, 0x00]);
}

// ---- TCP ------------------------------------------------------------------------

async fn tcp_server(broker: Broker) -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = Server::new(broker);
    tokio::spawn(async move {
        let _ = server.serve_tcp(listener).await;
    });
    Ok(addr)
}

/// The next packet from the broker, or `None` once it has closed the
/// connection.
async fn next_packet<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Option<PacketV5>, TestError> {
    Ok(timeout(WAIT, read_packet(stream)).await??)
}

/// Open a TCP connection and complete the CONNECT / CONNACK exchange.
async fn tcp_client(addr: SocketAddr, client_id: &str) -> Result<TcpStream, TestError> {
    let mut stream = TcpStream::connect(addr).await?;
    write_packet(&mut stream, &connect(client_id)).await?;
    assert_eq!(
        next_packet(&mut stream).await?,
        Some(connack(ReasonCode::Success))
    );
    Ok(stream)
}

#[tokio::test]
async fn tcp_serves_packets_in_order_until_disconnect() -> TestResult {
    let addr = tcp_server(open_broker()).await?;
    let mut client = tcp_client(addr, "c").await?;
    // Three packets in one write are answered one by one.
    let mut burst = frame(&subscribe("t", 1))?;
    burst.extend(frame(&PacketV5::PingReq)?);
    burst.extend(frame(&publish("t", b"hello".to_vec()))?);
    client.write_all(&burst).await?;
    assert_eq!(next_packet(&mut client).await?, Some(suback(1)));
    assert_eq!(next_packet(&mut client).await?, Some(PacketV5::PingResp));
    assert_eq!(
        next_packet(&mut client).await?,
        Some(publish("t", b"hello".to_vec()))
    );

    // §3.14.4: after DISCONNECT the broker closes the connection.
    write_packet(
        &mut client,
        &PacketV5::Disconnect(DisconnectV5 {
            reason_code: ReasonCode::Success,
            properties: vec![],
        }),
    )
    .await?;
    assert_eq!(next_packet(&mut client).await?, None);
    Ok(())
}

#[tokio::test]
async fn tcp_closes_a_connection_that_does_not_start_with_connect() -> TestResult {
    let addr = tcp_server(open_broker()).await?;
    let openers: [&[u8]; 3] = [
        // PINGREQ before CONNECT.
        &[0xc0, 0x00],
        // A frame the decoder refuses.
        &[0x00, 0x00],
        // A Remaining Length that never ends.
        &[0x10, 0xff, 0xff, 0xff, 0xff],
    ];
    for opener in openers {
        let mut stream = TcpStream::connect(addr).await?;
        stream.write_all(opener).await?;
        // Closed without a single byte in reply.
        let mut reply = Vec::new();
        timeout(WAIT, stream.read_to_end(&mut reply)).await??;
        assert_eq!(reply, [0_u8; 0], "{opener:02x?}");
    }
    Ok(())
}

#[tokio::test]
async fn tcp_refuses_bad_credentials_with_a_connack_and_closes() -> TestResult {
    let addr = tcp_server(closed_broker()).await?;
    let mut stream = TcpStream::connect(addr).await?;
    write_packet(&mut stream, &connect("c")).await?;
    assert_eq!(
        next_packet(&mut stream).await?,
        Some(connack(ReasonCode::BadUserNameOrPassword))
    );
    assert_eq!(next_packet(&mut stream).await?, None);
    Ok(())
}

#[tokio::test]
async fn tcp_closes_the_connection_on_a_malformed_packet_after_connect() -> TestResult {
    let addr = tcp_server(open_broker()).await?;
    let mut client = tcp_client(addr, "c").await?;
    client.write_all(&[0x00, 0x00]).await?;
    assert_eq!(next_packet(&mut client).await?, None);
    Ok(())
}

#[tokio::test]
async fn tcp_takeover_closes_the_old_connection_and_keeps_the_new_one() -> TestResult {
    let addr = tcp_server(open_broker()).await?;
    let mut old = tcp_client(addr, "dup").await?;
    let mut new = tcp_client(addr, "dup").await?;
    // §3.1.4.3: the first connection is closed by the broker.
    assert_eq!(next_packet(&mut old).await?, None);
    drop(old);
    // The teardown of the old connection must not detach the new one.
    tokio::time::sleep(Duration::from_millis(100)).await;
    write_packet(&mut new, &subscribe("t", 1)).await?;
    assert_eq!(next_packet(&mut new).await?, Some(suback(1)));
    let mut publisher = tcp_client(addr, "pub").await?;
    write_packet(&mut publisher, &publish("t", b"x".to_vec())).await?;
    assert_eq!(
        next_packet(&mut new).await?,
        Some(publish("t", b"x".to_vec()))
    );
    Ok(())
}

#[tokio::test]
async fn tcp_publishes_the_will_when_the_socket_is_dropped() -> TestResult {
    let addr = tcp_server(open_broker()).await?;
    let mut watcher = tcp_client(addr, "watcher").await?;
    write_packet(&mut watcher, &subscribe("status/+", 1)).await?;
    assert_eq!(next_packet(&mut watcher).await?, Some(suback(1)));

    let mut dier = TcpStream::connect(addr).await?;
    write_packet(
        &mut dier,
        &PacketV5::Connect(ConnectV5 {
            client_id: "dier".to_owned(),
            clean_start: true,
            keep_alive_secs: 0,
            properties: vec![],
            will: Some(Will {
                topic: "status/dier".to_owned(),
                payload: Bytes::from_static(b"offline"),
                qos: QoS::AtMostOnce,
                retain: false,
                properties: vec![],
            }),
            username: None,
            password: None,
        }),
    )
    .await?;
    assert_eq!(
        next_packet(&mut dier).await?,
        Some(connack(ReasonCode::Success))
    );
    drop(dier);
    assert_eq!(
        next_packet(&mut watcher).await?,
        Some(publish("status/dier", b"offline".to_vec()))
    );
    Ok(())
}

// ---- §6 MQTT over WebSocket ---------------------------------------------------------

type Ws = WebSocketStream<TcpStream>;

async fn ws_server(broker: Broker) -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = Server::new(broker);
    tokio::spawn(async move {
        let _ = server.serve_ws(listener).await;
    });
    Ok(addr)
}

async fn ws_client(addr: SocketAddr) -> Result<Ws, TestError> {
    let tcp = TcpStream::connect(addr).await?;
    let (ws, _response) = tokio_tungstenite::client_async(format!("ws://{addr}/mqtt"), tcp).await?;
    Ok(ws)
}

/// What the broker sent next over the WebSocket.
#[derive(Debug, PartialEq, Eq)]
enum WsEvent {
    /// One binary message holding exactly one MQTT packet.
    Packet(PacketV5),
    /// A Close frame.
    Close,
}

async fn ws_next(ws: &mut Ws) -> Result<WsEvent, TestError> {
    loop {
        match timeout(WAIT, ws.next()).await? {
            Some(Ok(Message::Binary(bytes))) => {
                let (packet, used) = decode_v5(&bytes)?;
                assert_eq!(used, bytes.len(), "one packet per binary message");
                return Ok(WsEvent::Packet(packet));
            }
            Some(Ok(Message::Close(_))) => return Ok(WsEvent::Close),
            // The answer to a Ping this test sent itself.
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(other)) => return Err(format!("unexpected message {other:?}").into()),
            Some(Err(error)) => return Err(error.into()),
            None => return Err("the WebSocket ended without a Close frame".into()),
        }
    }
}

#[tokio::test]
async fn websocket_answers_each_packet_in_its_own_binary_message() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    // Several MQTT packets may share one WebSocket message.
    let mut burst = frame(&connect("ws"))?;
    burst.extend(frame(&subscribe("t", 1))?);
    burst.extend(frame(&PacketV5::PingReq)?);
    ws.send(Message::Binary(burst)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Packet(suback(1)));
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Packet(PacketV5::PingResp));
    Ok(())
}

#[tokio::test]
async fn websocket_reassembles_a_packet_split_across_messages() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    let whole = frame(&connect("ws"))?;
    let (head, tail) = whole.split_at(5);
    ws.send(Message::Binary(head.to_vec())).await?;
    ws.send(Message::Binary(tail.to_vec())).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    Ok(())
}

#[tokio::test]
async fn websocket_currently_reads_text_messages_as_packet_bytes() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    // Every byte of this CONNECT is below 0x80, so it is valid UTF-8.
    let text = String::from_utf8(frame(&connect("ws"))?)?;
    ws.send(Message::Text(text)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    Ok(())
}

#[tokio::test]
async fn websocket_ignores_ping_frames_between_packets() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(frame(&connect("ws"))?)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    ws.send(Message::Ping(vec![1, 2, 3])).await?;
    ws.send(Message::Binary(frame(&PacketV5::PingReq)?)).await?;
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Packet(PacketV5::PingResp));
    Ok(())
}

#[tokio::test]
async fn websocket_closes_when_the_first_packet_is_not_connect() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(frame(&PacketV5::PingReq)?)).await?;
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);
    Ok(())
}

#[tokio::test]
async fn websocket_closes_on_malformed_packet_bytes() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    // Before CONNECT.
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(vec![0x00, 0x00])).await?;
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);
    // After CONNECT.
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(frame(&connect("ws"))?)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    ws.send(Message::Binary(vec![0x00, 0x00])).await?;
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);
    Ok(())
}

#[tokio::test]
async fn websocket_refuses_bad_credentials_with_a_connack_and_closes() -> TestResult {
    let addr = ws_server(closed_broker()).await?;
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(frame(&connect("ws"))?)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::BadUserNameOrPassword))
    );
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);
    Ok(())
}

#[tokio::test]
async fn websocket_closes_after_disconnect_and_answers_a_client_close() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut ws = ws_client(addr).await?;
    let mut burst = frame(&connect("ws"))?;
    burst.extend(frame(&PacketV5::Disconnect(DisconnectV5 {
        reason_code: ReasonCode::Success,
        properties: vec![],
    }))?);
    ws.send(Message::Binary(burst)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);

    // A client that closes first gets the closing handshake completed.
    let mut ws = ws_client(addr).await?;
    ws.send(Message::Binary(frame(&connect("ws2"))?)).await?;
    assert_eq!(
        ws_next(&mut ws).await?,
        WsEvent::Packet(connack(ReasonCode::Success))
    );
    ws.send(Message::Close(None)).await?;
    assert_eq!(ws_next(&mut ws).await?, WsEvent::Close);
    Ok(())
}

#[tokio::test]
async fn websocket_listener_drops_a_client_that_does_not_speak_websocket() -> TestResult {
    let addr = ws_server(open_broker()).await?;
    let mut stream = TcpStream::connect(addr).await?;
    // A raw MQTT CONNECT instead of the HTTP upgrade request.
    stream.write_all(&frame(&connect("raw"))?).await?;
    stream.shutdown().await?;
    // The broker ends the connection (how is up to the WebSocket library)
    // and no MQTT packet comes back.
    let mut reply = Vec::new();
    let _ = timeout(WAIT, stream.read_to_end(&mut reply)).await?;
    assert_eq!(decode_v5(&reply).ok(), None);
    Ok(())
}
