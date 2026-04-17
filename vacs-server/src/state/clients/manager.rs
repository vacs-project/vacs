use crate::metrics::guards::ClientConnectionGuard;
use crate::metrics::{CoverageMetrics, NetworkDatasetMetrics};
use crate::state::clients::session::ClientSession;
use crate::state::clients::{ClientManagerError, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::instrument;
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId, StationChange, StationId};
use vacs_protocol::ws::server;
use vacs_protocol::ws::server::{
    ClientInfo, DisconnectReason, ServerMessage, SessionProfile, StationInfo,
};
use vacs_vatsim::coverage::network::{Network, RelevantStations};
use vacs_vatsim::coverage::position::Position;
use vacs_vatsim::coverage::profile::Profile;
use vacs_vatsim::data_feed::DataFeed;
use vacs_vatsim::{ControllerInfo, FacilityType};

/// # Lock ordering
///
/// To prevent deadlocks, locks must always be acquired in this order:
///   1. `clients`
///   2. `online_positions`
///   3. `vatsim_only_positions`
///   4. `online_stations`
///
/// Read-only methods that only need a subset may skip unused locks but
/// must never invert this order. Note that this strict order does not
/// apply if a lock is dropped immediately again.
pub struct ClientManager {
    data_feed: Arc<dyn DataFeed>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    network: parking_lot::RwLock<Network>,
    clients: RwLock<HashMap<ClientId, ClientSession>>,
    online_positions: RwLock<HashMap<PositionId, HashSet<ClientId>>>,
    vatsim_only_positions: RwLock<HashMap<PositionId, HashSet<ClientId>>>,
    online_stations: RwLock<HashMap<StationId, PositionId>>,
}

/// Intermediate results from syncing vacs client positions against the VATSIM datafeed.
struct ClientPositionSync {
    /// Clients whose VATSIM connection was lost or became ambiguous.
    disconnected_clients: Vec<(ClientId, DisconnectReason)>,
    /// Session info messages to send to clients whose position changed.
    session_info_updates: Vec<(ClientSession, server::SessionInfo)>,
    /// Client info updates to broadcast to all clients.
    client_info_updates: Vec<ServerMessage>,
    /// Clients that joined an already-online position and need self-handoff events.
    self_handoff_clients: Vec<(ClientSession, PositionId)>,
    /// Clients that left a still-online position and need departure handoff events.
    departure_handoff_clients: Vec<(ClientSession, PositionId)>,
    /// Whether any position was added or removed from the online set.
    positions_changed: bool,
}

impl ClientManager {
    pub fn new(
        broadcast_tx: broadcast::Sender<ServerMessage>,
        network: Network,
        data_feed: Arc<dyn DataFeed>,
    ) -> Self {
        Self {
            data_feed,
            broadcast_tx,
            network: parking_lot::RwLock::new(network),
            clients: RwLock::new(HashMap::new()),
            online_positions: RwLock::new(HashMap::new()),
            vatsim_only_positions: RwLock::new(HashMap::new()),
            online_stations: RwLock::new(HashMap::new()),
        }
    }

