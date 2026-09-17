use crate::test_utils::connect_to_websocket;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use vacs_protocol::VACS_PROTOCOL_VERSION;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{self, ClientInfo, ServerMessage, StationInfo};

pub struct TestClient {
    id: ClientId,
    token: String,
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl TestClient {
    pub async fn new(ws_addr: &str, id: impl Into<ClientId>, token: &str) -> anyhow::Result<Self> {
        let ws_stream = connect_to_websocket(ws_addr).await;
        Ok(Self {
            id: id.into(),
            token: token.to_string(),
            ws_stream,
        })
    }

    pub async fn new_with_login<FI, FC, FS>(
        ws_addr: &str,
        id: impl Into<ClientId>,
        token: &str,
        client_info_predicate: FI,
        client_list_predicate: FC,
        station_list_predicate: FS,
    ) -> anyhow::Result<Self>
    where
        FI: FnOnce(bool, ClientInfo) -> anyhow::Result<()>,
        FC: FnOnce(&[ClientInfo]) -> anyhow::Result<()> + Copy,
        FS: FnOnce(&[StationInfo]) -> anyhow::Result<()> + Copy,
    {
        let mut client = Self::new(ws_addr, id, token).await?;
        client
            .login(
                client_info_predicate,
                client_list_predicate,
                station_list_predicate,
            )
            .await?;
        Ok(client)
    }

    pub fn id(&self) -> &ClientId {
        &self.id
    }

    pub async fn login<FI, FC, FS>(
        &mut self,
        client_info_predicate: FI,
        client_list_predicate: FC,
        station_list_predicate: FS,
    ) -> anyhow::Result<()>
    where
        FI: FnOnce(bool, ClientInfo) -> anyhow::Result<()>,
        FC: FnOnce(&[ClientInfo]) -> anyhow::Result<()> + Copy,
        FS: FnOnce(&[StationInfo]) -> anyhow::Result<()> + Copy,
    {
        let login_msg = ClientMessage::Login(vacs_protocol::ws::client::Login {
            token: self.token.to_string(),
            protocol_version: VACS_PROTOCOL_VERSION.to_string(),
            custom_profile: false,
            position_id: None,
        });
        self.send_and_expect_with_timeout(login_msg, Duration::from_millis(100), |msg| match msg {
            ServerMessage::SessionInfo(server::SessionInfo { client, .. }) => {
                client_info_predicate(true, client)
            }
            ServerMessage::LoginFailure(server::LoginFailure { reason }) => {
                Err(anyhow::anyhow!("Login failed: {:?}", reason))
            }
            _ => Err(anyhow::anyhow!("Unexpected response: {:?}", msg)),
        })
        .await?;

        self.recv_with_timeout_and_filter(Duration::from_millis(100), |msg| {
            matches!(msg, ServerMessage::ClientList(server::ClientList { clients }) if client_list_predicate(clients).is_ok())
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("Client list not received"))?;

        self.recv_with_timeout_and_filter(Duration::from_millis(100), |msg| {
            matches!(msg, ServerMessage::StationList(server::StationList { stations }) if station_list_predicate(stations).is_ok())
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("Station list not received"))?;

        Ok(())
    }

    pub async fn send_raw(&mut self, msg: Message) -> anyhow::Result<()> {
        self.ws_stream.send(msg).await?;
        Ok(())
    }

    pub async fn send(&mut self, msg: ClientMessage) -> anyhow::Result<()> {
        self.ws_stream
            .send(Message::from(ClientMessage::serialize(&msg)?))
            .await?;
        Ok(())
    }

    pub async fn recv_raw_with_timeout(&mut self, timeout: Duration) -> Option<Message> {
        loop {
            match tokio::time::timeout(timeout, self.ws_stream.next()).await {
                Ok(Some(Ok(Message::Ping(_)))) => continue,
                Ok(Some(Ok(message))) => return Some(message),
                _ => return None,
            }
        }
    }

    pub async fn recv_raw_until_timeout(&mut self, timeout: Duration) -> Vec<Message> {
        let mut messages = Vec::new();
        while let Some(message) = self.recv_raw_with_timeout(timeout).await {
            messages.push(message);
        }
        messages
    }

    pub async fn recv_raw(&mut self) -> Option<Message> {
        self.recv_raw_with_timeout(Duration::MAX).await
    }

    pub async fn recv_with_timeout(&mut self, timeout: Duration) -> Option<ServerMessage> {
        loop {
            match self.recv_raw_with_timeout(timeout).await {
                Some(Message::Text(text)) => return ServerMessage::deserialize(&text).ok(),
                Some(Message::Ping(_)) => continue,
                _ => return None,
            }
        }
    }

    pub async fn recv_with_timeout_and_filter<F>(
        &mut self,
        timeout: Duration,
        predicate: F,
    ) -> Option<ServerMessage>
    where
        F: Fn(&ServerMessage) -> bool,
    {
        while let Some(message) = self.recv_with_timeout(timeout).await {
            if predicate(&message) {
                return Some(message);
            }
        }
        None
    }

    pub async fn recv_until_timeout(&mut self, timeout: Duration) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Some(message) = self.recv_with_timeout(timeout).await {
            messages.push(message);
        }
        messages
    }

    pub async fn recv_until_timeout_with_filter<F>(
        &mut self,
        timeout: Duration,
        predicate: F,
    ) -> Vec<ServerMessage>
    where
        F: Fn(&ServerMessage) -> bool,
    {
        let mut messages = Vec::new();
        while let Some(message) = self.recv_with_timeout(timeout).await {
            if predicate(&message) {
                messages.push(message);
            }
        }
        messages
    }

    pub async fn recv(&mut self) -> Option<ServerMessage> {
        self.recv_with_timeout(Duration::MAX).await
    }

    pub async fn send_raw_and_expect_with_timeout<F>(
        &mut self,
        msg: Message,
        timeout: Duration,
        predicate: F,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(Message),
    {
        self.send_raw(msg).await?;
        match self.recv_raw_with_timeout(timeout).await {
            Some(response) => predicate(response),
            None => anyhow::bail!("No response received"),
        }
        Ok(())
    }

    pub async fn send_raw_and_expect<F>(&mut self, msg: Message, predicate: F) -> anyhow::Result<()>
    where
        F: FnOnce(Message),
    {
        self.send_raw_and_expect_with_timeout(msg, Duration::MAX, predicate)
            .await
    }

    pub async fn send_and_expect_with_timeout<F>(
        &mut self,
        msg: ClientMessage,
        timeout: Duration,
        predicate: F,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(ServerMessage) -> anyhow::Result<()>,
    {
        self.send(msg).await?;
        match self.recv_with_timeout(timeout).await {
            Some(response) => predicate(response),
            None => anyhow::bail!("No response received"),
        }
    }

    pub async fn send_and_expect<F>(
        &mut self,
        msg: ClientMessage,
        predicate: F,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(ServerMessage) -> anyhow::Result<()>,
    {
        self.send_and_expect_with_timeout(msg, Duration::MAX, predicate)
            .await
    }

    pub async fn close(&mut self) {
        self.ws_stream
            .close(None)
            .await
            .expect("Failed to close websocket");
    }
}
