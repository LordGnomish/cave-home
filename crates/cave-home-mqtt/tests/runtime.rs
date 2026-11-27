//! End-to-end async-runtime tests: real TCP / TLS / WebSocket sockets
//! driving the clean-room broker. Compiled only with `--features runtime`.
#![cfg(feature = "runtime")]

use bytes::Bytes;
use cave_home_mqtt::broker::auth::Authenticator;
use cave_home_mqtt::broker::{Broker, BrokerConfig};
use cave_home_mqtt::packet::QoS;
use cave_home_mqtt::runtime::Server;
use cave_home_mqtt::runtime::frame::{read_packet, write_packet};
use cave_home_mqtt::v5::packet::*;
use cave_home_mqtt::v5::property::Property;
use cave_home_mqtt::v5::reason::ReasonCode;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::net::{TcpListener, TcpStream};

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;

fn allow_all() -> Broker {
    let mut auth = Authenticator::default();
    auth.set_anonymous(true);
    auth.set_default_allow(true);
    Broker::new(BrokerConfig::default(), auth)
}

fn connect(id: &str, clean: bool, expiry: u32) -> PacketV5 {
    PacketV5::Connect(ConnectV5 {
        client_id: id.into(),
        clean_start: clean,
        keep_alive_secs: 0,
        properties: if expiry > 0 {
            vec![Property::SessionExpiryInterval(expiry)]
        } else {
            vec![]
        },
        will: None,
        username: None,
        password: None,
    })
}

fn subscribe(filter: &str, qos: QoS, pid: u16) -> PacketV5 {
    PacketV5::Subscribe(SubscribeV5 {
        packet_id: pid,
        properties: vec![],
        subscriptions: vec![SubscriptionV5 {
            topic_filter: filter.into(),
            qos,
            no_local: false,
            retain_as_published: false,
            retain_handling: RetainHandling::SendOnSubscribe,
        }],
    })
}

fn publish(topic: &str, qos: QoS, pid: Option<u16>, body: &'static [u8]) -> PacketV5 {
    PacketV5::Publish(PublishV5 {
        topic: topic.into(),
        qos,
        retain: false,
        dup: false,
        packet_id: pid,
        properties: vec![],
        payload: Bytes::from_static(body),
    })
}

async fn start_tcp() -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = Server::new(allow_all());
    tokio::spawn(async move {
        let _ = server.serve_tcp(listener).await;
    });
    Ok(addr)
}

/// The next packet from the broker; a closed connection is an error.
async fn next_packet<R: AsyncRead + Unpin>(stream: &mut R) -> Result<PacketV5, TestError> {
    Ok(read_packet(stream)
        .await?
        .ok_or("the broker closed the connection")?)
}