    #[instrument(level = "debug", skip(self))]
    pub fn find_positions(&self, controller_info: &ControllerInfo) -> Vec<Position> {
        self.network
            .read()
            .find_positions(
                &controller_info.callsign,
                &controller_info.frequency,
                controller_info.facility_type,
            )
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn get_profile(&self, profile_id: Option<&ProfileId>) -> Option<Profile> {
        profile_id.and_then(|profile_id| self.network.read().get_profile(profile_id).cloned())
    }

    pub fn get_position(&self, position_id: Option<&PositionId>) -> Option<Position> {
        position_id.and_then(|position_id| self.network.read().get_position(position_id).cloned())
    }

    pub async fn clients_for_position(&self, position_id: &PositionId) -> HashSet<ClientId> {
        self.online_positions
            .read()
            .await
            .get(position_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn clients_for_station(&self, station_id: &StationId) -> HashSet<ClientId> {
        let Some(position_id) = self.online_stations.read().await.get(station_id).cloned() else {
            return HashSet::new();
        };
        self.clients_for_position(&position_id).await
    }

    #[instrument(level = "debug", skip(self, client_connection_guard), err)]
    pub async fn add_client(
        &self,
        client_info: ClientInfo,
        active_profile: ActiveProfile<ProfileId>,
        client_connection_guard: ClientConnectionGuard,
    ) -> Result<(ClientSession, mpsc::Receiver<ServerMessage>)> {
        tracing::trace!("Adding client");

        let mut clients = self.clients.write().await;
        if clients.contains_key(&client_info.id) {
            tracing::trace!("Client already exists");
            return Err(ClientManagerError::DuplicateClient(
                client_info.id.to_string(),
            ));
        }

        let (tx, rx) = mpsc::channel(crate::config::CLIENT_CHANNEL_CAPACITY);

        let client = ClientSession::new(
            client_info.clone(),
            active_profile,
            tx,
            client_connection_guard,
        );
        clients.insert(client_info.id.clone(), client.clone());
        drop(clients);

        let changes = if let Some(position_id) = client.position_id() {
            let mut online_positions = self.online_positions.write().await;

            let exists_and_not_empty = online_positions
                .get(position_id)
                .is_some_and(|c| !c.is_empty());

            if exists_and_not_empty {
                tracing::trace!(
                    ?position_id,
                    "Position already exists in online positions list, adding client to list of controllers"
                );
                online_positions
                    .get_mut(position_id)
                    .unwrap()
                    .insert(client_info.id.clone());
                Vec::new()
            } else {
                tracing::trace!(?position_id, "Adding position to online positions list");
                let mut vatsim_only = self.vatsim_only_positions.write().await;
                let was_vatsim_only = vatsim_only.remove(position_id).is_some();

                if was_vatsim_only {
                    drop(vatsim_only);

                    tracing::debug!(
                        ?position_id,
                        "Position was VATSIM-only, transitioning to vacs"
                    );

                    online_positions
                        .insert(position_id.clone(), HashSet::from([client_info.id.clone()]));

                    // The total set of online positions hasn't changed (the
                    // position was already counted via vatsim_only), so there
                    // are no actual coverage changes. However, stations
                    // controlled by this position were invisible to vacs clients
                    // (they received Offline when the position became
                    // VATSIM-only) and now need Online events.
                    let online_stations = self.online_stations.read().await;
                    online_stations
                        .iter()
                        .filter(|(_, controlling_pos)| *controlling_pos == position_id)
                        .map(|(station_id, _)| StationChange::Online {
                            station_id: station_id.clone(),
                            position_id: position_id.clone(),
                        })
                        .collect()
                } else {
                    let all_positions: HashSet<&PositionId> =
                        online_positions.keys().chain(vatsim_only.keys()).collect();
                    let all_changes = self.network.read().coverage_changes(
                        None,
                        Some(position_id),
                        &all_positions,
                    );
                    drop(vatsim_only);

                    online_positions
                        .insert(position_id.clone(), HashSet::from([client_info.id.clone()]));

                    tracing::trace!(
                        ?position_id,
                        "Updating online stations list after position addition"
                    );
                    self.update_online_stations(&all_changes).await;
                    let pos_keys: HashSet<&PositionId> = online_positions.keys().collect();
                    Self::client_visible_changes(&all_changes, &pos_keys, &pos_keys)
                }
            }
        } else {
            tracing::trace!(
                "Client has no position, skipping online positions list addition and station changes broadcast"
            );
            Vec::new()
        };

        if let Err(err) = self.broadcast(server::ClientConnected {
            client: client_info,
        }) {
            tracing::warn!(?err, "Failed to broadcast client connected message");
        }

        self.broadcast_station_changes(&changes).await;
        self.emit_coverage_gauges().await;

        tracing::trace!("Client added");
        Ok((client, rx))
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn remove_client(
        &self,
        client_id: ClientId,
        disconnect_reason: Option<DisconnectReason>,
    ) {
        tracing::trace!("Removing client");

        let Some(client) = self.clients.write().await.remove(&client_id) else {
            tracing::debug!("Client not found in client list, skipping removal");
            return;
        };

        let changes = if let Some(position_id) = client.position_id() {
            let mut online_positions = self.online_positions.write().await;

            if online_positions.contains_key(position_id) {
                let mut changes = Vec::new();

                if online_positions.get(position_id).unwrap().len() == 1 {
                    tracing::trace!(?position_id, "Removing position from online positions list");

                    // Check if the controller is still on VATSIM. If so, the
                    // position transitions to VATSIM-only rather than going
                    // fully offline.
                    let became_vatsim_only = if let Ok(controllers) =
                        self.data_feed.fetch_controller_info().await
                    {
                        let controllers = controllers
                            .into_iter()
                            .filter(|c| !c.callsign.ends_with("_SUP"))
                            .map(|c| (c.cid.clone(), c))
                            .collect();
                        // Collect owned keys so the clients read guard
                        // drops immediately, preserving lock ordering
                        // (clients before online_positions).
                        let vacs_client_ids: HashSet<ClientId> =
                            self.clients.read().await.keys().cloned().collect();
                        let vacs_client_ids: HashSet<&ClientId> = vacs_client_ids.iter().collect();
                        let mut vacs_positions = online_positions.clone();
                        vacs_positions.remove(position_id);

                        let new_vatsim_only = self.rebuild_vatsim_only(
                            &controllers,
                            &vacs_client_ids,
                            &vacs_positions,
                        );

                        // Only insert the departing position into
                        // vatsim_only if it's still covered by a
                        // non-vacs VATSIM controller. Don't replace
                        // the entire map - other entries stay
                        // untouched until the next sync cycle.
                        if let Some(cids) = new_vatsim_only.get(position_id) {
                            self.vatsim_only_positions
                                .write()
                                .await
                                .insert(position_id.clone(), cids.clone());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if became_vatsim_only {
                        // The position transitions from vacs to vatsim-only.
                        // The total set of online positions hasn't changed, so
                        // there are no coverage changes and online_stations
                        // stays intact. However, stations controlled by this
                        // position are no longer callable by vacs clients and
                        // need Offline events.
                        tracing::debug!(
                            ?position_id,
                            "Position still on VATSIM, transitioning to VATSIM-only"
                        );
                        let online_stations = self.online_stations.read().await;
                        changes.extend(
                            online_stations
                                .iter()
                                .filter(|(_, controlling_pos)| *controlling_pos == position_id)
                                .map(|(station_id, _)| StationChange::Offline {
                                    station_id: station_id.clone(),
                                }),
                        );
                    } else {
                        // Position goes fully offline
                        let vatsim_only = self.vatsim_only_positions.read().await;
                        let before_all: HashSet<&PositionId> =
                            online_positions.keys().chain(vatsim_only.keys()).collect();
                        let mut after_all = before_all.clone();
                        after_all.remove(position_id);
                        let all_changes =
                            self.network.read().coverage_diff(&before_all, &after_all);
                        drop(vatsim_only);

                        tracing::trace!(
                            ?position_id,
                            "Updating online stations list after position removal"
                        );
                        self.update_online_stations(&all_changes).await;
                        // Compute client-visible changes BEFORE removing the
                        // position so that `client_visible_changes` still sees
                        // the departing position as a vacs position (not
                        // VATSIM-only). Otherwise Handoff events are
                        // incorrectly downgraded to Online events.
                        let pos_keys: HashSet<&PositionId> = online_positions.keys().collect();
                        changes.extend(Self::client_visible_changes(
                            &all_changes,
                            &pos_keys,
                            &pos_keys,
                        ));
                    }

                    online_positions.remove(position_id);
                } else {
                    tracing::trace!(
                        ?position_id,
                        "Removing client from position in online positions list"
                    );
                    online_positions
                        .get_mut(position_id)
                        .unwrap()
                        .remove(&client_id);
                }

                changes
            } else {
                tracing::trace!(
                    ?position_id,
                    "Position not found in online positions list, skipping removal of client from list of controllers"
                );
                Vec::new()
            }
        } else {
            tracing::trace!(
                "Client has no position, skipping online positions list removal and station changes broadcast"
            );
            Vec::new()
        };
        client.disconnect(disconnect_reason);

        if let Err(err) = self.broadcast(server::ClientDisconnected { client_id }) {
            tracing::warn!(?err, "Failed to broadcast client disconnected message");
        }

        if self.clients.read().await.is_empty() {
            tracing::debug!(
                "Last client disconnected, clearing VATSIM-only positions and online stations"
            );
            self.vatsim_only_positions.write().await.clear();
            self.online_stations.write().await.clear();
            return;
        }

        self.broadcast_station_changes(&changes).await;
        self.emit_coverage_gauges().await;

        tracing::debug!("Client removed");
    }

    pub async fn list_clients(&self, self_client_id: Option<&ClientId>) -> Vec<ClientInfo> {
        let mut clients: Vec<ClientInfo> = self
            .clients
            .read()
            .await
            .values()
            .filter(|c| self_client_id.map(|s| s != c.id()).unwrap_or(true))
            .map(|c| c.client_info().clone())
            .collect();

        clients.sort_by(|a, b| a.id.cmp(&b.id));
        clients
    }

    pub async fn list_stations(
        &self,
        profile: &ActiveProfile<ProfileId>,
        self_position_id: Option<&PositionId>,
    ) -> Vec<StationInfo> {
        // Resolve relevant station IDs synchronously to avoid holding parking_lot
        // lock across await points
        let relevant_station_ids = {
            let network = self.network.read();
            match network.relevant_stations(profile) {
                RelevantStations::All => None,
                RelevantStations::Subset(ids) => Some(ids.clone()),
                RelevantStations::None => return Vec::new(),
            }
        };
        let online_positions = self.online_positions.read().await;
        let online_stations = self.online_stations.read().await;

        let mut stations: Vec<StationInfo> = match relevant_station_ids {
            None => online_stations
                .iter()
                .filter(|(_, position_id)| online_positions.contains_key(*position_id))
                .map(|(id, controller)| {
                    let own = self_position_id
                        .map(|self_pos| controller == self_pos)
                        .unwrap_or(false);
                    StationInfo {
                        id: id.clone(),
                        own,
                    }
                })
                .collect(),
            Some(ids) => ids
                .iter()
                .filter_map(|id| {
                    online_stations.get(id).and_then(|controller| {
                        online_positions.contains_key(controller).then(|| {
                            let own = self_position_id
                                .map(|self_pos| controller == self_pos)
                                .unwrap_or(false);
                            StationInfo {
                                id: id.clone(),
                                own,
                            }
                        })
                    })
                })
                .collect(),
        };

        stations.sort_by(|a, b| a.id.cmp(&b.id));
        stations
    }

    pub async fn get_client(&self, client_id: &ClientId) -> Option<ClientSession> {
        self.clients.read().await.get(client_id).cloned()
    }

    pub async fn is_client_connected(&self, client_id: &ClientId) -> bool {
        self.clients.read().await.contains_key(client_id)
    }

    pub async fn is_empty(&self) -> bool {
        self.clients.read().await.is_empty()
    }

    /// Returns coverage info for a single station, or `None` if the station
    /// is not currently online.
    pub async fn station_coverage(&self, station_id: &StationId) -> Option<StationCoverage> {
        let pid = self.online_stations.read().await.get(station_id)?.clone();

        let online_positions = self.online_positions.read().await;
        let vatsim_only = self.vatsim_only_positions.read().await;

        let (controller_ids, is_vatsim_only) = vatsim_only
            .get(&pid)
            .map(|cids| (cids.clone(), true))
            .unwrap_or_else(|| {
                let cids = online_positions.get(&pid).cloned().unwrap_or_default();
                (cids, false)
            });

        Some(StationCoverage {
            station_id: station_id.clone(),
            controlling_position_id: pid,
            controller_ids,
            vatsim_only: is_vatsim_only,
        })
    }

    /// Returns a merged snapshot of the current coverage state.
    pub async fn coverage_snapshot(&self) -> CoverageSnapshot {
        let online_positions = self.online_positions.read().await;
        let vatsim_only = self.vatsim_only_positions.read().await;
        let online_stations = self.online_stations.read().await;

        let mut positions: Vec<PositionCoverage> = online_positions
            .iter()
            .map(|(pid, cids)| (pid, cids, false))
            .chain(vatsim_only.iter().map(|(pid, cids)| (pid, cids, true)))
            .map(|(pid, cids, vatsim_only)| PositionCoverage {
                position_id: pid.clone(),
                controller_ids: cids.clone(),
                vatsim_only,
            })
            .collect();
        positions.sort_unstable_by(|a, b| a.position_id.cmp(&b.position_id));

        let mut stations: Vec<StationCoverage> = online_stations
            .iter()
            .map(|(sid, pid)| {
                let (controller_ids, vatsim_only) = vatsim_only
                    .get(pid)
                    .map(|cids| (cids.clone(), true))
                    .unwrap_or_else(|| {
                        let cids = online_positions.get(pid).cloned().unwrap_or_default();
                        (cids, false)
                    });
                StationCoverage {
                    station_id: sid.clone(),
                    controlling_position_id: pid.clone(),
                    controller_ids,
                    vatsim_only,
                }
            })
            .collect();
        stations.sort_unstable_by(|a, b| a.station_id.cmp(&b.station_id));

        CoverageSnapshot {
            positions,
            stations,
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn broadcast(
        &self,
        message: impl Into<ServerMessage>,
    ) -> Result<usize, SendError<ServerMessage>> {
        let message = message.into();
        if self.broadcast_tx.receiver_count() > 0 {
            tracing::trace!(message_variant = message.variant(), "Broadcasting message");
            self.broadcast_tx.send(message)
        } else {
            tracing::trace!(
                message_variant = message.variant(),
                "No receivers subscribed, skipping message broadcast"
            );
            Ok(0)
        }
    }

    pub async fn replace_network(&self, network: Network) {
        tracing::info!(?network, "Replacing network coverage data");
        *self.network.write() = network;

        tracing::debug!("Network coverage data replaced, starting housekeeping");

        let old_online_stations = self.online_stations.read().await.clone();

        let mut online_positions = self.online_positions.write().await;
        let mut clients = self.clients.write().await;
        let mut vatsim_only = self.vatsim_only_positions.write().await;

        let (session_updates, new_online_stations) = {
            let network = self.network.read();
            let mut session_updates: Vec<(ClientSession, server::SessionInfo)> = Vec::new();

            // Remove positions that no longer exist in the new network
            let stale_positions: Vec<PositionId> = online_positions
                .keys()
                .filter(|pos_id| network.get_position(pos_id).is_none())
                .cloned()
                .collect();

            for stale_pos_id in &stale_positions {
                tracing::debug!(
                    ?stale_pos_id,
                    "Position no longer exists in new network, removing"
                );
                if let Some(client_ids) = online_positions.remove(stale_pos_id) {
                    for client_id in client_ids {
                        if let Some(session) = clients.get_mut(&client_id) {
                            tracing::debug!(
                                ?client_id,
                                ?stale_pos_id,
                                "Clearing stale position from client"
                            );
                            session.set_position_id(None);
                            let session_profile = session.update_active_profile(None, &network);
                            session_updates.push((
                                session.clone(),
                                server::SessionInfo {
                                    client: session.client_info().clone(),
                                    profile: session_profile,
                                    default_call_sources: Vec::new(),
                                },
                            ));
                        }
                    }
                }
            }

            // Remove VATSIM-only positions that no longer exist in the new network
            let stale_vatsim_only: Vec<PositionId> = vatsim_only
                .keys()
                .filter(|pos_id| network.get_position(pos_id).is_none())
                .cloned()
                .collect();

            for stale_pos_id in &stale_vatsim_only {
                tracing::debug!(
                    ?stale_pos_id,
                    "VATSIM-only position no longer exists in new network, removing"
                );
                vatsim_only.remove(stale_pos_id);
            }

            // Re-transmit profiles for all clients on surviving positions.
            // Profile *content* may change during a dataset reload even when
            // the profile ID stays the same, and we cannot cheaply detect
            // content changes, so we always send the resolved profile.
            for (pos_id, client_ids) in online_positions.iter() {
                let (new_profile_id, new_default_call_sources) = network
                    .get_position(pos_id)
                    .map(|p| (p.profile_id.clone(), p.default_call_sources.clone()))
                    .unwrap_or((None, Vec::new()));

                for client_id in client_ids {
                    if let Some(session) = clients.get_mut(client_id) {
                        let tracked =
                            session.update_active_profile(new_profile_id.clone(), &network);

                        let session_profile = match tracked {
                            // Profile ID changed or was cleared, send change.
                            SessionProfile::Changed(_) => tracked,

                            // Profile ID unchanged, but content may have changed under the same ID
                            // during the reload. Re-resolve Specific profiles; skip Custom/None.
                            SessionProfile::Unchanged => match session.active_profile() {
                                ActiveProfile::Specific(profile_id) => {
                                    match network.get_profile(profile_id) {
                                        Some(profile) => SessionProfile::Changed(
                                            ActiveProfile::Specific(profile.into()),
                                        ),
                                        None => {
                                            tracing::warn!(
                                                ?profile_id,
                                                "Profile not found in new network"
                                            );
                                            SessionProfile::Changed(ActiveProfile::None)
                                        }
                                    }
                                }
                                _ => continue,
                            },
                        };

                        tracing::debug!(
                            ?client_id,
                            ?pos_id,
                            "Re-transmitting profile to client after network reload"
                        );
                        session_updates.push((
                            session.clone(),
                            server::SessionInfo {
                                client: session.client_info().clone(),
                                profile: session_profile,
                                default_call_sources: new_default_call_sources.clone(),
                            },
                        ));
                    }
                }
            }

            // Recalculate the full online stations map from scratch, including
            // VATSIM-only positions for correct coverage computation
            let all_online_pos_ids: HashSet<&PositionId> =
                online_positions.keys().chain(vatsim_only.keys()).collect();

            let mut new_online_stations: HashMap<StationId, PositionId> = HashMap::new();
            let covered = network.covered_stations(None, &all_online_pos_ids);
            for covered_station in covered {
                if let Some(controlling_pos) =
                    network.controlling_position(&covered_station.station.id, &all_online_pos_ids)
                {
                    new_online_stations.insert(
                        covered_station.station.id.clone(),
                        controlling_pos.id.clone(),
                    );
                }
            }

            (session_updates, new_online_stations)
        };

        let all_changes = Self::compute_station_diff(&old_online_stations, &new_online_stations);
        self.update_online_stations(&all_changes).await;
        let pos_keys: HashSet<&PositionId> = online_positions.keys().collect();
        let station_changes = Self::client_visible_changes(&all_changes, &pos_keys, &pos_keys);

        drop(vatsim_only);
        drop(clients);
        drop(online_positions);

        for (session, session_info) in session_updates {
            if let Err(err) = session.send_message(session_info).await {
                tracing::warn!(
                    ?err,
                    client_id = ?session.id(),
                    "Failed to send updated session info after network reload"
                );
            }
        }

        self.broadcast_station_changes(&station_changes).await;
        self.emit_coverage_gauges().await;

        {
            let network = self.network.read();
            NetworkDatasetMetrics::set_dataset_size(
                network.positions_count(),
                network.stations_count(),
                network.profiles_count(),
            );
        }

        tracing::info!("Network housekeeping completed");
    }

    pub async fn sync_vatsim_state(
        &self,
        controllers: &HashMap<ClientId, ControllerInfo>,
        pending_disconnect: &mut HashSet<ClientId>,
        require_active_connection: bool,
    ) -> Vec<(ClientId, DisconnectReason)> {
        let mut coverage_changes: Vec<StationChange> = Vec::new();

        let (sync, per_client_changes) = {
            let mut clients = self.clients.write().await;
            let mut online_positions = self.online_positions.write().await;
            let mut vatsim_only = self.vatsim_only_positions.write().await;

            let start_all_positions: HashSet<PositionId> = online_positions
                .keys()
                .chain(vatsim_only.keys())
                .cloned()
                .collect();

            // Capture online position keys before sync_client_positions mutates them.
            // Used as the "from" set in client_visible_changes so that handoffs from
            // positions that are removed during sync are still classified correctly.
            let start_online_keys: HashSet<PositionId> = online_positions.keys().cloned().collect();

            let mut sync = self.sync_client_positions(
                controllers,
                pending_disconnect,
                require_active_connection,
                &mut clients,
                &mut online_positions,
            );

            let vacs_client_ids: HashSet<&ClientId> = clients.keys().collect();
            let new_vatsim_only =
                self.rebuild_vatsim_only(controllers, &vacs_client_ids, &online_positions);

            if *vatsim_only != new_vatsim_only {
                tracing::debug!(
                    before = vatsim_only.len(),
                    after = new_vatsim_only.len(),
                    "VATSIM-only positions changed"
                );
                *vatsim_only = new_vatsim_only;
                sync.positions_changed = true;
            }

            if sync.positions_changed {
                tracing::debug!("Online positions changed, calculating coverage changes");
                let start_all = start_all_positions.iter().collect::<HashSet<_>>();
                let end_all: HashSet<&PositionId> =
                    online_positions.keys().chain(vatsim_only.keys()).collect();

                let all_changes = self.network.read().coverage_diff(&start_all, &end_all);
                self.update_online_stations(&all_changes).await;
                let from_online: HashSet<&PositionId> = start_online_keys.iter().collect();
                let to_online: HashSet<&PositionId> = online_positions.keys().collect();
                coverage_changes.extend(Self::client_visible_changes(
                    &all_changes,
                    &from_online,
                    &to_online,
                ));
            }

            let mut all_handoff_clients = std::mem::take(&mut sync.self_handoff_clients);
            all_handoff_clients.append(&mut sync.departure_handoff_clients);

            let per_client_changes = self.compute_self_handoffs(
                all_handoff_clients,
                &coverage_changes,
                &online_positions,
                &vatsim_only,
            );

            (sync, per_client_changes)
        };

        // Phase 5: Send all collected messages (locks released)
        for (session, session_info) in sync.session_info_updates {
            if let Err(err) = session.send_message(session_info).await {
                tracing::warn!(
                    ?err,
                    client_id = ?session.id(),
                    "Failed to send updated session info to client"
                );
            }
        }

        for (session, changes) in per_client_changes {
            if let Err(err) = session
                .send_message(server::StationChanges { changes })
                .await
            {
                tracing::warn!(
                    ?err,
                    client_id = ?session.id(),
                    "Failed to send self-handoff station changes to client"
                );
            }
        }

        if self.broadcast_tx.receiver_count() > 0 {
            for msg in sync.client_info_updates {
                if let Err(err) = self.broadcast(msg) {
                    tracing::warn!(?err, "Failed to broadcast client info update");
                }
            }
        }

        self.broadcast_station_changes(&coverage_changes).await;
        self.emit_coverage_gauges().await;

        sync.disconnected_clients
    }

    /// Iterates all connected vacs clients and checks each against the VATSIM
    /// datafeed. Handles disconnect decisions, position changes (including
    /// updating `online_positions`), and collects session info updates,
    /// client info broadcasts, and self-handoff candidates.
    fn sync_client_positions(
        &self,
        controllers: &HashMap<ClientId, ControllerInfo>,
        pending_disconnect: &mut HashSet<ClientId>,
        require_active_connection: bool,
        clients: &mut HashMap<ClientId, ClientSession>,
        online_positions: &mut HashMap<PositionId, HashSet<ClientId>>,
    ) -> ClientPositionSync {
        let mut result = ClientPositionSync {
            disconnected_clients: Vec::new(),
            session_info_updates: Vec::new(),
            client_info_updates: Vec::new(),
            self_handoff_clients: Vec::new(),
            departure_handoff_clients: Vec::new(),
            positions_changed: false,
        };

        fn disconnect_or_mark_pending(
            cid: &ClientId,
            pending_disconnect: &mut HashSet<ClientId>,
            disconnected_clients: &mut Vec<(ClientId, DisconnectReason)>,
        ) {
            if pending_disconnect.remove(cid) {
                tracing::trace!(
                    ?cid,
                    "No active VATSIM connection found after grace period, disconnecting client and sending broadcast"
                );
                disconnected_clients
                    .push((cid.clone(), DisconnectReason::NoActiveVatsimConnection));
            } else {
                tracing::trace!(
                    ?cid,
                    "Client not found in data feed, but active VATSIM connection is required, marking for disconnect"
                );
                pending_disconnect.insert(cid.clone());
            }
        }

        for (cid, session) in clients.iter_mut() {
            tracing::trace!(?cid, ?session, "Checking session for client info update");

            match controllers.get(cid) {
                Some(controller) if controller.facility_type == FacilityType::Unknown => {
                    if require_active_connection {
                        disconnect_or_mark_pending(
                            cid,
                            pending_disconnect,
                            &mut result.disconnected_clients,
                        );
                    }
                }
                None => {
                    if require_active_connection {
                        disconnect_or_mark_pending(
                            cid,
                            pending_disconnect,
                            &mut result.disconnected_clients,
                        );
                    }
                }
                Some(controller) => {
                    if pending_disconnect.remove(cid) {
                        tracing::trace!(
                            ?cid,
                            "Found active VATSIM connection for client again, removing pending disconnect"
                        );
                    }

                    let updated = session.update_client_info(controller);
                    if updated {
                        tracing::trace!(?cid, ?session, "Client info updated, updating position");

                        let old_position_id = session.position_id().cloned();
                        let new_positions: Vec<Position> = self
                            .network
                            .read()
                            .find_positions(
                                &controller.callsign,
                                &controller.frequency,
                                controller.facility_type,
                            )
                            .into_iter()
                            .cloned()
                            .collect();

                        let new_position = if new_positions.len() > 1 {
                            tracing::info!(
                                ?cid,
                                ?old_position_id,
                                ?new_positions,
                                "Multiple positions found for updated client info, disconnecting as ambiguous"
                            );
                            pending_disconnect.remove(cid);
                            result.disconnected_clients.push((
                                cid.clone(),
                                DisconnectReason::AmbiguousVatsimPosition(
                                    new_positions.into_iter().map(|p| p.id.clone()).collect(),
                                ),
                            ));
                            continue;
                        } else if new_positions.len() == 1 {
                            Some(&new_positions[0])
                        } else {
                            None
                        };
                        let (new_position_id, new_default_call_sources) = new_position
                            .map(|p| (Some(p.id.clone()), p.default_call_sources.clone()))
                            .unwrap_or((None, Vec::new()));

                        if old_position_id != new_position_id {
                            tracing::debug!(
                                ?cid,
                                ?new_position_id,
                                ?old_position_id,
                                "Client position changed"
                            );

                            session.set_position_id(new_position_id.clone());

                            if let Some(old_position_id) = &old_position_id {
                                if online_positions
                                    .get(old_position_id)
                                    .map(|s| s.len() <= 1)
                                    .unwrap_or(false)
                                {
                                    tracing::trace!(
                                        ?cid,
                                        ?old_position_id,
                                        "Removing position from online positions list"
                                    );
                                    online_positions.remove(old_position_id);
                                    result.positions_changed = true;
                                } else if let Some(clients) =
                                    online_positions.get_mut(old_position_id)
                                {
                                    tracing::trace!(
                                        ?cid,
                                        ?old_position_id,
                                        "Removing client from shared position in online positions list"
                                    );
                                    clients.remove(cid);
                                    // The position stays online with remaining clients.
                                    // This client needs departure events for stations
                                    // that won't appear in the global coverage_diff.
                                    result
                                        .departure_handoff_clients
                                        .push((session.clone(), old_position_id.clone()));
                                }
                            }

                            if let Some(new_position_id) = &new_position_id {
                                let clients =
                                    online_positions.entry(new_position_id.clone()).or_default();
                                if clients.insert(cid.clone()) && clients.len() == 1 {
                                    result.positions_changed = true;
                                }

                                // When the new position already had other
                                // clients, the position-level coverage_diff
                                // won't emit events for stations that stay
                                // under the same controller. But this client
                                // needs "self-handoff" events so it knows
                                // those stations are now "own".
                                if clients.len() > 1 {
                                    result
                                        .self_handoff_clients
                                        .push((session.clone(), new_position_id.clone()));
                                }
                            }

                            let session_profile = {
                                let network = self.network.read();
                                session.update_active_profile(
                                    new_position.and_then(|p| p.profile_id.clone()),
                                    &network,
                                )
                            };

                            result.session_info_updates.push((
                                session.clone(),
                                server::SessionInfo {
                                    client: session.client_info().clone(),
                                    profile: session_profile,
                                    default_call_sources: new_default_call_sources.clone(),
                                },
                            ));
                        }

                        tracing::trace!(?cid, ?session, "Client info updated, broadcasting");
                        result
                            .client_info_updates
                            .push(ServerMessage::from(session.client_info().clone()));
                    }
                }
            }
        }

        result
    }

    /// Builds a fresh VATSIM-only position map from non-vacs controllers in the
    /// datafeed. Positions that are already covered by a vacs client are excluded.
    fn rebuild_vatsim_only(
        &self,
        controllers: &HashMap<ClientId, ControllerInfo>,
        vacs_client_ids: &HashSet<&ClientId>,
        online_positions: &HashMap<PositionId, HashSet<ClientId>>,
    ) -> HashMap<PositionId, HashSet<ClientId>> {
        let mut new_vatsim_only: HashMap<PositionId, HashSet<ClientId>> = HashMap::new();
        let network = self.network.read();

        for (cid, controller) in controllers {
            if controller.facility_type == FacilityType::Unknown || vacs_client_ids.contains(cid) {
                continue;
            }
            let positions: Vec<Position> = network
                .find_positions(
                    &controller.callsign,
                    &controller.frequency,
                    controller.facility_type,
                )
                .into_iter()
                .cloned()
                .collect();
            if positions.len() == 1 && !online_positions.contains_key(&positions[0].id) {
                new_vatsim_only
                    .entry(positions[0].id.clone())
                    .or_default()
                    .insert(cid.clone());
            }
        }

        new_vatsim_only
    }

    /// Computes per-client self-handoff events for clients that joined an
    /// already-online position. Stations controlled by the position that
    /// weren't affected by the global `coverage_diff` get a `Handoff(pos → pos)`
    /// so the client knows they are now "own".
    fn compute_self_handoffs(
        &self,
        self_handoff_clients: Vec<(ClientSession, PositionId)>,
        coverage_changes: &[StationChange],
        online_positions: &HashMap<PositionId, HashSet<ClientId>>,
        vatsim_only: &HashMap<PositionId, HashSet<ClientId>>,
    ) -> Vec<(ClientSession, Vec<StationChange>)> {
        if self_handoff_clients.is_empty() {
            return Vec::new();
        }

        let changed_station_ids: HashSet<&StationId> = coverage_changes
            .iter()
            .map(StationChange::station_id)
            .collect();
        let network = self.network.read();
        let all_online: HashSet<&PositionId> =
            online_positions.keys().chain(vatsim_only.keys()).collect();

        self_handoff_clients
            .into_iter()
            .filter_map(|(session, new_pos_id)| {
                let position = network.get_position(&new_pos_id)?;
                let mut self_handoffs: Vec<StationChange> = position
                    .controlled_stations
                    .iter()
                    .filter(|sid| !changed_station_ids.contains(sid))
                    .filter_map(|station_id| {
                        let controller = network.controlling_position(station_id, &all_online)?;
                        (controller.id == new_pos_id).then(|| StationChange::Handoff {
                            station_id: station_id.clone(),
                            from_position_id: new_pos_id.clone(),
                            to_position_id: new_pos_id.clone(),
                        })
                    })
                    .collect();
                self_handoffs.sort();

                if self_handoffs.is_empty() {
                    None
                } else {
                    tracing::debug!(
                        client_id = ?session.id(),
                        ?new_pos_id,
                        count = self_handoffs.len(),
                        "Prepared self-handoff station changes"
                    );
                    Some((session, self_handoffs))
                }
            })
            .collect()
    }

    fn compute_station_diff(
        old: &HashMap<StationId, PositionId>,
        new: &HashMap<StationId, PositionId>,
    ) -> Vec<StationChange> {
        let mut changes = Vec::new();

        // Stations that went offline or changed controller
        for (station_id, old_pos_id) in old {
            match new.get(station_id) {
                None => {
                    changes.push(StationChange::Offline {
                        station_id: station_id.clone(),
                    });
                }
                Some(new_pos_id) if new_pos_id != old_pos_id => {
                    changes.push(StationChange::Handoff {
                        station_id: station_id.clone(),
                        from_position_id: old_pos_id.clone(),
                        to_position_id: new_pos_id.clone(),
                    });
                }
                _ => {}
            }
        }

        // Stations that came online
        for (station_id, new_pos_id) in new {
            if !old.contains_key(station_id) {
                changes.push(StationChange::Online {
                    station_id: station_id.clone(),
                    position_id: new_pos_id.clone(),
                });
            }
        }

        changes.sort();
        changes
    }

    /// Transforms station changes to only include changes visible to vacs clients.
    /// Stations covered solely by VATSIM-only positions are not callable, so:
    /// - `Online` for a VATSIM-only position is dropped
    /// - `Offline` events are always forwarded, even if the previous covering position
    ///   was VATSIM-only. Clients handle duplicate/unknown `Offline` events gracefully.
    /// - `Handoff` to a VATSIM-only position becomes `Offline` (station leaves vacs coverage)
    /// - `Handoff` from a VATSIM-only position becomes `Online` (station enters vacs coverage)
    ///
    /// `from_online_positions` is the set of position IDs that were in `online_positions`
    /// before the change (used for `from_position_id` lookups in handoffs).
    /// `to_online_positions` is the set of position IDs in `online_positions` after the
    /// change (used for `Online` events and `to_position_id` lookups in handoffs).
    ///
    /// When no positions are added or removed between the before/after snapshots
    /// (e.g. in `add_client` and `remove_client`), both parameters point to the
    /// same set.
    fn client_visible_changes(
        changes: &[StationChange],
        from_online_positions: &HashSet<&PositionId>,
        to_online_positions: &HashSet<&PositionId>,
    ) -> Vec<StationChange> {
        changes
            .iter()
            .filter_map(|change| match change {
                StationChange::Online { position_id, .. } => {
                    if to_online_positions.contains(position_id) {
                        Some(change.clone())
                    } else {
                        None
                    }
                }
                StationChange::Handoff {
                    station_id,
                    from_position_id,
                    to_position_id,
                } => {
                    let from_vacs = from_online_positions.contains(from_position_id);
                    let to_vacs = to_online_positions.contains(to_position_id);
                    match (from_vacs, to_vacs) {
                        // vacs -> vacs: normal handoff
                        (true, true) => Some(change.clone()),
                        // vacs -> VATSIM-only: station leaves vacs coverage
                        (true, false) => Some(StationChange::Offline {
                            station_id: station_id.clone(),
                        }),
                        // VATSIM-only -> vacs: station enters vacs coverage
                        (false, true) => Some(StationChange::Online {
                            station_id: station_id.clone(),
                            position_id: to_position_id.clone(),
                        }),
                        // VATSIM-only -> VATSIM-only: invisible to clients
                        (false, false) => None,
                    }
                }
                StationChange::Offline { .. } => Some(change.clone()),
            })
            .collect()
    }

    async fn update_online_stations(&self, changes: &[StationChange]) {
        if changes.is_empty() {
            return;
        }

        let mut online_stations = self.online_stations.write().await;
        for change in changes {
            match change {
                StationChange::Online {
                    station_id,
                    position_id,
                } => {
                    online_stations.insert(station_id.clone(), position_id.clone());
                }
                StationChange::Offline { station_id } => {
                    online_stations.remove(station_id);
                }
                StationChange::Handoff {
                    station_id,
                    to_position_id,
                    ..
                } => {
                    online_stations.insert(station_id.clone(), to_position_id.clone());
                }
            }
        }
    }

    async fn broadcast_station_changes(&self, changes: &[StationChange]) {
        if changes.is_empty() {
            return;
        }

        for change in changes {
            CoverageMetrics::station_change(change.as_str());
        }

        tracing::trace!(?changes, "Sending station changes to clients");

        let mut filtered_changes_cache: HashMap<ActiveProfile<ProfileId>, Vec<StationChange>> =
            HashMap::new();

        let clients = self
            .clients
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();

        for client in clients {
            let profile = client.active_profile();

            let changes_to_send = if let Some(cached_changes) = filtered_changes_cache.get(profile)
            {
                cached_changes.clone()
            } else {
                let relevant_station_ids = {
                    let network = self.network.read();
                    match network.relevant_stations(profile) {
                        RelevantStations::All => None,
                        RelevantStations::Subset(ids) => Some(ids.clone()),
                        RelevantStations::None => Some(HashSet::new()),
                    }
                };

                let filtered_changes = match relevant_station_ids {
                    None => changes.to_vec(),
                    Some(relevant_ids) if relevant_ids.is_empty() => Vec::new(),
                    Some(relevant_ids) => changes
                        .iter()
                        .filter(|change| {
                            let station_id = match change {
                                StationChange::Online { station_id, .. } => station_id,
                                StationChange::Offline { station_id } => station_id,
                                StationChange::Handoff { station_id, .. } => station_id,
                            };
                            relevant_ids.contains(station_id)
                        })
                        .cloned()
                        .collect(),
                };

                filtered_changes_cache.insert(profile.clone(), filtered_changes.clone());
                filtered_changes
            };

            if changes_to_send.is_empty() {
                continue;
            }

            if let Err(err) = client
                .send_message(server::StationChanges {
                    changes: changes_to_send,
                })
                .await
            {
                tracing::warn!(?err, ?client, "Failed to send station changes to client");
            }
        }
    }

    async fn emit_coverage_gauges(&self) {
        let online_positions = self.online_positions.read().await;
        let online_stations = self.online_stations.read().await;
        let vatsim_only = self.vatsim_only_positions.read().await;

        CoverageMetrics::set_stations_online(online_stations.len());
        CoverageMetrics::set_positions_vatsim_only(vatsim_only.len());

        let mut facility_counts: HashMap<FacilityType, usize> = HashMap::new();
        let network = self.network.read();
        for position_id in online_positions.keys() {
            if let Some(position) = network.get_position(position_id) {
                *facility_counts.entry(position.facility_type).or_default() += 1;
            }
        }
        drop(network);
        drop(online_positions);
        drop(online_stations);
        drop(vatsim_only);

        for facility_type in FacilityType::ALL {
            CoverageMetrics::set_positions_online(
                facility_type,
                facility_counts.get(facility_type).copied().unwrap_or(0),
            );
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageSnapshot {
    pub positions: Vec<PositionCoverage>,
    pub stations: Vec<StationCoverage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionCoverage {
    pub position_id: PositionId,
    pub controller_ids: HashSet<ClientId>,
    pub vatsim_only: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationCoverage {
    pub station_id: StationId,
    pub controlling_position_id: PositionId,
    pub controller_ids: HashSet<ClientId>,
    pub vatsim_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use vacs_vatsim::coverage::test_support::TestFirBuilder;

    fn pos(id: &str) -> PositionId {
        PositionId::from(id)
    }

    fn station(id: &str) -> StationId {
        StationId::from(id)
    }

    fn cid(id: &str) -> ClientId {
        ClientId::from(id)
    }

    fn controller(cid: &str, callsign: &str, freq: &str, ft: FacilityType) -> ControllerInfo {
        ControllerInfo {
            cid: ClientId::from(cid),
            callsign: callsign.to_string(),
            frequency: freq.to_string(),
            facility_type: ft,
        }
    }

    fn client_info(id: &str, position_id: &str, freq: &str) -> ClientInfo {
        ClientInfo {
            id: ClientId::from(id),
            position_id: Some(PositionId::from(position_id)),
            display_name: id.to_string(),
            frequency: freq.to_string(),
        }
    }

    fn client_info_without_position(id: &str) -> ClientInfo {
        ClientInfo {
            id: ClientId::from(id),
            position_id: None,
            display_name: id.to_string(),
            frequency: String::new(),
        }
    }

    fn online_positions(entries: &[&str]) -> HashMap<PositionId, HashSet<ClientId>> {
        entries
            .iter()
            .map(|id| (pos(id), HashSet::from([cid("1000000")])))
            .collect()
    }

    fn client_manager(network: Network) -> ClientManager {
        let (tx, _) = broadcast::channel(64);
        let data_feed = Arc::new(vacs_vatsim::data_feed::mock::MockDataFeed::new(Vec::new()));
        ClientManager::new(tx, network, data_feed)
    }

    struct DrainedMessages {
        station_changes: Vec<StationChange>,
        session_infos: Vec<server::SessionInfo>,
    }

    /// Drain all pending messages from a client receiver, collecting station
    /// changes (sorted for deterministic comparison) and session info updates.
    fn drain_messages(rx: &mut mpsc::Receiver<ServerMessage>) -> DrainedMessages {
        let mut station_changes = Vec::new();
        let mut session_infos = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ServerMessage::StationChanges(sc) => station_changes.extend(sc.changes),
                ServerMessage::SessionInfo(si) => session_infos.push(si),
                _ => {}
            }
        }
        station_changes.sort();
        DrainedMessages {
            station_changes,
            session_infos,
        }
    }

    #[test]
    fn online_vacs_position_is_visible() {
        let changes = vec![StationChange::Online {
            station_id: station("LOWW_TWR"),
            position_id: pos("LOWW_TWR"),
        }];
        let positions = online_positions(&["LOWW_TWR"]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert_eq!(result, changes);
    }

    #[test]
    fn online_vatsim_only_position_is_dropped() {
        let changes = vec![StationChange::Online {
            station_id: station("LOWW_TWR"),
            position_id: pos("LOWW_TWR"),
        }];
        let positions = online_positions(&[]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert!(result.is_empty());
    }

    #[test]
    fn handoff_vacs_to_vacs_is_visible() {
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_APP"),
            from_position_id: pos("LOVV_CTR"),
            to_position_id: pos("LOWW_APP"),
        }];
        let positions = online_positions(&["LOVV_CTR", "LOWW_APP"]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert_eq!(result, changes);
    }

    #[test]
    fn handoff_vacs_to_vatsim_only_becomes_offline() {
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_TWR"),
            from_position_id: pos("LOWW_APP"),
            to_position_id: pos("LOWW_TWR"),
        }];
        let positions = online_positions(&["LOWW_APP"]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert_eq!(
            result,
            vec![StationChange::Offline {
                station_id: station("LOWW_TWR"),
            }]
        );
    }

    #[test]
    fn handoff_vatsim_only_to_vacs_becomes_online() {
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_TWR"),
            from_position_id: pos("LOWW_TWR"),
            to_position_id: pos("LOWW_APP"),
        }];
        let positions = online_positions(&["LOWW_APP"]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert_eq!(
            result,
            vec![StationChange::Online {
                station_id: station("LOWW_TWR"),
                position_id: pos("LOWW_APP"),
            }]
        );
    }

    #[test]
    fn handoff_vatsim_only_to_vatsim_only_is_dropped() {
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_APP"),
            from_position_id: pos("LOVV_CTR"),
            to_position_id: pos("LOWW_APP"),
        }];
        let positions = online_positions(&[]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert!(result.is_empty());
    }

    #[test]
    fn offline_is_always_visible() {
        let changes = vec![StationChange::Offline {
            station_id: station("LOWW_TWR"),
        }];
        let positions = online_positions(&[]);
        let pos_keys: HashSet<&PositionId> = positions.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &pos_keys, &pos_keys);
        assert_eq!(result, changes);
    }

    #[test]
    fn handoff_from_removed_vacs_position_stays_handoff() {
        // Simulates the sync_vatsim_state scenario: a position was in
        // online_positions before sync but got removed. The handoff from
        // that position to another vacs position should remain a Handoff,
        // not become an Online (which would happen if we used the mutated
        // post-sync positions for both from and to lookups).
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_APP"),
            from_position_id: pos("LOVV_CTR"),
            to_position_id: pos("LOWW_APP"),
        }];

        // Before sync: both positions online
        let before = online_positions(&["LOVV_CTR", "LOWW_APP"]);
        let from_keys: HashSet<&PositionId> = before.keys().collect();

        // After sync: LOVV_CTR removed
        let after = online_positions(&["LOWW_APP"]);
        let to_keys: HashSet<&PositionId> = after.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &from_keys, &to_keys);
        assert_eq!(
            result, changes,
            "handoff should stay a handoff, not become Online"
        );
    }

    #[test]
    fn handoff_from_removed_vacs_to_vatsim_only_becomes_offline() {
        // After sync, from_position removed and to_position is vatsim-only.
        // Should become Offline (station leaves vacs coverage).
        let changes = vec![StationChange::Handoff {
            station_id: station("LOWW_TWR"),
            from_position_id: pos("LOWW_APP"),
            to_position_id: pos("LOWW_TWR"),
        }];

        let before = online_positions(&["LOWW_APP"]);
        let from_keys: HashSet<&PositionId> = before.keys().collect();

        let after = online_positions(&[]);
        let to_keys: HashSet<&PositionId> = after.keys().collect();

        let result = ClientManager::client_visible_changes(&changes, &from_keys, &to_keys);
        assert_eq!(
            result,
            vec![StationChange::Offline {
                station_id: station("LOWW_TWR"),
            }]
        );
    }

    #[tokio::test]
    async fn vatsim_only_position_removes_station_from_vacs_client() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // LOWW_APP should cover LOWW_APP, LOWW_TWR, LOWW_GND, LOWW_DEL stations
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOWW_APP")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(station_ids.contains(&"LOWW_APP"));
        assert!(station_ids.contains(&"LOWW_TWR"));
        assert!(station_ids.contains(&"LOWW_GND"));
        assert!(station_ids.contains(&"LOWW_DEL"));

        // Now LOWW_TWR comes online on VATSIM only (not on vacs)
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOWW_APP", "134.675", FacilityType::Approach),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);

        let disconnected = manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;
        assert!(disconnected.is_empty());

        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOWW_APP")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(station_ids.contains(&"LOWW_APP"));
        assert!(
            !station_ids.contains(&"LOWW_TWR"),
            "LOWW_TWR should not be listed (VATSIM-only)"
        );
        // LOWW_GND and LOWW_DEL are children of LOWW_TWR, now covered by VATSIM-only LOWW_TWR
        assert!(
            !station_ids.contains(&"LOWW_GND"),
            "LOWW_GND should not be listed (covered by VATSIM-only LOWW_TWR)"
        );
        assert!(
            !station_ids.contains(&"LOWW_DEL"),
            "LOWW_DEL should not be listed (covered by VATSIM-only LOWW_TWR)"
        );

        // But internally, LOWW_TWR station should be tracked in online_stations
        let internal_stations = manager.online_stations.read().await;
        assert!(internal_stations.contains_key(&station("LOWW_TWR")));
        drop(internal_stations);

        // Client should receive Offline for the stations that became vatsim-only
        // (LOWW_APP stays online - still covered by vacs LOWW_APP position)
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );
    }

    #[tokio::test]
    async fn vatsim_only_position_becomes_vacs_when_client_connects() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR (covers everything including LOWW_APP,
        // LOWW_TWR, etc.)
        let (_client, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);

        // LOWW_TWR comes online on VATSIM only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // LOWW_TWR station is NOT callable (VATSIM-only)
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(!station_ids.contains(&"LOWW_TWR"));

        // CTR client should have received Offline for stations that became VATSIM-only
        let changes_after_sync = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_sync,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );

        // Now a vacs client connects as LOWW_TWR
        let _client_twr = manager
            .add_client(
                client_info("client2", "LOWW_TWR", "119.400"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_TWR should now be in the list (vacs client covers it)
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(
            station_ids.contains(&"LOWW_TWR"),
            "LOWW_TWR should be listed after vacs client connects"
        );

        // CTR client should receive Online for stations that transitioned from
        // VATSIM-only to vacs coverage.
        let changes_after_connect = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_connect,
            vec![
                StationChange::Online {
                    station_id: station("LOWW_DEL"),
                    position_id: pos("LOWW_TWR"),
                },
                StationChange::Online {
                    station_id: station("LOWW_GND"),
                    position_id: pos("LOWW_TWR"),
                },
                StationChange::Online {
                    station_id: station("LOWW_TWR"),
                    position_id: pos("LOWW_TWR"),
                },
            ]
        );
    }

    #[tokio::test]
    async fn vacs_client_disconnect_with_vatsim_only_covering_same_position() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // vacs client connects as LOWW_TWR
        let _client_twr = manager
            .add_client(
                client_info("client2", "LOWW_TWR", "119.400"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);

        // LOWW_TWR is callable
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(station_ids.contains(&"LOWW_TWR"));

        // vacs LOWW_TWR client disconnects
        manager
            .remove_client(cid("client2"), Some(DisconnectReason::Terminated))
            .await;

        // CTR client should see LOWW_TWR come back under its control
        let changes_after_disconnect = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_disconnect,
            vec![
                StationChange::Handoff {
                    station_id: station("LOWW_DEL"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_GND"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_TWR"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
            ]
        );

        // But VATSIM-only LOWW_TWR is still online
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // After sync, LOWW_TWR becomes VATSIM-only → CTR client sees it go Offline
        let changes_after_sync = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_sync,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );

        // LOWW_TWR should NOT be callable (VATSIM-only now)
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(
            !station_ids.contains(&"LOWW_TWR"),
            "LOWW_TWR should not be listed (VATSIM-only after vacs client disconnect)"
        );

        // But LOWW_TWR should still be in internal tracking
        let internal_stations = manager.online_stations.read().await;
        assert!(internal_stations.contains_key(&station("LOWW_TWR")));
    }

    /// Removal: vacs → none. The last vacs client on a position disconnects and
    /// no other position covers the stations. Stations should go Offline.
    #[tokio::test]
    async fn remove_client_vacs_to_none() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // LOWW_DEL is the only position online
        let _client = manager
            .add_client(
                client_info("client0", "LOWW_DEL", "122.125"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Station is online
        assert!(
            manager
                .online_stations
                .read()
                .await
                .contains_key(&station("LOWW_DEL"))
        );

        manager
            .remove_client(cid("client0"), Some(DisconnectReason::Terminated))
            .await;

        // Station should be gone - no vacs position to take over
        assert!(
            manager.online_stations.read().await.is_empty(),
            "All stations should be offline after last position is removed"
        );
        assert!(
            manager.online_positions.read().await.is_empty(),
            "No online positions should remain"
        );
    }

    /// Removal: vacs → vatsim. A vacs client disconnects and a subsequent
    /// VATSIM sync establishes a vatsim-only controller on the same position.
    /// The combined effect is that the station transitions from callable (vacs)
    /// to invisible (vatsim-only).
    #[tokio::test]
    async fn remove_client_vacs_to_vatsim_only() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // LOVV_CTR and LOWW_TWR are online
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        let _client_twr = manager
            .add_client(
                client_info("client1", "LOWW_TWR", "119.400"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);

        // vacs LOWW_TWR client disconnects
        manager
            .remove_client(cid("client1"), Some(DisconnectReason::Terminated))
            .await;

        // Stations hand off from LOWW_TWR to LOVV_CTR (both were vacs positions)
        let changes_after_disconnect = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_disconnect,
            vec![
                StationChange::Handoff {
                    station_id: station("LOWW_DEL"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_GND"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_TWR"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
            ]
        );

        // Now VATSIM sync establishes a VATSIM-only TWR controller
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_twr"),
                controller("vatsim_twr", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // Stations now go Offline for the CTR client (VATSIM-only is invisible)
        let changes_after_sync = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_after_sync,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );

        // Stations tracked internally under VATSIM-only TWR position
        let internal_stations = manager.online_stations.read().await;
        assert_eq!(
            internal_stations.get(&station("LOWW_TWR")),
            Some(&pos("LOWW_TWR")),
            "LOWW_TWR should be tracked internally under vatsim-only position"
        );
    }

    /// Removal: vacs → vacs (multiple clients on same position). When multiple
    /// vacs clients share a position, removing one should NOT produce any
    /// station changes - the position stays online.
    #[tokio::test]
    async fn remove_client_vacs_to_vacs_multiple_on_same_position() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Two clients on LOVV_CTR
        let (_client0, mut rx0) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        let _client1 = manager
            .add_client(
                client_info("client1", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx0);

        // Position should have both clients
        let pos_clients = manager
            .online_positions
            .read()
            .await
            .get(&pos("LOVV_CTR"))
            .cloned()
            .unwrap();
        assert_eq!(pos_clients.len(), 2);

        // Remove one client
        manager
            .remove_client(cid("client1"), Some(DisconnectReason::Terminated))
            .await;

        // No station changes - position is still online
        let changes = drain_messages(&mut rx0).station_changes;
        assert!(
            changes.is_empty(),
            "No station changes expected when a co-client leaves: {changes:?}"
        );

        // Position should still be online with the remaining client
        let pos_clients = manager
            .online_positions
            .read()
            .await
            .get(&pos("LOVV_CTR"))
            .cloned()
            .unwrap();
        assert_eq!(pos_clients, HashSet::from([cid("client0")]));

        // All stations should still be tracked
        assert!(
            !manager.online_stations.read().await.is_empty(),
            "Online stations should still be tracked"
        );
    }

    /// Removal: vacs → vacs (coverage changes). A vacs client on one position
    /// disconnects and stations fall back to a different vacs position. The
    /// remaining client should see Handoff events.
    #[tokio::test]
    async fn remove_client_vacs_to_vacs_coverage_handoff() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // LOVV_CTR and LOWW_TWR are online
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        let _client_twr = manager
            .add_client(
                client_info("client1", "LOWW_TWR", "119.400"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);

        // LOWW_TWR, LOWW_GND, LOWW_DEL are controlled by LOWW_TWR position
        let online = manager.online_stations.read().await;
        assert_eq!(online.get(&station("LOWW_TWR")), Some(&pos("LOWW_TWR")));
        assert_eq!(online.get(&station("LOWW_GND")), Some(&pos("LOWW_TWR")));
        assert_eq!(online.get(&station("LOWW_DEL")), Some(&pos("LOWW_TWR")));
        drop(online);

        // LOWW_TWR client disconnects
        manager
            .remove_client(cid("client1"), Some(DisconnectReason::Terminated))
            .await;

        // CTR client sees Handoff: stations move from TWR position to CTR position
        let changes = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes,
            vec![
                StationChange::Handoff {
                    station_id: station("LOWW_DEL"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_GND"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
                StationChange::Handoff {
                    station_id: station("LOWW_TWR"),
                    from_position_id: pos("LOWW_TWR"),
                    to_position_id: pos("LOVV_CTR"),
                },
            ]
        );

        // Stations should now be controlled by LOVV_CTR
        let online = manager.online_stations.read().await;
        assert_eq!(online.get(&station("LOWW_TWR")), Some(&pos("LOVV_CTR")));
        assert_eq!(online.get(&station("LOWW_GND")), Some(&pos("LOVV_CTR")));
        assert_eq!(online.get(&station("LOWW_DEL")), Some(&pos("LOVV_CTR")));
    }

    #[tokio::test]
    async fn multiple_vatsim_only_positions_not_callable() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // Both LOWW_TWR and LOWW_GND online on VATSIM only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
            (
                cid("vatsim_client2"),
                controller(
                    "vatsim_client2",
                    "LOWW_GND",
                    "121.600",
                    FacilityType::Ground,
                ),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();

        assert!(
            !station_ids.contains(&"LOWW_TWR"),
            "LOWW_TWR should not be callable (vatsim-only)"
        );
        assert!(
            !station_ids.contains(&"LOWW_GND"),
            "LOWW_GND should not be callable (vatsim-only)"
        );
        assert!(
            !station_ids.contains(&"LOWW_DEL"),
            "LOWW_DEL should not be callable (covered by vatsim-only LOWW_GND)"
        );
        // LOWW_APP should still be covered by LOVV_CTR
        assert!(
            station_ids.contains(&"LOWW_APP"),
            "LOWW_APP should still be callable (covered by VACS LOVV_CTR)"
        );

        // Client should receive Offline for all three stations that became VATSIM-only
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );
    }

    #[tokio::test]
    async fn last_client_disconnect_clears_vatsim_only_state() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects
        let _client = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Sync with VATSIM-only TWR
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOWW_APP", "134.675", FacilityType::Approach),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        assert!(!manager.vatsim_only_positions.read().await.is_empty());
        assert!(!manager.online_stations.read().await.is_empty());

        // Last vacs client disconnects
        manager
            .remove_client(cid("client0"), Some(DisconnectReason::Terminated))
            .await;

        assert!(
            manager.vatsim_only_positions.read().await.is_empty(),
            "VATSIM-only positions should be cleared after last client disconnects"
        );
        assert!(
            manager.online_stations.read().await.is_empty(),
            "online stations should be cleared after last client disconnects"
        );
    }

    #[tokio::test]
    async fn clients_for_station_returns_empty_for_vatsim_only() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let _client = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_TWR online VATSIM-only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // LOWW_TWR station exists internally but has no callable clients
        let clients = manager.clients_for_station(&station("LOWW_TWR")).await;
        assert!(
            clients.is_empty(),
            "clients_for_station should return empty for VATSIM-only station"
        );
    }

    #[tokio::test]
    async fn replace_network_removes_stale_position() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Client connects as LOWW_DEL
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_DEL", "122.125"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Drain initial station changes from add_client
        drain_messages(&mut rx);

        assert!(
            manager
                .online_positions
                .read()
                .await
                .contains_key(&pos("LOWW_DEL"))
        );

        // Replace with a network that no longer has LOWW_DEL position
        let new_network = create_lovv_network_without_del(dir.path());
        manager.replace_network(new_network).await;

        // Position should be gone
        assert!(
            !manager
                .online_positions
                .read()
                .await
                .contains_key(&pos("LOWW_DEL")),
            "LOWW_DEL position should be removed after network replace"
        );

        // Client's position_id should be cleared
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.position_id(),
            None,
            "Client's position_id should be None after their position is removed"
        );

        // Client should receive Offline for LOWW_DEL station
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }]
        );
    }

    #[tokio::test]
    async fn replace_network_removes_stale_station() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Client connects as LOVV_CTR which covers all stations
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // LOWW_DEL station should be online
        assert!(
            manager
                .online_stations
                .read()
                .await
                .contains_key(&station("LOWW_DEL"))
        );

        // Replace with network that has no LOWW_DEL station
        let new_network = create_lovv_network_without_del(dir.path());
        manager.replace_network(new_network).await;

        // LOWW_DEL station should be gone
        assert!(
            !manager
                .online_stations
                .read()
                .await
                .contains_key(&station("LOWW_DEL")),
            "LOWW_DEL station should be removed after network replace"
        );

        // Remaining stations (LOWW_APP, LOWW_TWR, LOWW_GND) should still be online
        let online_stations = manager.online_stations.read().await;
        assert!(online_stations.contains_key(&station("LOWW_APP")));
        assert!(online_stations.contains_key(&station("LOWW_TWR")));
        assert!(online_stations.contains_key(&station("LOWW_GND")));
        drop(online_stations);

        // Client should receive exactly Offline for LOWW_DEL
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }],
            "Only LOWW_DEL should go offline"
        );
    }

    #[tokio::test]
    async fn replace_network_adds_new_station() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Client connects as LOVV_CTR
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // No LOVV_N1 station initially
        assert!(
            !manager
                .online_stations
                .read()
                .await
                .contains_key(&station("LOVV_N1"))
        );

        // Replace with network that adds LOVV_N1 station controlled by LOVV_CTR
        let new_network = create_lovv_network_with_extra_station(dir.path());
        manager.replace_network(new_network).await;

        // LOVV_N1 should now be online, controlled by LOVV_CTR
        let online_stations = manager.online_stations.read().await;
        assert_eq!(
            online_stations.get(&station("LOVV_N1")),
            Some(&pos("LOVV_CTR")),
            "LOVV_N1 should be online and controlled by LOVV_CTR"
        );
        drop(online_stations);

        // Client should receive Online for LOVV_N1
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![StationChange::Online {
                station_id: station("LOVV_N1"),
                position_id: pos("LOVV_CTR"),
            }]
        );
    }

    #[tokio::test]
    async fn replace_network_updates_station_controller() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // LOVV_CTR connects, covers all stations
        let _client_ctr = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_APP should be controlled by LOVV_CTR (only online position)
        assert_eq!(
            manager
                .online_stations
                .read()
                .await
                .get(&station("LOWW_APP")),
            Some(&pos("LOVV_CTR"))
        );

        // Now LOWW_APP also connects
        let _client_app = manager
            .add_client(
                client_info("client1", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_APP station should now be controlled by LOWW_APP position
        // (higher priority in controlled_by list)
        assert_eq!(
            manager
                .online_stations
                .read()
                .await
                .get(&station("LOWW_APP")),
            Some(&pos("LOWW_APP"))
        );

        // Replace with same network structure - controllers should remain
        let new_network = Network::load_from_dir(dir.path()).unwrap();
        manager.replace_network(new_network).await;

        // Station controller assignments should be preserved
        let online = manager.online_stations.read().await;
        assert_eq!(
            online.get(&station("LOWW_APP")),
            Some(&pos("LOWW_APP")),
            "LOWW_APP should still be controlled by LOWW_APP position after no-op reload"
        );
    }

    #[tokio::test]
    async fn replace_network_cleans_vatsim_only_positions() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let _client = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_TWR comes online on VATSIM only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        assert!(
            manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should be in vatsim_only"
        );

        // Replace with network that no longer has LOWW_TWR position
        let new_network = create_lovv_network_without_del(dir.path());

        // Verify LOWW_TWR position actually doesn't exist in the new network
        assert!(
            new_network.get_position(&pos("LOWW_TWR")).is_some(),
            "LOWW_TWR position should still exist in the reduced network"
        );

        manager.replace_network(new_network).await;

        // LOWW_TWR position still exists (we only removed DEL), so it stays
        // in vatsim_only
        assert!(
            manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should still be in vatsim_only (position still exists)"
        );
    }

    #[tokio::test]
    async fn replace_network_removes_nonexistent_vatsim_only() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let _client = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOWW_DEL comes online on VATSIM only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller(
                    "vatsim_client1",
                    "LOWW_DEL",
                    "122.125",
                    FacilityType::Delivery,
                ),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        assert!(
            manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOWW_DEL"))
        );

        // Replace with network that no longer has LOWW_DEL position
        let new_network = create_lovv_network_without_del(dir.path());
        assert!(
            new_network.get_position(&pos("LOWW_DEL")).is_none(),
            "Precondition: LOWW_DEL should not exist in the new network"
        );
        manager.replace_network(new_network).await;

        // LOWW_DEL should be cleaned from vatsim_only
        assert!(
            !manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOWW_DEL")),
            "LOWW_DEL should be removed from vatsim_only after network replace"
        );
    }

    #[tokio::test]
    async fn replace_network_with_profile_reassignment() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let network = create_lovv_network_with_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects as LOWW_APP with profile APP_PROFILE
        let _client = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Verify initial profile
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
        );

        // Replace network where LOWW_APP's profile_id changes to CTR_PROFILE
        let new_network = create_lovv_network_with_reassigned_profile(dir.path());
        manager.replace_network(new_network).await;

        // Client's active profile should now be CTR_PROFILE
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::Specific(ProfileId::from("CTR_PROFILE")),
            "Client's profile should be updated to CTR_PROFILE after network reload"
        );
    }

    #[tokio::test]
    async fn replace_network_custom_profile_stays_custom() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let network = create_lovv_network_with_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects with Custom profile (user's own selection)
        let _client = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Replace network where LOWW_APP's profile changes
        let new_network = create_lovv_network_with_reassigned_profile(dir.path());
        manager.replace_network(new_network).await;

        // Client's profile should remain Custom
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::Custom,
            "Client with Custom profile should remain Custom after network reload"
        );
    }

    #[tokio::test]
    async fn replace_network_profile_cleared_sends_session_update() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let network = create_lovv_network_with_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects as LOWW_APP with Specific(APP_PROFILE)
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Drain initial messages
        drain_messages(&mut rx);

        // Reload with a network where LOWW_APP no longer has a profile_id
        let new_network = create_lovv_network_without_profiles(dir.path());
        manager.replace_network(new_network).await;

        // Client's internal state should be cleared to None
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::None,
            "Client's profile should be cleared after position loses its profile_id"
        );

        // Client should have received a SessionInfo with Changed(None)
        let session_infos = drain_messages(&mut rx).session_infos;
        assert_eq!(session_infos.len(), 1, "Exactly one SessionInfo expected");
        assert_eq!(
            session_infos[0].profile,
            SessionProfile::Changed(ActiveProfile::None),
            "Client should be told profile was cleared"
        );
    }

    #[tokio::test]
    async fn replace_network_same_profile_id_content_changed() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let network = create_lovv_network_with_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects as LOWW_APP with Specific(APP_PROFILE)
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // Reload with same profile ID but different tab content
        let new_network = create_lovv_network_with_modified_profile_content(dir.path());
        manager.replace_network(new_network).await;

        // Internal profile ID should stay the same
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
        );

        // Client should receive the updated profile content
        let session_infos = drain_messages(&mut rx).session_infos;
        assert_eq!(session_infos.len(), 1, "Exactly one SessionInfo expected");
        match &session_infos[0].profile {
            SessionProfile::Changed(ActiveProfile::Specific(profile)) => {
                assert_eq!(profile.id, ProfileId::from("APP_PROFILE"));
                // The modified profile has a different tab label
                match &profile.profile_type {
                    vacs_protocol::profile::ProfileType::Tabbed(tabs) => {
                        assert_eq!(tabs[0].label, vec!["Updated"]);
                    }
                    other => panic!("Expected Tabbed profile, got: {other:?}"),
                }
            }
            other => panic!("Expected Changed(Specific(...)), got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_network_none_profile_not_notified() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let network = create_lovv_network_with_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects as LOWW_TWR which has no profile (ActiveProfile::None)
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_TWR", "119.400"),
                ActiveProfile::None,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // Reload with modified content (doesn't matter, LOWW_TWR has no profile)
        let new_network = create_lovv_network_with_modified_profile_content(dir.path());
        manager.replace_network(new_network).await;

        // Client should still have None profile
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(client.active_profile(), &ActiveProfile::None);

        // No SessionInfo should have been sent
        let session_infos = drain_messages(&mut rx).session_infos;
        assert!(
            session_infos.is_empty(),
            "Client with None profile should not receive SessionInfo after reload"
        );
    }

    #[tokio::test]
    async fn replace_network_none_to_specific_sends_profile() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        // Initial network: LOWW_APP has no profile_id
        let network = create_lovv_network_without_profiles(dir.path());
        let manager = client_manager(network);

        // Client connects as LOWW_APP with ActiveProfile::None
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::None,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // Reload with a network where LOWW_APP now has a profile
        let new_network = create_lovv_network_with_profiles(dir.path());
        manager.replace_network(new_network).await;

        // Client's internal state should now be Specific
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            client.active_profile(),
            &ActiveProfile::Specific(ProfileId::from("APP_PROFILE")),
            "Client's profile should transition from None to Specific after reload"
        );

        // Client should have received a SessionInfo with the new profile
        let session_infos = drain_messages(&mut rx).session_infos;
        assert_eq!(session_infos.len(), 1, "Exactly one SessionInfo expected");
        match &session_infos[0].profile {
            SessionProfile::Changed(ActiveProfile::Specific(profile)) => {
                assert_eq!(profile.id, ProfileId::from("APP_PROFILE"));
            }
            other => panic!("Expected Changed(Specific(...)), got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_network_no_change_is_noop() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Client connects as LOVV_CTR
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        let stations_before = manager.online_stations.read().await.clone();
        let positions_before = manager.online_positions.read().await.clone();

        // Reload the exact same network
        let same_network = Network::load_from_dir(dir.path()).unwrap();
        manager.replace_network(same_network).await;

        // Everything should be unchanged
        assert_eq!(
            *manager.online_stations.read().await,
            stations_before,
            "Online stations should be unchanged after no-op reload"
        );
        assert_eq!(
            *manager.online_positions.read().await,
            positions_before,
            "Online positions should be unchanged after no-op reload"
        );

        // Client should still have their position
        let client = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(client.position_id(), Some(&pos("LOVV_CTR")));

        // No station changes should be sent
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![],
            "No station changes should be sent for no-op reload"
        );
    }

    #[tokio::test]
    async fn replace_network_station_coverage_shift() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Two clients online: LOVV_CTR and LOWW_APP
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();
        let (_client_app, mut rx_app) = manager
            .add_client(
                client_info("client1", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);
        drain_messages(&mut rx_app);

        // LOWW_APP station controlled by LOWW_APP position
        assert_eq!(
            manager
                .online_stations
                .read()
                .await
                .get(&station("LOWW_APP")),
            Some(&pos("LOWW_APP"))
        );

        // Replace with network where LOWW_APP station's controlled_by only
        // lists LOVV_CTR (removing LOWW_APP from the list)
        let fir_path = dir.path().join("LOVV");
        std::fs::write(
            fir_path.join("stations.toml"),
            r#"
[[stations]]
id = "LOWW_APP"
controlled_by = ["LOVV_CTR"]

[[stations]]
id = "LOWW_TWR"
parent_id = "LOWW_APP"
controlled_by = ["LOWW_TWR"]

[[stations]]
id = "LOWW_GND"
parent_id = "LOWW_TWR"
controlled_by = ["LOWW_GND"]

[[stations]]
id = "LOWW_DEL"
parent_id = "LOWW_GND"
controlled_by = ["LOWW_DEL"]
"#,
        )
        .unwrap();
        let new_network = Network::load_from_dir(dir.path()).unwrap();
        manager.replace_network(new_network).await;

        // LOWW_APP station should now be controlled by LOVV_CTR
        assert_eq!(
            manager
                .online_stations
                .read()
                .await
                .get(&station("LOWW_APP")),
            Some(&pos("LOVV_CTR")),
            "LOWW_APP station should shift to LOVV_CTR after controlled_by change"
        );

        // Both clients should receive Handoffs for all stations moving from
        // LOWW_APP to LOVV_CTR (LOWW_APP position was removed from the network)
        let expected = vec![
            StationChange::Handoff {
                station_id: station("LOWW_APP"),
                from_position_id: pos("LOWW_APP"),
                to_position_id: pos("LOVV_CTR"),
            },
            StationChange::Handoff {
                station_id: station("LOWW_DEL"),
                from_position_id: pos("LOWW_APP"),
                to_position_id: pos("LOVV_CTR"),
            },
            StationChange::Handoff {
                station_id: station("LOWW_GND"),
                from_position_id: pos("LOWW_APP"),
                to_position_id: pos("LOVV_CTR"),
            },
            StationChange::Handoff {
                station_id: station("LOWW_TWR"),
                from_position_id: pos("LOWW_APP"),
                to_position_id: pos("LOVV_CTR"),
            },
        ];
        let changes_ctr = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(changes_ctr, expected, "LOVV_CTR client");
        let changes_app = drain_messages(&mut rx_app).station_changes;
        assert_eq!(changes_app, expected, "LOWW_APP client");
    }

    #[tokio::test]
    async fn replace_network_vatsim_only_position_removed_stations_become_visible() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOVV_CTR
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx);

        // LOWW_TWR comes online as VATSIM-only
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
            ),
            (
                cid("vatsim_client1"),
                controller("vatsim_client1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // Client received Offline for LOWW_TWR/GND/DEL (now VATSIM-only)
        let changes_after_sync = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes_after_sync,
            vec![
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ]
        );

        // Replace with network that removes LOWW_TWR position entirely
        // → VATSIM-only LOWW_TWR gets cleaned, stations fall back to LOVV_CTR
        let new_network = create_lovv_network_without_twr_position(dir.path());
        manager.replace_network(new_network).await;

        // LOWW_TWR should be removed from vatsim_only
        assert!(
            !manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should be removed from vatsim_only"
        );

        // Stations should now be visible again under LOVV_CTR
        let stations = manager
            .list_stations(&ActiveProfile::Custom, Some(&pos("LOVV_CTR")))
            .await;
        let station_ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        assert!(station_ids.contains(&"LOWW_TWR"));
        assert!(station_ids.contains(&"LOWW_GND"));
        assert!(station_ids.contains(&"LOWW_DEL"));

        // Client should receive Online for the stations that became visible again
        let changes = drain_messages(&mut rx).station_changes;
        assert_eq!(
            changes,
            vec![
                StationChange::Online {
                    station_id: station("LOWW_DEL"),
                    position_id: pos("LOVV_CTR"),
                },
                StationChange::Online {
                    station_id: station("LOWW_GND"),
                    position_id: pos("LOVV_CTR"),
                },
                StationChange::Online {
                    station_id: station("LOWW_TWR"),
                    position_id: pos("LOVV_CTR"),
                },
            ]
        );
    }

    #[tokio::test]
    async fn replace_network_multiple_clients_on_stale_position() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Two clients connect on the same position LOWW_DEL
        let (_client0, mut rx0) = manager
            .add_client(
                client_info("client0", "LOWW_DEL", "122.125"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();
        let (_client1, mut rx1) = manager
            .add_client(
                client_info("client1", "LOWW_DEL", "122.125"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx0);
        drain_messages(&mut rx1);

        // Verify both are on the position
        let pos_clients = manager
            .online_positions
            .read()
            .await
            .get(&pos("LOWW_DEL"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(pos_clients.len(), 2);

        // Replace with network that removes LOWW_DEL position
        let new_network = create_lovv_network_without_del(dir.path());
        manager.replace_network(new_network).await;

        // Position should be gone
        assert!(
            !manager
                .online_positions
                .read()
                .await
                .contains_key(&pos("LOWW_DEL")),
        );

        // Both clients should have their position_id cleared
        let c0 = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(c0.position_id(), None, "client0 position should be cleared");
        let c1 = manager.get_client(&cid("client1")).await.unwrap();
        assert_eq!(c1.position_id(), None, "client1 position should be cleared");

        // Both should receive Offline for LOWW_DEL
        let changes0 = drain_messages(&mut rx0).station_changes;
        assert_eq!(
            changes0,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }],
            "client0"
        );
        let changes1 = drain_messages(&mut rx1).station_changes;
        assert_eq!(
            changes1,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }],
            "client1"
        );
    }

    #[tokio::test]
    async fn add_client_vatsim_only_position_not_controlling_any_station() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // vacs client connects as LOWW_APP (covers LOWW_APP, LOWW_TWR, LOWW_GND, LOWW_DEL)
        let (_client_app, mut rx_app) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_app);

        // LOVV_CTR comes online as VATSIM-only. It would cover LOWW_APP station
        // via controlled_by, but LOWW_APP position has higher priority, so
        // LOVV_CTR controls no stations.
        let vatsim_controllers = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOWW_APP", "134.675", FacilityType::Approach),
            ),
            (
                cid("vatsim_client1"),
                controller(
                    "vatsim_client1",
                    "LOVV_CTR",
                    "132.600",
                    FacilityType::Enroute,
                ),
            ),
        ]);
        manager
            .sync_vatsim_state(&vatsim_controllers, &mut HashSet::new(), false)
            .await;

        // No station changes - LOVV_CTR is VATSIM-only but controls nothing
        // (all stations already covered by higher-priority LOWW_APP)
        let changes_after_sync = drain_messages(&mut rx_app).station_changes;
        assert_eq!(changes_after_sync, vec![], "No station changes expected");

        // Now a vacs client connects as LOVV_CTR (was VATSIM-only)
        let (_client_ctr, _rx_ctr) = manager
            .add_client(
                client_info("client1", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // LOVV_CTR was vatsim-only but controlled no stations, so the
        // transition shouldn't produce any Online events for the APP client
        let changes_after_connect = drain_messages(&mut rx_app).station_changes;
        assert_eq!(
            changes_after_connect,
            vec![],
            "No Online events expected - LOVV_CTR controls no stations while LOWW_APP is online"
        );

        // LOVV_CTR should no longer be in vatsim_only
        assert!(
            !manager
                .vatsim_only_positions
                .read()
                .await
                .contains_key(&pos("LOVV_CTR")),
            "LOVV_CTR should be removed from vatsim_only"
        );
    }

    #[tokio::test]
    async fn replace_network_reduces_to_minimal() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Two clients on different positions
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();
        let (_client_app, mut rx_app) = manager
            .add_client(
                client_info("client1", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);
        drain_messages(&mut rx_app);

        // Verify we have stations and positions
        assert!(!manager.online_stations.read().await.is_empty());
        assert!(!manager.online_positions.read().await.is_empty());

        // Replace with minimal network: only LOVV_CTR position + LOWW_APP station
        let new_network = create_minimal_lovv_network(dir.path());
        manager.replace_network(new_network).await;

        // LOWW_APP position should be gone (doesn't exist in new network)
        assert!(
            !manager
                .online_positions
                .read()
                .await
                .contains_key(&pos("LOWW_APP")),
        );

        // LOVV_CTR position should still exist (it's in the new network)
        assert!(
            manager
                .online_positions
                .read()
                .await
                .contains_key(&pos("LOVV_CTR")),
        );

        // Only LOWW_APP station should remain
        let stations = manager.online_stations.read().await;
        assert_eq!(stations.len(), 1, "Only LOWW_APP station should remain");
        assert!(stations.contains_key(&station("LOWW_APP")));
        drop(stations);

        // LOWW_APP client's position should be cleared (position doesn't exist)
        let c0 = manager.get_client(&cid("client0")).await.unwrap();
        assert_eq!(
            c0.position_id(),
            Some(&pos("LOVV_CTR")),
            "LOVV_CTR still exists in new network"
        );
        let c1 = manager.get_client(&cid("client1")).await.unwrap();
        assert_eq!(
            c1.position_id(),
            None,
            "LOWW_APP position doesn't exist in new network"
        );

        // CTR client: LOWW_TWR/GND/DEL go Offline (removed stations),
        // LOWW_APP transitions LOWW_APP→LOVV_CTR but since LOWW_APP position
        // is gone (removed as stale), client_visible_changes sees it as Online.
        let changes_ctr = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_ctr,
            vec![
                StationChange::Online {
                    station_id: station("LOWW_APP"),
                    position_id: pos("LOVV_CTR"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ],
            "CTR client"
        );

        // APP client: same changes
        let changes_app = drain_messages(&mut rx_app).station_changes;
        assert_eq!(
            changes_app,
            vec![
                StationChange::Online {
                    station_id: station("LOWW_APP"),
                    position_id: pos("LOVV_CTR"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_DEL"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_GND"),
                },
                StationChange::Offline {
                    station_id: station("LOWW_TWR"),
                },
            ],
            "APP client"
        );
    }

    #[tokio::test]
    async fn replace_network_client_without_position_unaffected() {
        let (dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // Position-holding client
        let (_client_ctr, mut rx_ctr) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        // Client without a position (e.g. position lookup yielded no match)
        let (_client_nopos, mut rx_nopos) = manager
            .add_client(
                client_info_without_position("nopos0"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        drain_messages(&mut rx_ctr);
        drain_messages(&mut rx_nopos);

        // Verify client has no position
        let nopos = manager.get_client(&cid("nopos0")).await.unwrap();
        assert_eq!(nopos.position_id(), None);

        // Replace with network that removes LOWW_DEL
        let new_network = create_lovv_network_without_del(dir.path());
        manager.replace_network(new_network).await;

        // No-position client should still be connected with no position
        let nopos = manager.get_client(&cid("nopos0")).await.unwrap();
        assert_eq!(nopos.position_id(), None, "Position should remain None");

        // CTR client should receive Offline for LOWW_DEL
        let changes_ctr = drain_messages(&mut rx_ctr).station_changes;
        assert_eq!(
            changes_ctr,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }]
        );

        // No-position client should also receive Offline for LOWW_DEL
        // (they see all stations via Custom profile)
        let changes_nopos = drain_messages(&mut rx_nopos).station_changes;
        assert_eq!(
            changes_nopos,
            vec![StationChange::Offline {
                station_id: station("LOWW_DEL"),
            }],
            "No-position client should receive station changes too"
        );
    }

    /// Base builder for the standard LOVV FIR used by most tests.
    fn lovv_fir() -> TestFirBuilder {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .position("LOWW_APP", &["LOWW"], "134.675", "APP")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
    }

    /// Standard LOVV network (5 positions, 5 stations). Returns the temp-dir
    /// so the caller can pass it to variants that only rewrite positions/stations.
    fn create_lovv_network() -> (tempfile::TempDir, Network) {
        let dir = tempfile::tempdir().unwrap();
        let network = lovv_fir().build(dir.path());
        (dir, network)
    }

    /// LOVV without the LOWW_DEL position *and* station.
    fn create_lovv_network_without_del(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .position("LOWW_APP", &["LOWW"], "134.675", "APP")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .build(dir)
    }

    /// LOVV with profiles assigned to CTR and APP positions.
    fn create_lovv_network_with_profiles(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position_with_profile("LOVV_CTR", &["LOVV"], "132.600", "CTR", "CTR_PROFILE")
            .position_with_profile("LOWW_APP", &["LOWW"], "134.675", "APP", "APP_PROFILE")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .tabbed_profile(
                "CTR_PROFILE",
                &[("LOWW APP", "LOWW_APP"), ("LOWW TWR", "LOWW_TWR")],
            )
            .tabbed_profile(
                "APP_PROFILE",
                &[("LOWW TWR", "LOWW_TWR"), ("LOWW GND", "LOWW_GND")],
            )
            .build(dir)
    }

    /// LOVV with LOWW_APP's profile reassigned to CTR_PROFILE.
    /// Only rewrites positions.toml - stations and profiles remain from a
    /// previous `create_lovv_network_with_profiles` call.
    fn create_lovv_network_with_reassigned_profile(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .position_with_profile("LOVV_CTR", &["LOVV"], "132.600", "CTR", "CTR_PROFILE")
            .position_with_profile("LOWW_APP", &["LOWW"], "134.675", "APP", "CTR_PROFILE")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .build(dir)
    }

    /// LOVV with an extra LOVV_N1 station controlled by LOVV_CTR.
    fn create_lovv_network_with_extra_station(dir: &std::path::Path) -> Network {
        lovv_fir().station("LOVV_N1", &["LOVV_CTR"]).build(dir)
    }

    /// Creates a network without the LOWW_TWR position.
    /// LOWW_TWR *station* remains (falls back to parent LOWW_APP).
    fn create_lovv_network_without_twr_position(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .position("LOWW_APP", &["LOWW"], "134.675", "APP")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .build(dir)
    }

    /// Creates a minimal network with only LOVV_CTR position and one station.
    fn create_minimal_lovv_network(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOVV_CTR"])
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .build(dir)
    }

    /// LOVV with the same stations/positions as `create_lovv_network_with_profiles`
    /// but positions no longer carry a profile_id.
    fn create_lovv_network_without_profiles(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .position("LOWW_APP", &["LOWW"], "134.675", "APP")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .build(dir)
    }

    /// LOVV with profiles, but APP_PROFILE has different tab content (label
    /// changed from "Main" to "Updated") to simulate a profile content change
    /// under the same ID.
    fn create_lovv_network_with_modified_profile_content(dir: &std::path::Path) -> Network {
        TestFirBuilder::new("LOVV")
            .station("LOWW_APP", &["LOWW_APP", "LOVV_CTR"])
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position_with_profile("LOVV_CTR", &["LOVV"], "132.600", "CTR", "CTR_PROFILE")
            .position_with_profile("LOWW_APP", &["LOWW"], "134.675", "APP", "APP_PROFILE")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .tabbed_profile(
                "CTR_PROFILE",
                &[("LOWW APP", "LOWW_APP"), ("LOWW TWR", "LOWW_TWR")],
            )
            .tabbed_profile_with_label(
                "APP_PROFILE",
                "Updated",
                &[("LOWW TWR", "LOWW_TWR"), ("LOWW GND", "LOWW_GND")],
            )
            .build(dir)
    }

    #[tokio::test]
    async fn vatsim_only_round_trip_in_single_sync() {
        let (_dir, network) = create_lovv_network();
        let manager = client_manager(network);

        // client0 connects as LOWW_APP (vacs)
        let (_client, mut rx) = manager
            .add_client(
                client_info("client0", "LOWW_APP", "134.675"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();
        drain_messages(&mut rx);

        // First sync: establish LOWW_TWR as vatsim-only
        let controllers1 = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOWW_APP", "134.675", FacilityType::Approach),
            ),
            (
                cid("vatsim1"),
                controller("vatsim1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
        ]);
        manager
            .sync_vatsim_state(&controllers1, &mut HashSet::new(), false)
            .await;
        drain_messages(&mut rx);

        let vatsim_only = manager.vatsim_only_positions.read().await;
        assert!(
            vatsim_only.contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should be vatsim-only"
        );
        assert!(
            !vatsim_only.contains_key(&pos("LOWW_APP")),
            "LOWW_APP should not be vatsim-only"
        );
        drop(vatsim_only);

        // Second sync: client0 moves from LOWW_APP to LOWW_TWR, and a new
        // VATSIM controller takes LOWW_APP. In one sync cycle LOWW_TWR goes
        // from VATSIM-only → vacs and LOWW_APP goes from vacs → VATSIM-only.
        let controllers2 = HashMap::from([
            (
                cid("client0"),
                controller("client0", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
            (
                cid("vatsim1"),
                controller("vatsim1", "LOWW_TWR", "119.400", FacilityType::Tower),
            ),
            (
                cid("vatsim2"),
                controller("vatsim2", "LOWW_APP", "134.675", FacilityType::Approach),
            ),
        ]);
        let disconnected = manager
            .sync_vatsim_state(&controllers2, &mut HashSet::new(), false)
            .await;
        assert!(disconnected.is_empty());

        // LOWW_TWR should now be a vacs position
        let online_positions = manager.online_positions.read().await;
        assert!(
            online_positions.contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should be a vacs position"
        );
        assert!(
            !online_positions.contains_key(&pos("LOWW_APP")),
            "LOWW_APP should no longer be a vacs position"
        );
        drop(online_positions);

        // LOWW_APP should now be vatsim-only
        let vatsim_only = manager.vatsim_only_positions.read().await;
        assert!(
            vatsim_only.contains_key(&pos("LOWW_APP")),
            "LOWW_APP should be vatsim-only"
        );
        assert!(
            !vatsim_only.contains_key(&pos("LOWW_TWR")),
            "LOWW_TWR should not be vatsim-only (it's vacs)"
        );
        drop(vatsim_only);
    }

    #[tokio::test]
    async fn concurrent_add_client_and_sync_does_not_deadlock() {
        let (_dir, network) = create_lovv_network();
        let manager = std::sync::Arc::new(client_manager(network));

        // Pre-connect one client so the manager isn't empty during sync
        let (_client, _rx) = manager
            .add_client(
                client_info("client0", "LOVV_CTR", "132.600"),
                ActiveProfile::Custom,
                ClientConnectionGuard::default(),
            )
            .await
            .unwrap();

        let m1 = manager.clone();
        let m2 = manager.clone();

        // Run add_client and sync_vatsim_state concurrently via tokio::join!.
        // The test passes if neither side deadlocks and no panic occurs.
        let (add_result, _disconnected) = tokio::join!(
            async move {
                m1.add_client(
                    client_info("client1", "LOWW_APP", "134.675"),
                    ActiveProfile::Custom,
                    ClientConnectionGuard::default(),
                )
                .await
            },
            async move {
                let controllers = HashMap::from([(
                    cid("client0"),
                    controller("client0", "LOVV_CTR", "132.600", FacilityType::Enroute),
                )]);
                m2.sync_vatsim_state(&controllers, &mut HashSet::new(), false)
                    .await
            }
        );

        assert!(add_result.is_ok());
    }

    // Scenario-based sync tests
    //
    // Place JSON scenario files in:
    //   vacs-server/tests/fixtures/scenarios/
    //
    // Each file describes a sequence of steps (connect clients, apply
    // VATSIM datafeed dumps, assert network state). The runner below
    // discovers and executes every *.json file in that directory
    // (recursively), excluding the `feeds/` and `datasets/`
    // sub-directories.
    //
    // # Network sources
    //
    // Use `"network": "lovv"` for the built-in synthetic test network,
    // or `"dataset": "tests/fixtures/scenarios/datasets/LO"`
    // to load a committed dataset directory (relative to CARGO_MANIFEST_DIR).
    //
    // See existing files for the full format.

    mod scenario {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde::Deserialize;
        use std::collections::{HashMap, HashSet};
        use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
        use std::path::{Path, PathBuf};

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub struct Scenario {
            pub description: String,
            /// Named synthetic network (e.g. "lovv"). Mutually exclusive with `dataset`.
            #[serde(default)]
            pub network: Option<String>,
            /// Path to a committed dataset directory, relative to CARGO_MANIFEST_DIR.
            /// The directory must be loadable by `Network::load_from_dir`
            /// (containing FIR sub-directories with positions/stations/profiles).
            #[serde(default)]
            pub dataset: Option<String>,
            pub steps: Vec<Step>,
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Step {
            Connect(ConnectStep),
            ConnectWithoutPosition(ConnectWithoutPositionStep),
            Disconnect(DisconnectStep),
            Datafeed(DatafeedStep),
            DatafeedFile(String),
            DrainMessages(DrainMessagesStep),
            AssertCallableStations(AssertCallableStationsStep),
            AssertStationChanges(AssertStationChangesStep),
            AssertVatsimOnlyPositions(AssertVatsimOnlyPositionsStep),
            AssertOnlineStations(AssertOnlineStationsStep),
            AssertOnlinePositions(AssertOnlinePositionsStep),
            AssertClientCount(usize),
            /// Ignored by the runner - use for inline documentation.
            #[serde(rename = "_comment")]
            #[allow(dead_code)]
            Comment(serde_json::Value),
        }

        #[derive(Debug, Deserialize)]
        pub struct ConnectStep {
            pub client_id: String,
            pub position_id: String,
            pub frequency: String,
        }

        #[derive(Debug, Deserialize)]
        pub struct ConnectWithoutPositionStep {
            pub client_id: String,
        }

        #[derive(Debug, Deserialize)]
        pub struct DisconnectStep {
            pub client_id: String,
        }

        #[derive(Debug, Deserialize)]
        pub struct DatafeedStep {
            pub controllers: Vec<DatafeedController>,
        }

        /// Mirrors the VATSIM V3 datafeed format.
        /// `facility` is optional - when absent the facility type is
        /// inferred from the callsign suffix, just like production.
        #[derive(Debug, Deserialize)]
        pub struct DatafeedController {
            pub cid: serde_json::Value,
            pub callsign: String,
            pub frequency: String,
            #[serde(default)]
            pub facility: Option<u8>,
        }

        impl DatafeedController {
            pub fn to_controller_info(&self) -> (ClientId, ControllerInfo) {
                let cid = match &self.cid {
                    serde_json::Value::Number(n) => ClientId::from(n.to_string()),
                    serde_json::Value::String(s) => ClientId::from(s.clone()),
                    other => panic!("Unexpected cid type: {other:?}"),
                };
                let facility_type = match self.facility {
                    Some(f) => FacilityType::from_vatsim_facility(f),
                    None => FacilityType::from(self.callsign.as_str()),
                };
                let info = ControllerInfo {
                    cid: cid.clone(),
                    callsign: self.callsign.clone(),
                    frequency: self.frequency.clone(),
                    facility_type,
                };
                (cid, info)
            }
        }

        /// When loading a full VATSIM datafeed JSON file, only the
        /// `controllers` array is used. Other top-level keys are ignored.
        #[derive(Debug, Deserialize)]
        struct DatafeedFile {
            controllers: Vec<DatafeedController>,
        }

        #[derive(Debug, Deserialize)]
        pub struct DrainMessagesStep {
            pub client_id: String,
        }

        #[derive(Debug, Deserialize)]
        pub struct AssertCallableStationsStep {
            pub client_id: String,
            /// Stations that must be callable AND owned by this client's position.
            #[serde(default)]
            pub own: Vec<String>,
            /// Stations that must be callable but covered by another position.
            #[serde(default)]
            pub callable: Vec<String>,
            /// Stations that must NOT appear in the callable list at all.
            #[serde(default)]
            pub not_callable: Vec<String>,
        }

        #[derive(Debug, Deserialize)]
        pub struct AssertStationChangesStep {
            pub client_id: String,
            pub changes: Vec<StationChangeJson>,
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum StationChangeJson {
            Online {
                station_id: String,
                position_id: String,
            },
            Offline {
                station_id: String,
            },
            Handoff {
                station_id: String,
                from_position_id: String,
                to_position_id: String,
            },
        }

        impl From<&StationChangeJson> for StationChange {
            fn from(value: &StationChangeJson) -> Self {
                match value {
                    StationChangeJson::Online {
                        station_id,
                        position_id,
                    } => StationChange::Online {
                        station_id: StationId::from(station_id.clone()),
                        position_id: PositionId::from(position_id.clone()),
                    },
                    StationChangeJson::Offline { station_id } => StationChange::Offline {
                        station_id: StationId::from(station_id.clone()),
                    },
                    StationChangeJson::Handoff {
                        station_id,
                        from_position_id,
                        to_position_id,
                    } => StationChange::Handoff {
                        station_id: StationId::from(station_id.clone()),
                        from_position_id: PositionId::from(from_position_id.clone()),
                        to_position_id: PositionId::from(to_position_id.clone()),
                    },
                }
            }
        }

        #[derive(Debug, Deserialize)]
        pub struct AssertVatsimOnlyPositionsStep {
            pub positions: Vec<String>,
        }

        #[derive(Debug, Deserialize)]
        pub struct AssertOnlineStationsStep {
            /// Station IDs that must be present in online_stations.
            #[serde(default)]
            pub online: Vec<String>,
            /// Station IDs that must NOT be present in online_stations.
            #[serde(default)]
            pub offline: Vec<String>,
        }

        #[derive(Debug, Deserialize)]
        pub struct AssertOnlinePositionsStep {
            /// Position IDs that must have connected vacs clients.
            #[serde(default)]
            pub online: Vec<String>,
            /// Position IDs that must NOT appear in online_positions.
            #[serde(default)]
            pub not_online: Vec<String>,
        }

        struct ScenarioContext {
            manager: ClientManager,
            receivers: HashMap<String, mpsc::Receiver<ServerMessage>>,
            pending_disconnect: HashSet<ClientId>,
            /// Present when using a synthetic network (tempdir-backed).
            _dir: Option<tempfile::TempDir>,
        }

        fn build_synthetic_network(name: &str) -> (tempfile::TempDir, Network) {
            match name {
                "lovv" => create_lovv_network(),
                other => panic!(
                    "Unknown synthetic network: {other}. Add it to scenario::build_synthetic_network()."
                ),
            }
        }

        fn load_dataset_network(relative_path: &str) -> Network {
            let dataset_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
            assert!(
                dataset_dir.exists(),
                "Dataset directory does not exist: {}",
                dataset_dir.display()
            );
            Network::load_from_dir(&dataset_dir).unwrap_or_else(|errs| {
                panic!(
                    "Failed to load dataset from {}: {errs:?}",
                    dataset_dir.display()
                )
            })
        }

        fn build_context(scenario: &Scenario) -> ScenarioContext {
            let (dir, network) = match (&scenario.network, &scenario.dataset) {
                (Some(name), None) => {
                    let (dir, net) = build_synthetic_network(name);
                    (Some(dir), net)
                }
                (None, Some(path)) => {
                    let net = load_dataset_network(path);
                    (None, net)
                }
                (Some(_), Some(_)) => {
                    panic!("Scenario specifies both 'network' and 'dataset' - use exactly one.")
                }
                (None, None) => panic!("Scenario must specify either 'network' or 'dataset'."),
            };
            let manager = client_manager(network);
            ScenarioContext {
                manager,
                receivers: HashMap::new(),
                pending_disconnect: HashSet::new(),
                _dir: dir,
            }
        }

        fn controllers_from_vec(
            datafeed_controllers: &[DatafeedController],
        ) -> HashMap<ClientId, ControllerInfo> {
            datafeed_controllers
                .iter()
                .filter(|c| !c.callsign.ends_with("_SUP"))
                .map(|c| c.to_controller_info())
                .collect()
        }

        fn load_datafeed_file(scenario_dir: &Path, relative_path: &str) -> Vec<DatafeedController> {
            let path = scenario_dir.join(relative_path);
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read datafeed file {}: {e}", path.display()));
            let feed: DatafeedFile = serde_json::from_str(&content).unwrap_or_else(|e| {
                panic!("Failed to parse datafeed file {}: {e}", path.display())
            });
            feed.controllers
        }

        /// Formats the full internal state of the `ClientManager` for debugging.
        /// Called when an assertion step fails so the tester can see the actual
        /// network state at the point of failure.
        async fn format_debug_state(manager: &ClientManager) -> String {
            let online_stations = manager.online_stations.read().await;
            let online_positions = manager.online_positions.read().await;
            let vatsim_only = manager.vatsim_only_positions.read().await;
            let mut out = String::new();

            out.push_str("Online stations:\n");
            if online_stations.is_empty() {
                out.push_str("  (none)\n");
            }
            let mut stations_sorted: Vec<_> = online_stations.iter().collect();
            stations_sorted.sort_by_key(|(sid, _)| sid.as_str());
            for (sid, pid) in stations_sorted {
                let pos_clients = online_positions.get(pid).cloned().unwrap_or_default();
                let vatsim_cids = vatsim_only.get(pid).cloned().unwrap_or_default();
                let is_vatsim_only = !vatsim_cids.is_empty();
                out.push_str(&format!(
                    "  {sid} -> {pid} (vatsim_only={is_vatsim_only}, clients={pos_clients:?}, vatsim_cids={vatsim_cids:?})\n"
                ));
            }

            out.push_str("\nOnline positions:\n");
            if online_positions.is_empty() {
                out.push_str("  (none)\n");
            }
            let mut pos_sorted: Vec<_> = online_positions.iter().collect();
            pos_sorted.sort_by_key(|(k, _)| k.as_str());
            for (pid, cids) in pos_sorted {
                out.push_str(&format!("  {pid} -> {cids:?}\n"));
            }

            out.push_str("\nVatsim-only positions:\n");
            if vatsim_only.is_empty() {
                out.push_str("  (none)\n");
            }
            let mut vo_sorted: Vec<_> = vatsim_only.iter().collect();
            vo_sorted.sort_by_key(|(k, _)| k.as_str());
            for (pid, cids) in vo_sorted {
                out.push_str(&format!("  {pid} -> {cids:?}\n"));
            }

            out
        }

        /// If `result` is a caught panic, prints the debug state of the
        /// `ClientManager` to stderr and then resumes the panic.
        async fn dump_state_on_panic<R>(
            manager: &ClientManager,
            step_label: &str,
            result: std::thread::Result<R>,
        ) -> R {
            match result {
                Ok(v) => v,
                Err(e) => {
                    let state = format_debug_state(manager).await;
                    eprintln!(
                        "\n=== Debug State ({step_label}) ===\n{state}=== End Debug State ===\n"
                    );
                    resume_unwind(e);
                }
            }
        }

        async fn run_scenario(path: &Path) {
            let content = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("Failed to read scenario {}: {e}", path.display()));
            let scenario: Scenario = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse scenario {}: {e}", path.display()));

            let scenario_dir = path.parent().unwrap();
            let mut ctx = build_context(&scenario);

            for (step_idx, step) in scenario.steps.iter().enumerate() {
                let step_label = format!(
                    "{}[step {}]",
                    path.file_name().unwrap().to_string_lossy(),
                    step_idx + 1
                );
                match step {
                    Step::Connect(s) => {
                        let info = client_info(&s.client_id, &s.position_id, &s.frequency);
                        let (_session, rx) = ctx
                            .manager
                            .add_client(
                                info,
                                ActiveProfile::Custom,
                                ClientConnectionGuard::default(),
                            )
                            .await
                            .unwrap_or_else(|e| panic!("{step_label}: add_client failed: {e}"));
                        ctx.receivers.insert(s.client_id.clone(), rx);
                    }
                    Step::ConnectWithoutPosition(s) => {
                        let info = client_info_without_position(&s.client_id);
                        let (_session, rx) = ctx
                            .manager
                            .add_client(
                                info,
                                ActiveProfile::Custom,
                                ClientConnectionGuard::default(),
                            )
                            .await
                            .unwrap_or_else(|e| {
                                panic!("{step_label}: add_client (no position) failed: {e}")
                            });
                        ctx.receivers.insert(s.client_id.clone(), rx);
                    }
                    Step::Disconnect(s) => {
                        ctx.manager
                            .remove_client(cid(&s.client_id), Some(DisconnectReason::Terminated))
                            .await;
                        ctx.receivers.remove(&s.client_id);
                    }
                    Step::Datafeed(s) => {
                        let controllers = controllers_from_vec(&s.controllers);
                        ctx.manager
                            .sync_vatsim_state(&controllers, &mut ctx.pending_disconnect, false)
                            .await;
                    }
                    Step::DatafeedFile(relative_path) => {
                        let feed = load_datafeed_file(scenario_dir, relative_path);
                        let controllers = controllers_from_vec(&feed);
                        ctx.manager
                            .sync_vatsim_state(&controllers, &mut ctx.pending_disconnect, false)
                            .await;
                    }
                    Step::DrainMessages(s) => {
                        let rx = ctx.receivers.get_mut(&s.client_id).unwrap_or_else(|| {
                            panic!("{step_label}: unknown client_id '{}'", s.client_id)
                        });
                        drain_messages(rx);
                    }
                    Step::AssertCallableStations(s) => {
                        let position_id = {
                            let client = ctx
                                .manager
                                .get_client(&cid(&s.client_id))
                                .await
                                .unwrap_or_else(|| {
                                    panic!("{step_label}: client '{}' not found", s.client_id)
                                });
                            client.position_id().cloned()
                        };

                        let stations = ctx
                            .manager
                            .list_stations(&ActiveProfile::Custom, position_id.as_ref())
                            .await;
                        let own_ids: HashSet<&str> = stations
                            .iter()
                            .filter(|s| s.own)
                            .map(|s| s.id.as_str())
                            .collect();
                        let other_ids: HashSet<&str> = stations
                            .iter()
                            .filter(|s| !s.own)
                            .map(|s| s.id.as_str())
                            .collect();
                        let all_ids: HashSet<&str> =
                            stations.iter().map(|s| s.id.as_str()).collect();

                        let own = &s.own;
                        let callable = &s.callable;
                        let not_callable = &s.not_callable;
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            for expected in own {
                                assert!(
                                    own_ids.contains(expected.as_str()),
                                    "{step_label}: expected station '{expected}' to be own, but it was not.\n  own: {own_ids:?}\n  other: {other_ids:?}"
                                );
                            }
                            for expected in callable {
                                assert!(
                                    other_ids.contains(expected.as_str()),
                                    "{step_label}: expected station '{expected}' to be callable (not own), but it was not.\n  own: {own_ids:?}\n  other: {other_ids:?}"
                                );
                            }
                            for unexpected in not_callable {
                                assert!(
                                    !all_ids.contains(unexpected.as_str()),
                                    "{step_label}: expected station '{unexpected}' to NOT be callable, but it was.\n  own: {own_ids:?}\n  other: {other_ids:?}"
                                );
                            }
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::AssertStationChanges(s) => {
                        let rx = ctx.receivers.get_mut(&s.client_id).unwrap_or_else(|| {
                            panic!("{step_label}: unknown client_id '{}'", s.client_id)
                        });
                        let drained = drain_messages(rx);
                        let mut expected: Vec<StationChange> =
                            s.changes.iter().map(StationChange::from).collect();
                        expected.sort();

                        let r = catch_unwind(AssertUnwindSafe(|| {
                            assert_eq!(
                                drained.station_changes, expected,
                                "{step_label}: station changes mismatch"
                            );
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::AssertVatsimOnlyPositions(s) => {
                        let vatsim_only = ctx.manager.vatsim_only_positions.read().await;
                        let mut actual: Vec<String> =
                            vatsim_only.keys().map(|p| p.as_str().to_string()).collect();
                        actual.sort();
                        drop(vatsim_only);

                        let mut expected = s.positions.clone();
                        expected.sort();

                        let r = catch_unwind(AssertUnwindSafe(|| {
                            assert_eq!(
                                actual, expected,
                                "{step_label}: vatsim_only_positions mismatch"
                            );
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::AssertOnlineStations(s) => {
                        let online_stations = ctx.manager.online_stations.read().await;
                        let actual_keys: Vec<String> = online_stations
                            .keys()
                            .map(|k| k.as_str().to_string())
                            .collect();
                        let actual_set: HashSet<StationId> =
                            online_stations.keys().cloned().collect();
                        drop(online_stations);

                        let online = &s.online;
                        let offline = &s.offline;
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            for expected in online {
                                assert!(
                                    actual_set.contains(&StationId::from(expected.clone())),
                                    "{step_label}: expected station '{expected}' to be in online_stations, but it was not.\n  online: {actual_keys:?}"
                                );
                            }
                            for unexpected in offline {
                                assert!(
                                    !actual_set.contains(&StationId::from(unexpected.clone())),
                                    "{step_label}: expected station '{unexpected}' to NOT be in online_stations, but it was.\n  online: {actual_keys:?}"
                                );
                            }
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::AssertOnlinePositions(s) => {
                        let online_positions = ctx.manager.online_positions.read().await;
                        let actual_keys: Vec<String> = online_positions
                            .keys()
                            .map(|k| k.as_str().to_string())
                            .collect();
                        let actual_set: HashSet<PositionId> =
                            online_positions.keys().cloned().collect();
                        drop(online_positions);

                        let online = &s.online;
                        let not_online = &s.not_online;
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            for expected in online {
                                assert!(
                                    actual_set.contains(&PositionId::from(expected.clone())),
                                    "{step_label}: expected position '{expected}' to be in online_positions, but it was not.\n  online: {actual_keys:?}"
                                );
                            }
                            for unexpected in not_online {
                                assert!(
                                    !actual_set.contains(&PositionId::from(unexpected.clone())),
                                    "{step_label}: expected position '{unexpected}' to NOT be in online_positions, but it was.\n  online: {actual_keys:?}"
                                );
                            }
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::AssertClientCount(expected_count) => {
                        let actual = ctx.manager.clients.read().await.len();
                        let expected = *expected_count;
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            assert_eq!(actual, expected, "{step_label}: client count mismatch");
                        }));
                        dump_state_on_panic(&ctx.manager, &step_label, r).await;
                    }
                    Step::Comment(_) => {}
                }
            }
        }

        fn discover_scenarios() -> Vec<PathBuf> {
            let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("scenarios");

            if !fixtures_dir.exists() {
                return Vec::new();
            }

            let mut paths: Vec<PathBuf> = Vec::new();
            collect_scenarios(&fixtures_dir, &mut paths);
            paths.sort();
            paths
        }

        /// Recursively collect *.json scenario files, skipping `feeds/`
        /// and `datasets/` directories.
        fn collect_scenarios(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap().to_string_lossy();
                    if name == "feeds" || name == "datasets" {
                        continue;
                    }
                    collect_scenarios(&path, out);
                } else if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                    out.push(path);
                }
            }
        }

        #[tokio::test]
        async fn run_scenarios() {
            let scenarios = discover_scenarios();
            assert!(
                !scenarios.is_empty(),
                "No sync scenario files found in tests/fixtures/scenarios/"
            );

            for path in &scenarios {
                let name = path.file_name().unwrap().to_string_lossy();
                println!("Running scenario: {name}");
                run_scenario(path).await;
                println!("Scenario passed: {name}");
            }
        }
    }
}
