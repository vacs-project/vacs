use crate::metrics::MessageMetrics;
use crate::ws::traits::{WebSocketSink, WebSocketStream};
use axum::extract::ws;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::ServerMessage;

/// Represents the outcome of [`receive_message`], indicating whether the message received should be handled, skipped, or receiving errored.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // short-lived, consumed immediately; boxing would allocate per message
pub enum MessageResult {
    /// A valid application-message that can be processed.
    ApplicationMessage(ClientMessage),
    /// A control message (e.g., Ping, Pong) that should be skipped.
    ControlMessage,
    /// The client has disconnected.
    Disconnected,
    /// An error occurred while receiving the message.
    Error(anyhow::Error),
}

impl PartialEq for MessageResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (MessageResult::ApplicationMessage(a), MessageResult::ApplicationMessage(b)) => a == b,
            (MessageResult::ControlMessage, MessageResult::ControlMessage) => true,
            (MessageResult::Disconnected, MessageResult::Disconnected) => true,
            (MessageResult::Error(self_err), MessageResult::Error(other_err)) => {
                self_err.to_string() == other_err.to_string()
            }
            _ => false,
        }
    }
}

pub async fn send_message(
    ws_outbound_tx: &mpsc::Sender<ws::Message>,
    message: impl Into<ServerMessage>,
) -> anyhow::Result<()> {
    let message = message.into();
    let serialized_message = ServerMessage::serialize(&message)
        .map_err(|e| anyhow::anyhow!(e).context("Failed to serialize message"))?;
    MessageMetrics::sent(&message, serialized_message.len());
    ws_outbound_tx
        .send(ws::Message::from(serialized_message))
        .await
        .map_err(|e| anyhow::anyhow!(e).context("Failed to send message"))?;
    Ok(())
}

pub async fn send_message_raw<T: WebSocketSink>(
    websocket_tx: &mut T,
    message: impl Into<ServerMessage>,
) -> anyhow::Result<()> {
    let message = message.into();
    let serialized_message = ServerMessage::serialize(&message)
        .map_err(|e| anyhow::anyhow!(e).context("Failed to serialize message"))?;
    MessageMetrics::sent(&message, serialized_message.len());
    websocket_tx
        .send(ws::Message::from(serialized_message))
        .await
        .map_err(|e| anyhow::anyhow!(e).context("Failed to send message"))?;
    Ok(())
}

