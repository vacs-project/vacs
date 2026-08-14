use crate::error::{SignalingError, SignalingRuntimeError, TransportFailureReason};
use crate::transport::{SignalingReceiver, SignalingSender, SignalingTransport};
use async_trait::async_trait;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite};
use tokio_util::sync::CancellationToken;
use vacs_protocol::ws::server::ServerMessage;

const HEARTBEAT_PING_INTERVAL: Duration = Duration::from_secs(15);
const HEARTBEAT_PONG_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct TokioTransport {
    url: String,
}

impl TokioTransport {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

#[async_trait]
impl SignalingTransport for TokioTransport {
    type Sender = TokioSender;
    type Receiver = TokioReceiver;

    #[tracing::instrument(level = "info", err)]
    async fn connect(&self) -> Result<(Self::Sender, Self::Receiver), SignalingError> {
        let (websocket_stream, response) = tokio_tungstenite::connect_async(&self.url)
            .await
            .map_err(|err| {
                tracing::error!(?err, "Failed to connect to signaling server");
                SignalingError::Transport(err.into())
            })?;
        tracing::debug!(?response, "WebSocket handshake response");

        let (websocket_tx, websocket_rx) = websocket_stream.split();

        Ok((
            TokioSender::new(websocket_tx),
            TokioReceiver::new(websocket_rx),
        ))
    }
}

pub struct TokioSender {
    websocket_tx: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
}

pub struct TokioReceiver {
    websocket_rx: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    cancel: CancellationToken,
    heartbeat_state: Arc<HeartbeatState>,
    heartbeat_handle: Option<JoinHandle<()>>,
}

#[async_trait]
impl SignalingSender for TokioSender {
    #[tracing::instrument(level = "debug", skip(self, msg), err)]
    async fn send(&mut self, msg: tungstenite::Message) -> Result<(), SignalingRuntimeError> {
        if !matches!(
            msg,
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)
        ) {
            tracing::trace!("Sending message to server");
        }
        self.websocket_tx.send(msg).await.map_err(|err| {
            tracing::warn!(?err, "Failed to send message");
            SignalingRuntimeError::Transport(TransportFailureReason::Send(err.to_string()))
        })?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self), err)]
    async fn close(&mut self) -> Result<(), SignalingRuntimeError> {
        let _ = self
            .websocket_tx
            .send(tungstenite::Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "".into(),
            })))
            .await
            .inspect_err(|err| {
                tracing::warn!(?err, "Failed to send Close frame");
            });

        self.websocket_tx.close().await.map_err(|err| {
            tracing::warn!(?err, "Failed to close WebSocket connection");
            SignalingRuntimeError::Transport(TransportFailureReason::Close(err.to_string()))
        })
    }
}

#[async_trait]
impl SignalingReceiver for TokioReceiver {
    #[tracing::instrument(level = "debug", skip(self, send_tx), err)]
    async fn recv(
        &mut self,
        send_tx: &mpsc::Sender<tungstenite::Message>,
    ) -> Result<ServerMessage, SignalingRuntimeError> {
        if self.heartbeat_handle.is_none() {
            self.spawn_heartbeat(send_tx);
        }

        loop {
            tokio::select! {
                _ = self.heartbeat_state.disconnected.notified() => {
                    tracing::warn!("Disconnecting due to heartbeat timeout");
                    return Err(SignalingRuntimeError::Disconnected(None));
                }
                msg = self.websocket_rx.next() => {
                    let Some(msg) = msg else { break; };
                    match msg {
                        Ok(tungstenite::Message::Text(text)) => {
                            self.heartbeat_state.mark_rx();
                            return match ServerMessage::deserialize(&text) {
                                Ok(ServerMessage::Disconnected(disconnected)) => {
                                    tracing::debug!(
                                        reason = ?disconnected.reason,
                                        "Received Disconnected message, returning disconnected error"
                                    );
                                    Err(SignalingRuntimeError::Disconnected(Some(disconnected.reason)))
                                }
                                Ok(msg) => Ok(msg),
                                Err(err) => {
                                    tracing::warn!(?err, "Failed to deserialize message");
                                    Err(SignalingRuntimeError::SerializationError(err.to_string()))
                                }
                            };
                        }
                        Ok(tungstenite::Message::Close(reason)) => {
                            tracing::warn!(?reason, "Received Close WebSocket frame");
                            return Err(SignalingRuntimeError::Disconnected(None));
                        }
                        Ok(tungstenite::Message::Ping(data)) => {
                            self.heartbeat_state.mark_rx();
                            if let Err(err) = send_tx.send(tungstenite::Message::Pong(data)).await {
                                tracing::warn!(?err, "Failed to send tokio Pong");
                                return Err(SignalingRuntimeError::Disconnected(None));
                            }
                        }
                        Ok(tungstenite::Message::Pong(_)) => {
                            self.heartbeat_state.mark_pong();
                        }
                        Ok(other) => {
                            tracing::debug!(?other, "Skipping non-text WebSocket frame");
                        }
                        Err(err) => {
                            tracing::warn!(?err, "Failed to receive message");
                            return Err(SignalingRuntimeError::Transport(
                                TransportFailureReason::Receive(err.to_string()),
                            ));
                        }
                    }
                }
            }
        }
        tracing::warn!("WebSocket stream closed");
        Err(SignalingRuntimeError::Disconnected(None))
    }
}

