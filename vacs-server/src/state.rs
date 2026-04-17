pub mod calls;
pub mod clients;

use crate::config;
use crate::config::AppConfig;
use crate::dataset::DatasetManager;
use crate::ice::provider::IceConfigProvider;
use crate::metrics::guards::ClientConnectionGuard;
use crate::metrics::{ErrorMetrics, VatsimSyncMetrics};
use crate::ratelimit::RateLimiters;
use crate::release::UpdateChecker;
use crate::state::calls::CallManager;
use crate::state::clients::{ClientManager, ClientSession};
use crate::store::{Store, StoreBackend};
use anyhow::Context;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{Instrument, instrument};
use uuid::Uuid;
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId};
use vacs_protocol::ws::server::{ClientInfo, DisconnectReason, ServerMessage, StationInfo};
use vacs_protocol::ws::shared::{Error, ErrorReason};
use vacs_vatsim::ControllerInfo;
use vacs_vatsim::coverage::network::Network;
use vacs_vatsim::data_feed::DataFeed;
use vacs_vatsim::slurper::SlurperClient;

pub struct AppState {
    pub config: AppConfig,
    pub updates: UpdateChecker,
    pub calls: CallManager,
    pub clients: ClientManager,
    pub dataset: Option<DatasetManager>,
    pub ice_config_provider: Arc<dyn IceConfigProvider>,
    store: Store,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    slurper: SlurperClient,
    data_feed: Arc<dyn DataFeed>,
    rate_limiters: RateLimiters,
    shutdown_rx: watch::Receiver<()>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AppConfig,
        updates: UpdateChecker,
        store: Store,
        slurper: SlurperClient,
        data_feed: Arc<dyn DataFeed>,
        network: Network,
        rate_limiters: RateLimiters,
        shutdown_rx: watch::Receiver<()>,
        ice_config_provider: Arc<dyn IceConfigProvider>,
        dataset: Option<DatasetManager>,
    ) -> Self {
        let (broadcast_tx, _) = broadcast::channel(config::BROADCAST_CHANNEL_CAPACITY);
        Self {
            config,
            updates,
            ice_config_provider,
            store,
            calls: CallManager::new(),
            clients: ClientManager::new(broadcast_tx.clone(), network, data_feed.clone()),
            dataset,
            broadcast_tx,
            slurper,
            data_feed,
            rate_limiters,
            shutdown_rx,
        }
    }

    pub fn get_client_receivers(
        &self,
    ) -> (broadcast::Receiver<ServerMessage>, watch::Receiver<()>) {
        (self.broadcast_tx.subscribe(), self.shutdown_rx.clone())
    }

    #[instrument(level = "debug", skip(self, client_connection_guard), err)]
    pub async fn register_client(
        &self,
        client_info: ClientInfo,
        active_profile: ActiveProfile<ProfileId>,
        client_connection_guard: ClientConnectionGuard,
    ) -> anyhow::Result<(ClientSession, mpsc::Receiver<ServerMessage>)> {
        tracing::trace!("Registering client");

        if self.clients.is_empty().await {
            tracing::debug!("First client connected, triggering initial VATSIM controller sync");
            if let Err(err) = self
                .update_vatsim_controllers(&mut HashSet::new(), false)
                .await
            {
                tracing::warn!(?err, "Initial VATSIM controller sync failed");
            }
        }

        let (client, rx) = self
            .clients
            .add_client(client_info, active_profile, client_connection_guard)
            .await?;

        tracing::trace!("Client registered");
        Ok((client, rx))
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn unregister_client(
        &self,
        client_id: &ClientId,
        disconnect_reason: Option<DisconnectReason>,
    ) {
        tracing::trace!("Unregistering client");

        self.clients
            .remove_client(client_id.clone(), disconnect_reason)
            .await;

        self.calls.cleanup_client_calls(self, client_id).await;

        tracing::debug!("Client unregistered");
    }

    pub async fn list_clients(&self, self_client_id: Option<&ClientId>) -> Vec<ClientInfo> {
        self.clients.list_clients(self_client_id).await
    }

    pub async fn list_stations(
        &self,
        active_profile: &ActiveProfile<ProfileId>,
        self_position_id: Option<&PositionId>,
    ) -> Vec<StationInfo> {
        self.clients
            .list_stations(active_profile, self_position_id)
            .await
    }

    pub async fn get_client(&self, client_id: &ClientId) -> Option<ClientSession> {
        self.clients.get_client(client_id).await
    }

    #[tracing::instrument(level = "trace", skip(self, message))]
    pub async fn send_message(
        &self,
        client_id: &ClientId,
        message: impl Into<ServerMessage>,
    ) -> Result<(), Error> {
        match self.get_client(client_id).await {
            Some(client) => {
                tracing::trace!("Sending message to client");
                if let Err(err) = client.send_message(message).await {
                    tracing::warn!(?err, "Failed to send message to client");
                    ErrorMetrics::error(&ErrorReason::PeerConnection);
                    Err(Error::new(ErrorReason::PeerConnection).with_client_id(client_id.clone()))
                } else {
                    Ok(())
                }
            }
            None => {
                tracing::warn!("Client not found");
                ErrorMetrics::peer_not_found();
                Err(Error::new(ErrorReason::ClientNotFound).with_client_id(client_id.clone()))
            }
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn generate_ws_auth_token(&self, cid: &str) -> anyhow::Result<String> {
        tracing::debug!("Generating web socket auth token");

        let token = Uuid::now_v7().to_string();

        tracing::trace!("Storing web socket auth token");
        self.store
            .set(
                format!("ws.token.{token}").as_str(),
                cid,
                Some(Duration::from_secs(30)),
            )
            .await
            .context("Failed to store web socket auth token")?;

        tracing::debug!("Web socket auth token generated");
        Ok(token)
    }

    #[instrument(level = "debug", skip_all, err)]
    pub async fn verify_ws_auth_token(&self, token: &str) -> anyhow::Result<ClientId> {
        tracing::debug!("Verifying web socket auth token");

        match self.store.get(format!("ws.token.{token}").as_str()).await {
            Ok(Some(cid)) => {
                tracing::debug!(?cid, "Web socket auth token verified");
                Ok(cid)
            }
            Ok(None) => anyhow::bail!("Web socket auth token not found"),
            Err(err) => anyhow::bail!(err),
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn generate_api_token(&self, cid: &str) -> anyhow::Result<String> {
        tracing::debug!("Generating API token");

        let token = Uuid::now_v7().to_string();
        let expiry = Duration::from_secs(self.config.auth.api_token.expiry_secs);

        self.store
            .set(format!("api.token.{token}").as_str(), cid, Some(expiry))
            .await
            .context("Failed to store API token")?;

        tracing::debug!("API token generated");
        Ok(token)
    }

    #[instrument(level = "debug", skip_all, err)]
    pub async fn verify_api_token(&self, token: &str) -> anyhow::Result<Option<ClientId>> {
        tracing::debug!("Verifying API token");

        if Uuid::try_parse(token).is_err() {
            tracing::debug!("Rejected malformed API token");
            return Ok(None);
        }

        let key = format!("api.token.{token}");
        match self.store.get(key.as_str()).await {
            Ok(Some(cid)) => {
                tracing::debug!(?cid, "API token verified");
                // extend TTL on each use (inactivity-based expiry, like sessions)
                let expiry = Duration::from_secs(self.config.auth.api_token.expiry_secs);
                if let Err(err) = self.store.expire(&key, expiry).await {
                    tracing::warn!(?err, "Failed to extend API token TTL");
                }
                Ok(Some(cid))
            }
            Ok(None) => Ok(None),
            Err(err) => anyhow::bail!(err),
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn revoke_api_token(&self, token: &str) -> anyhow::Result<()> {
        tracing::debug!("Revoking API token");
        self.store
            .remove(format!("api.token.{token}").as_str())
            .await
            .context("Failed to revoke API token")
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn get_vatsim_controller_info(
        &self,
        cid: &ClientId,
    ) -> anyhow::Result<Option<ControllerInfo>> {
        tracing::debug!("Retrieving connection info from VATSIM slurper");
        self.slurper
            .get_controller_info(cid)
            .await
            .map_err(Into::into)
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn get_vatsim_controllers(&self) -> anyhow::Result<Vec<ControllerInfo>> {
        tracing::debug!("Retrieving controller info from VATSIM data feed");
        self.data_feed
            .fetch_controller_info()
            .await
            .map_err(Into::into)
    }

    #[instrument(level = "debug", skip(state))]
    pub fn start_controller_update_task(
        state: Arc<AppState>,
        interval: Duration,
    ) -> JoinHandle<()> {
        tokio::spawn(
            async move {
                let mut ticker = time::interval(interval);
                ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

                let mut shutdown = state.shutdown_rx.clone();
                let mut pending_disconnect = HashSet::new();
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown.changed() => {
                            tracing::info!("Shutting down controller update task");
                            break;
                        }
                        _ = ticker.tick() => {
                            if state.clients.is_empty().await {
                                tracing::trace!("No clients connected, skipping controller update");
                                continue;
                            }

                            if let Err(err) = state.update_vatsim_controllers(&mut pending_disconnect, state.config.vatsim.require_active_connection).await {
                                tracing::warn!(?err, "Failed to update controller info");
                            }
                        }
                    }
                }
            }
            .in_current_span(),
        )
    }

    pub async fn force_update_controllers(&self) -> anyhow::Result<()> {
        self.update_vatsim_controllers(
            &mut HashSet::new(),
            self.config.vatsim.require_active_connection,
        )
        .await
    }

    #[tracing::instrument(level = "debug", skip(self, pending_disconnect), fields(pending_disconnect = pending_disconnect.len()), err)]
    async fn update_vatsim_controllers(
        &self,
        pending_disconnect: &mut HashSet<ClientId>,
        require_active_connection: bool,
    ) -> anyhow::Result<()> {
        tracing::debug!("Updating VATSIM controllers");

        let start = std::time::Instant::now();
        let controllers = self.get_vatsim_controllers().await?;
        let fetch_elapsed = start.elapsed();
        VatsimSyncMetrics::sync_phase("fetch", fetch_elapsed.as_secs_f64());
        tracing::trace!(elapsed = ?fetch_elapsed, "Finished retrieving VATSIM controllers");

        let start_sync = std::time::Instant::now();
        let current: HashMap<ClientId, ControllerInfo> = controllers
            .into_iter()
            .filter(|c| !c.callsign.ends_with("_SUP"))
            .map(|c| (c.cid.clone(), c))
            .collect();

        VatsimSyncMetrics::set_controllers_seen(current.len());

        let disconnected_clients = self
            .clients
            .sync_vatsim_state(&current, pending_disconnect, require_active_connection)
            .await;
        let sync_elapsed = start_sync.elapsed();
        VatsimSyncMetrics::sync_phase("sync", sync_elapsed.as_secs_f64());
        tracing::trace!(elapsed = ?sync_elapsed, "Finished syncing VATSIM state");

        let start_unregister = std::time::Instant::now();
        for (cid, disconnect_reason) in disconnected_clients {
            self.unregister_client(&cid, Some(disconnect_reason)).await;
        }
        let unregister_elapsed = start_unregister.elapsed();
        VatsimSyncMetrics::sync_phase("unregister", unregister_elapsed.as_secs_f64());
        tracing::trace!(elapsed = ?unregister_elapsed, "Finished unregistering clients");

        let total_elapsed = start.elapsed();
        VatsimSyncMetrics::sync_completed(total_elapsed.as_secs_f64());
        tracing::debug!(elapsed = ?total_elapsed, "Finished updating VATSIM controllers");
        Ok(())
    }

    pub async fn health_check(&self) -> anyhow::Result<()> {
        self.store.is_healthy().await
    }

    pub fn rate_limiters(&self) -> &RateLimiters {
        &self.rate_limiters
    }

    pub async fn replace_network(&self, network: Network) {
        self.clients.replace_network(network).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ice::provider::stun::StunOnlyProvider;
    use crate::release::UpdateChecker;
    use crate::store::Store;
    use crate::store::memory::MemoryStore;
    use vacs_vatsim::coverage::network::Network;
    use vacs_vatsim::data_feed::mock::MockDataFeed;
    use vacs_vatsim::slurper::SlurperClient;

    fn test_state() -> Arc<AppState> {
        let (_, shutdown_rx) = watch::channel(());
        Arc::new(AppState::new(
            AppConfig::default(),
            UpdateChecker::default(),
            Store::Memory(MemoryStore::default()),
            SlurperClient::new("http://localhost:12345").unwrap(),
            Arc::new(MockDataFeed::default()),
            Network::default(),
            RateLimiters::default(),
            shutdown_rx,
            Arc::new(StunOnlyProvider::default()),
            None,
        ))
    }

    #[tokio::test]
    async fn verify_api_token_returns_cid() {
        let state = test_state();
        let token = state.generate_api_token("123456").await.unwrap();
        let cid = state.verify_api_token(&token).await.unwrap();
        assert_eq!(cid, Some(ClientId::from("123456")));
    }

    #[tokio::test]
    async fn verify_api_token_rejects_malformed_token() {
        let state = test_state();
        let result = state.verify_api_token("not-a-uuid").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn verify_api_token_returns_none_for_unknown() {
        let state = test_state();
        // Valid UUID format but not stored
        let result = state
            .verify_api_token("01234567-89ab-7def-8000-000000000099")
            .await
            .unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn revoke_api_token_removes_it() {
        let state = test_state();
        let token = state.generate_api_token("123456").await.unwrap();

        state.revoke_api_token(&token).await.unwrap();

        let result = state.verify_api_token(&token).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn revoke_nonexistent_token_succeeds() {
        let state = test_state();
        state.revoke_api_token("does-not-exist").await.unwrap();
    }

    #[tokio::test]
    async fn multiple_tokens_for_same_cid() {
        let state = test_state();
        let token1 = state.generate_api_token("123456").await.unwrap();
        let token2 = state.generate_api_token("123456").await.unwrap();

        assert_ne!(token1, token2);

        let cid1 = state.verify_api_token(&token1).await.unwrap();
        let cid2 = state.verify_api_token(&token2).await.unwrap();
        assert_eq!(cid1, Some(ClientId::from("123456")));
        assert_eq!(cid2, Some(ClientId::from("123456")));

        state.revoke_api_token(&token1).await.unwrap();
        assert_eq!(state.verify_api_token(&token1).await.unwrap(), None);
        assert_eq!(
            state.verify_api_token(&token2).await.unwrap(),
            Some(ClientId::from("123456"))
        );
    }
}