pub async fn receive_message<R: WebSocketStream>(websocket_rx: &mut R) -> MessageResult {
    match websocket_rx.next().await {
        Some(Ok(ws::Message::Text(raw_message))) => {
            match ClientMessage::deserialize(&raw_message) {
                Ok(message) => {
                    MessageMetrics::received(&message, raw_message.len());
                    MessageResult::ApplicationMessage(message)
                }
                Err(err) => {
                    MessageMetrics::malformed();
                    MessageResult::Error(
                        anyhow::anyhow!(err).context("Failed to deserialize message"),
                    )
                }
            }
        }
        Some(Ok(ws::Message::Ping(_))) => MessageResult::ControlMessage,
        Some(Ok(ws::Message::Pong(_))) => MessageResult::ControlMessage,
        Some(Ok(ws::Message::Close(reason))) => {
            tracing::debug!(?reason, "Received websocket close message");
            MessageResult::Disconnected
        }
        Some(Ok(other)) => {
            tracing::trace!(?other, "Received unexpected websocket message");
            MessageResult::Error(anyhow::anyhow!("Received unexpected websocket message"))
        }
        Some(Err(err)) => {
            tracing::warn!(?err, "Failed to receive message");
            MessageResult::Error(anyhow::anyhow!(err).context("Failed to receive message"))
        }
        None => {
            tracing::debug!("Client receiver closed, disconnecting");
            MessageResult::Disconnected
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::test_util::*;
    use pretty_assertions::{assert_eq, assert_matches};
    use std::sync::Arc;
    use test_log::test;
    use tokio::sync::{Mutex, mpsc};
    use tokio_tungstenite::tungstenite;
    use uuid::Uuid;
    use vacs_protocol::vatsim::{ClientId, PositionId};
    use vacs_protocol::ws::server::{self, ClientInfo, ServerMessage};
    use vacs_protocol::ws::shared::CallId;

    #[test(tokio::test)]
    async fn send_single_message_raw() {
        let (tx, mut rx) = mpsc::channel(100);
        let mut mock_sink = MockSink::new(tx);

        let message = ServerMessage::ClientConnected(server::ClientConnected {
            client: ClientInfo {
                id: ClientId::from("client1"),
                position_id: Some(PositionId::from("position1")),
                display_name: "Client 1".to_string(),
                frequency: "100.000".to_string(),
            },
        });

        assert!(
            send_message_raw(&mut mock_sink, message.clone())
                .await
                .is_ok()
        );

        if let Some(sent_message) = rx.recv().await {
            if let ws::Message::Text(serialized_message) = sent_message {
                let deserialized_message = ServerMessage::deserialize(&serialized_message)
                    .expect("Failed to deserialize message");
                assert_eq!(deserialized_message, message);
            } else {
                panic!("Expected a Text message, got: {:?}", sent_message);
            }
        } else {
            panic!("No message received");
        }
    }

    #[test(tokio::test)]
    async fn send_multiple_messages_raw() {
        let (tx, mut rx) = mpsc::channel(100);
        let mut mock_sink = MockSink::new(tx);

        let messages = vec![
            ServerMessage::ClientList(server::ClientList { clients: vec![] }),
            ServerMessage::StationList(server::StationList { stations: vec![] }),
            ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }),
        ];
        for message in &messages {
            assert!(
                send_message_raw(&mut mock_sink, message.clone())
                    .await
                    .is_ok()
            );
        }

        for expected in messages {
            let sent = rx.recv().await.expect("No message received");
            match sent {
                ws::Message::Text(raw_message) => {
                    let message = ServerMessage::deserialize(&raw_message)
                        .expect("Failed to deserialize message");
                    assert_eq!(message, expected);
                }
                _ => panic!("Expected a Text message, got: {:?}", sent),
            }
        }
    }

    #[test(tokio::test)]
    async fn send_messages_concurrently_raw() {
        let (tx, mut rx) = mpsc::channel(100);
        let mock_sink = Arc::new(Mutex::new(MockSink::new(tx)));

        let messages = vec![
            ServerMessage::ClientList(server::ClientList { clients: vec![] }),
            ServerMessage::StationList(server::StationList { stations: vec![] }),
            ServerMessage::Disconnected(server::Disconnected {
                reason: server::DisconnectReason::Terminated,
            }),
        ];

        let mut tasks = vec![];
        for message in &messages {
            let mock_sink = mock_sink.clone();
            let message = message.clone();
            let task = tokio::spawn(async move {
                let mut mock_sink = mock_sink.lock().await;
                send_message_raw(&mut *mock_sink, message.clone()).await
            });
            tasks.push(task);
        }

        let results = futures_util::future::join_all(tasks).await;

        for result in results {
            assert!(result.unwrap().is_ok(), "Sending message failed");
        }

        let mut sent = vec![];
        for _ in 0..messages.len() {
            let msg = rx.recv().await.expect("Expected a message");
            if let ws::Message::Text(raw_message) = msg {
                let message = ServerMessage::deserialize(&raw_message)
                    .expect("Failed to deserialize message");
                sent.push(message);
            }
        }

        for expected in &messages {
            assert!(messages.contains(expected));
        }
        assert_eq!(sent.len(), messages.len());
    }

    #[test(tokio::test)]
    async fn send_message_sink_disconnected_raw() {
        let (tx, rx) = mpsc::channel(100);
        drop(rx); // Drop the receiver to simulate the sink being disconnected.
        let mut mock_sink = MockSink::new(tx);

        let message = ServerMessage::ClientConnected(server::ClientConnected {
            client: ClientInfo {
                id: ClientId::from("client1"),
                position_id: Some(PositionId::from("position1")),
                display_name: "Client 1".to_string(),
                frequency: "100.000".to_string(),
            },
        });

        assert!(
            send_message_raw(&mut mock_sink, message.clone())
                .await
                .is_err_and(|err| err.to_string().contains("Failed to send message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_single_message() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::from(
            "{\"type\":\"login\",\"id\":\"client1\",\"token\":\"token1\",\"protocolVersion\":\"0.0.0\",\"customProfile\":false}",
        ))]);

        let result = receive_message(&mut mock_stream).await;

        assert_eq!(
            result,
            MessageResult::ApplicationMessage(ClientMessage::Login(
                vacs_protocol::ws::client::Login {
                    token: "token1".to_string(),
                    protocol_version: "0.0.0".to_string(),
                    custom_profile: false,
                    position_id: None,
                }
            ))
        );
    }

    #[test(tokio::test)]
    async fn receive_multiple_messages() {
        let mut mock_stream = MockStream::new(vec![
            Ok(ws::Message::from(
                "{\"type\":\"login\",\"id\":\"client1\",\"token\":\"token1\",\"protocolVersion\":\"0.0.0\",\"customProfile\":false}",
            )),
            Ok(ws::Message::from("{\"type\":\"logout\"}")),
            Ok(ws::Message::from(
                "{\"type\":\"webrtcOffer\",\"callId\":\"00000000-0000-0000-0000-000000000000\",\"fromClientId\":\"client1\",\"toClientId\":\"client2\",\"sdp\":\"sdp1\"}",
            )),
        ]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ApplicationMessage(ClientMessage::Login(
                vacs_protocol::ws::client::Login {
                    token: "token1".to_string(),
                    protocol_version: "0.0.0".to_string(),
                    custom_profile: false,
                    position_id: None,
                }
            ))
        );
        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ApplicationMessage(ClientMessage::Logout)
        );
        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ApplicationMessage(ClientMessage::WebrtcOffer(
                vacs_protocol::ws::shared::WebrtcOffer {
                    call_id: CallId::from(Uuid::nil()),
                    from_client_id: ClientId::from("client1"),
                    to_client_id: ClientId::from("client2"),
                    sdp: "sdp1".to_string()
                }
            ))
        );
    }

    #[test(tokio::test)]
    async fn receive_messages_concurrently() {
        let mock_stream = Arc::new(Mutex::new(MockStream::new(vec![
            Ok(ws::Message::from(
                "{\"type\":\"login\",\"id\":\"client1\",\"token\":\"token1\",\"protocolVersion\":\"0.0.0\",\"customProfile\":false}",
            )),
            Ok(ws::Message::from("{\"type\":\"logout\"}")),
            Ok(ws::Message::from(
                "{\"type\":\"webrtcOffer\",\"callId\":\"00000000-0000-0000-0000-000000000000\",\"fromClientId\":\"client1\",\"toClientId\":\"client2\",\"sdp\":\"sdp1\"}",
            )),
        ])));

        let mut tasks = vec![];
        for _ in 0..3 {
            let mock_stream = mock_stream.clone();
            let task = tokio::spawn(async move {
                let mut mock_stream = mock_stream.lock().await;
                receive_message(&mut *mock_stream).await
            });
            tasks.push(task);
        }

        let results = futures_util::future::join_all(tasks).await;
        for result in results {
            assert!(result.is_ok(), "Receiving message failed");
            assert_matches!(result.unwrap(), MessageResult::ApplicationMessage(_));
        }
    }

    #[test(tokio::test)]
    async fn receive_replayed_messages() {
        let msg = ws::Message::from(
            "{\"type\":\"login\",\"id\":\"client1\",\"token\":\"token1\",\"protocolVersion\":\"0.0.0\",\"customProfile\":false}",
        );
        let mut mock_stream = MockStream::new(vec![Ok(msg.clone()), Ok(msg)]);

        for _ in 0..2 {
            assert_eq!(
                receive_message(&mut mock_stream).await,
                MessageResult::ApplicationMessage(ClientMessage::Login(
                    vacs_protocol::ws::client::Login {
                        token: "token1".to_string(),
                        protocol_version: "0.0.0".to_string(),
                        custom_profile: false,
                        position_id: None,
                    }
                ))
            );
        }
    }

    #[test(tokio::test)]
    async fn receive_control_messages() {
        let mut mock_stream = MockStream::new(vec![
            Ok(ws::Message::Ping(tungstenite::Bytes::from("ping"))),
            Ok(ws::Message::Pong(tungstenite::Bytes::from("pong"))),
        ]);

        for _ in 0..2 {
            assert_eq!(
                receive_message(&mut mock_stream).await,
                MessageResult::ControlMessage
            );
        }
    }

    #[test(tokio::test)]
    async fn receive_close_message() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::Close(None))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Disconnected
        );
    }

    #[test(tokio::test)]
    async fn receive_close_message_with_close_frame() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::Close(Some(ws::CloseFrame {
            reason: ws::Utf8Bytes::from("goodbye"),
            code: 69,
        })))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Disconnected
        );
    }

    #[test(tokio::test)]
    async fn receive_mixed_messages() {
        let mut mock_stream = MockStream::new(vec![
            Ok(ws::Message::Ping(tungstenite::Bytes::from("ping"))),
            Ok(ws::Message::from("{\"type\":\"logout\"}")),
            Ok(ws::Message::Pong(tungstenite::Bytes::from("pong"))),
        ]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ControlMessage
        );
        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ApplicationMessage(ClientMessage::Logout)
        );
        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::ControlMessage
        );
    }

    #[test(tokio::test)]
    async fn receive_message_deserialization_error() {
        let mut mock_stream =
            MockStream::new(vec![Ok(ws::Message::Text(ws::Utf8Bytes::from("invalid")))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to deserialize message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_message_invalid_json() {
        let mut mock_stream =
            MockStream::new(vec![Ok(ws::Message::Text(ws::Utf8Bytes::from("\"logout")))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to deserialize message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_unknown_message_type() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::Text(ws::Utf8Bytes::from(
            "{\"InvalidMessageType\":{\"unknown_field\":\"value\"}}",
        )))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to deserialize message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_empty_text() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::Text(ws::Utf8Bytes::from("")))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to deserialize message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_message_abrupt_disconnect() {
        let mut mock_stream = MockStream::new(vec![Err(axum::Error::new(tungstenite::Error::Io(
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Abrupt disconnection"),
        )))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to receive message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_unexpected_message() {
        let mut mock_stream = MockStream::new(vec![Ok(ws::Message::Binary(
            tungstenite::Bytes::from("binary"),
        ))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Received unexpected websocket message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_message_socket_error() {
        let mut mock_stream = MockStream::new(vec![Err(axum::Error::new(
            tungstenite::Error::ConnectionClosed,
        ))]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Error(anyhow::anyhow!("Failed to receive message"))
        );
    }

    #[test(tokio::test)]
    async fn receive_message_stream_end() {
        let mut mock_stream = MockStream::new(vec![]);

        assert_eq!(
            receive_message(&mut mock_stream).await,
            MessageResult::Disconnected
        );
    }
}