impl TokioSender {
    fn new(
        websocket_tx: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    ) -> Self {
        Self { websocket_tx }
    }
}

impl TokioReceiver {
    fn new(websocket_rx: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) -> Self {
        Self {
            websocket_rx,
            cancel: CancellationToken::new(),
            heartbeat_state: HeartbeatState::new(),
            heartbeat_handle: None,
        }
    }

    fn spawn_heartbeat(&mut self, send_tx: &mpsc::Sender<tungstenite::Message>) {
        if self.heartbeat_handle.is_some() {
            return;
        }

        let heartbeat_state = self.heartbeat_state.clone();
        let mut pong_rx = self.heartbeat_state.pong_rx.clone();
        let send_tx = send_tx.clone();
        let cancel = self.cancel.clone();
        self.heartbeat_handle = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(HEARTBEAT_PING_INTERVAL);
            let mut last_tick = Instant::now();

            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        tracing::trace!("Cancelling heartbeat task");
                        break;
                    }
                    _ = ticker.tick() => {
                        let now = Instant::now();
                        let delta = now.duration_since(last_tick);
                        last_tick = now;
                        if delta > HEARTBEAT_PING_INTERVAL * 3 {
                            tracing::warn!(?delta, "Long pause between heartbeat pings detected, assuming system sleep or interruption, forcing reconnect");
                            heartbeat_state.disconnected.notify_one();
                            break;
                        }

                        if heartbeat_state.last_rx().elapsed() < HEARTBEAT_PING_INTERVAL / 2 {
                            continue;
                        }

                        if let Err(err) = send_tx.send(tungstenite::Message::Ping(tungstenite::Bytes::from_static(b""))).await {
                            tracing::warn!(?err, "Failed to send heartbeat ping");
                            heartbeat_state.disconnected.notify_one();
                            break;
                        }

                        let before = *pong_rx.borrow();
                        if match tokio::time::timeout(HEARTBEAT_PONG_TIMEOUT, pong_rx.changed()).await {
                            Ok(Ok(_)) => *pong_rx.borrow() == before,
                            _ => true,
                        } {
                            tracing::warn!("Heartbeat timeout");
                            heartbeat_state.disconnected.notify_one();
                            break;
                        }
                    }
                }
            }
            tracing::trace!("Heartbeat task finished");
        }));
    }
}

impl Drop for TokioReceiver {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
        }
    }
}

struct HeartbeatState {
    last_rx: RwLock<Instant>,
    pong_tx: watch::Sender<Instant>,
    pong_rx: watch::Receiver<Instant>,
    disconnected: Notify,
}

impl HeartbeatState {
    fn new() -> Arc<Self> {
        let now = Instant::now();
        let (pong_tx, pong_rx) = watch::channel(now);
        Arc::new(Self {
            last_rx: RwLock::new(now),
            pong_tx,
            pong_rx,
            disconnected: Notify::new(),
        })
    }

    fn mark_rx(&self) {
        *self.last_rx.write() = Instant::now();
    }

    fn mark_pong(&self) {
        let now = Instant::now();
        let _ = self.pong_tx.send(now);
        *self.last_rx.write() = now;
    }

    fn last_rx(&self) -> Instant {
        *self.last_rx.read()
    }
}
