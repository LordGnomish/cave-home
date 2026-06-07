// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! The async SysAP client: real REST + WebSocket I/O over the tested cores.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthMethod;
    use crate::config::ClientConfig;
    use crate::event::FreeAtHomeEvent;
    use futures_util::stream;
    use tokio_tungstenite::tungstenite::Message;

    fn config() -> ClientConfig {
        ClientConfig::new("192.168.1.10", AuthMethod::basic("user", "pass"))
    }

    #[test]
    fn client_builds_from_config() {
        let client = FreeAtHomeClient::new(config()).expect("client");
        assert_eq!(client.config().host(), "192.168.1.10");
    }

    #[test]
    fn client_exposes_authorization_header() {
        let client = FreeAtHomeClient::new(config()).expect("client");
        assert_eq!(
            client.authorization_header(),
            Some("Basic dXNlcjpwYXNz".to_string())
        );
    }

    #[tokio::test]
    async fn event_loop_dispatches_parsed_events() {
        let frame = r#"{ "u": { "datapoints": { "ABB700C12345/ch0000/odp0000": "1" } } }"#;
        let messages = vec![
            Ok(Message::Text(frame.to_string())),
            Ok(Message::Close(None)),
        ];
        let s = stream::iter(messages);
        let mut got = Vec::new();
        run_event_loop(s, |ev| got.push(ev)).await.expect("loop ok");
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], FreeAtHomeEvent::DatapointUpdate(_)));
    }

    #[tokio::test]
    async fn event_loop_ignores_non_text_frames() {
        let messages = vec![
            Ok(Message::Ping(Vec::new())),
            Ok(Message::Close(None)),
        ];
        let s = stream::iter(messages);
        let mut count = 0usize;
        run_event_loop(s, |_| count += 1).await.expect("loop ok");
        assert_eq!(count, 0);
    }
}