#[tokio::test]
async fn tcp_connect_is_acknowledged() -> TestResult {
    let addr = start_tcp().await?;
    let mut s = TcpStream::connect(addr).await?;
    write_packet(&mut s, &connect("c1", true, 0)).await?;
    match next_packet(&mut s).await? {
        PacketV5::ConnAck(ack) => assert_eq!(ack.reason_code, ReasonCode::Success),
        other => return Err(format!("expected CONNACK, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn tcp_qos1_publish_round_trip_between_two_clients() -> TestResult {
    let addr = start_tcp().await?;

    let mut sub = TcpStream::connect(addr).await?;
    write_packet(&mut sub, &connect("sub", true, 0)).await?;
    next_packet(&mut sub).await?; // CONNACK
    write_packet(&mut sub, &subscribe("home/#", QoS::AtLeastOnce, 1)).await?;
    match next_packet(&mut sub).await? {
        PacketV5::SubAck(s) => assert_eq!(s.reason_codes, vec![ReasonCode::GrantedQoS1]),
        other => return Err(format!("expected SUBACK, got {other:?}").into()),
    }

    let mut publisher = TcpStream::connect(addr).await?;
    write_packet(&mut publisher, &connect("pub", true, 0)).await?;
    next_packet(&mut publisher).await?; // CONNACK
    write_packet(
        &mut publisher,
        &publish("home/loft/temp", QoS::AtLeastOnce, Some(5), b"21.5"),
    )
    .await?;

    // Publisher receives its PUBACK(5).
    match next_packet(&mut publisher).await? {
        PacketV5::PubAck(a) => assert_eq!(a.packet_id, 5),
        other => return Err(format!("expected PUBACK, got {other:?}").into()),
    }
    // Subscriber receives the forwarded PUBLISH.
    match next_packet(&mut sub).await? {
        PacketV5::Publish(p) => {
            assert_eq!(p.topic, "home/loft/temp");
            assert_eq!(&p.payload[..], b"21.5");
            assert_eq!(p.qos, QoS::AtLeastOnce);
        }
        other => return Err(format!("expected PUBLISH, got {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn tcp_persistent_session_survives_reconnect() -> TestResult {
    let addr = start_tcp().await?;

    // Subscriber: persistent session, then drop the socket.
    {
        let mut sub = TcpStream::connect(addr).await?;
        write_packet(&mut sub, &connect("durable", false, 3600)).await?;
        next_packet(&mut sub).await?;
        write_packet(&mut sub, &subscribe("home/#", QoS::AtLeastOnce, 1)).await?;
        next_packet(&mut sub).await?;
    } // socket dropped → ungraceful disconnect

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Publish while the subscriber is offline.
    let mut publisher = TcpStream::connect(addr).await?;
    write_packet(&mut publisher, &connect("pub", true, 0)).await?;
    next_packet(&mut publisher).await?;
    write_packet(
        &mut publisher,
        &publish("home/x", QoS::AtLeastOnce, Some(9), b"queued"),
    )
    .await?;
    next_packet(&mut publisher).await?; // PUBACK

    // Reconnect: session resumes and the queued message is delivered.
    let mut sub = TcpStream::connect(addr).await?;
    write_packet(&mut sub, &connect("durable", false, 3600)).await?;
    match next_packet(&mut sub).await? {
        PacketV5::ConnAck(ack) => assert!(ack.session_present, "session must resume"),
        other => return Err(format!("expected CONNACK, got {other:?}").into()),
    }
    match next_packet(&mut sub).await? {
        PacketV5::Publish(p) => {
            assert_eq!(p.topic, "home/x");
            assert_eq!(&p.payload[..], b"queued");
        }
        other => return Err(format!("expected queued PUBLISH, got {other:?}").into()),
    }
    Ok(())
}

// ---- TLS -----------------------------------------------------------------

#[tokio::test]
async fn tls_handshake_then_connect() -> TestResult {
    use tokio_rustls::TlsConnector;
    use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    // Self-signed cert for "localhost" via rcgen (offline).
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let acceptor = cave_home_mqtt::runtime::tls::acceptor_from_pem(&cert_pem, &key_pem)?;
    let server = Server::new(allow_all());
    tokio::spawn(async move {
        let _ = server.serve_tls(listener, acceptor).await;
    });

    // Client trusts the self-signed cert.
    let mut roots = RootCertStore::empty();
    roots.add(cert_der)?;
    let client_config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(std::sync::Arc::new(client_config));

    let tcp = TcpStream::connect(addr).await?;
    let domain = ServerName::try_from("localhost")?;
    let mut tls = connector.connect(domain, tcp).await?;

    write_packet(&mut tls, &connect("tls-client", true, 0)).await?;
    match next_packet(&mut tls).await? {
        PacketV5::ConnAck(ack) => assert_eq!(ack.reason_code, ReasonCode::Success),
        other => return Err(format!("expected CONNACK over TLS, got {other:?}").into()),
    }
    Ok(())
}

// ---- WebSocket -----------------------------------------------------------

#[tokio::test]
async fn websocket_connect_is_acknowledged() -> TestResult {
    use cave_home_mqtt::v5::codec::{decode_v5, encode_v5};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = Server::new(allow_all());
    tokio::spawn(async move {
        let _ = server.serve_ws(listener).await;
    });

    let tcp = TcpStream::connect(addr).await?;
    let (mut ws, _resp) = tokio_tungstenite::client_async(format!("ws://{addr}/mqtt"), tcp).await?;

    let frame = encode_v5(&connect("ws-client", true, 0))?;
    ws.send(Message::Binary(frame.to_vec())).await?;

    let msg = ws.next().await.ok_or("the WebSocket ended")??;
    let bytes = match msg {
        Message::Binary(b) => b,
        other => return Err(format!("expected binary WS message, got {other:?}").into()),
    };
    let (packet, _) = decode_v5(&bytes)?;
    match packet {
        PacketV5::ConnAck(ack) => assert_eq!(ack.reason_code, ReasonCode::Success),
        other => return Err(format!("expected CONNACK over WS, got {other:?}").into()),
    }
    Ok(())
}
