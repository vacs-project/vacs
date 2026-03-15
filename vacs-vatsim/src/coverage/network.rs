use crate::FacilityType;
use crate::coverage::flight_information_region::{
    FlightInformationRegion, FlightInformationRegionId, FlightInformationRegionRaw,
};
use crate::coverage::position::Position;
use crate::coverage::profile::Profile;
use crate::coverage::station::Station;
use crate::coverage::{
    CoverageError, IoError, ReferenceValidator, StructureError, ValidationError,
};
use std::collections::{HashMap, HashSet};
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{PositionId, StationChange, StationId};

#[derive(Clone, Default)]
pub struct Network {
    firs: HashMap<FlightInformationRegionId, FlightInformationRegion>,
    positions: HashMap<PositionId, Position>,
    stations: HashMap<StationId, Station>,
    profiles: HashMap<ProfileId, Profile>,
}

impl std::fmt::Debug for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network")
            .field("firs", &self.firs.len())
            .field("positions", &self.positions.len())
            .field("stations", &self.stations.len())
            .field("profiles", &self.profiles.len())
            .finish()
    }
}

impl Network {
    #[tracing::instrument(level = "trace", skip(dir), fields(dir = tracing::field::Empty))]
    pub fn load_from_dir(dir: impl AsRef<std::path::Path>) -> Result<Self, Vec<CoverageError>> {
        let dir = dir.as_ref();
        tracing::Span::current().record("dir", tracing::field::debug(dir));
        tracing::trace!("Loading network");

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                return Err(vec![
                    IoError::Read {
                        path: dir.to_path_buf(),
                        reason: err.to_string(),
                    }
                    .into(),
                ]);
            }
        };

        let mut errors = Vec::new();
        let mut raw_firs = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    let err: CoverageError = IoError::ReadEntry(err.to_string()).into();
                    tracing::warn!(?err, "Failed to read directory entry");
                    errors.push(err);
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_dir() {
                tracing::trace!(?path, "Skipping non-directory entry");
                continue;
            }

            let fir = match FlightInformationRegionRaw::load_from_dir(&path) {
                Ok(fir) => fir,
                Err(errs) => {
                    for err in errs {
                        let err: CoverageError = StructureError::Load {
                            entity: "FIR".to_string(),
                            id: path.display().to_string(),
                            reason: err.to_string(),
                        }
                        .into();
                        tracing::warn!(?err, ?path, "Failed to load FIR");
                        errors.push(err);
                    }
                    continue;
                }
            };

            raw_firs.push(fir);
        }

        let mut firs = HashMap::new();
        let mut stations = HashMap::new();
        let mut positions = HashMap::new();
        let mut profiles = HashMap::new();

        let all_stations = raw_firs
            .iter()
            .flat_map(|fir| fir.stations.iter().map(|s| (s.id.clone(), s)))
            .collect::<HashMap<_, _>>();

        let all_station_ids = all_stations.keys().collect::<HashSet<_>>();
        let all_position_ids = raw_firs
            .iter()
            .flat_map(|fir| fir.positions.iter().map(|p| &p.id))
            .collect::<HashSet<_>>();
        let all_profile_ids = raw_firs
            .iter()
            .flat_map(|fir| fir.profiles.keys())
            .collect::<HashSet<_>>();

        for fir_raw in &raw_firs {
            for station in &fir_raw.stations {
                if let Err(err) = station.validate_references(&all_position_ids) {
                    tracing::warn!(?err, ?station.id, ?fir_raw.id, "Invalid position reference in station");
                    errors.push(
                        err.context(station.id.as_str())
                            .context(fir_raw.id.as_str()),
                    );
                }
            }
            for position in &fir_raw.positions {
                if let Err(err) = position.validate_references(&all_profile_ids) {
                    tracing::warn!(?err, ?position.id, ?fir_raw.id, "Invalid profile reference in position");
                    errors.push(
                        err.context(position.id.as_str())
                            .context(fir_raw.id.as_str()),
                    );
                }

                if let Err(err) = position.validate_references(&all_station_ids) {
                    tracing::warn!(?err, ?position.id, ?fir_raw.id, "Invalid default call source reference in position");
                    errors.push(
                        err.context(position.id.as_str())
                            .context(fir_raw.id.as_str()),
                    );
                }
            }
            for profile in fir_raw.profiles.values() {
                if let Err(err) = profile.validate_references(&all_station_ids) {
                    tracing::warn!(?err, ?profile.id, ?fir_raw.id, "Invalid station reference in profile");
                    errors.push(
                        err.context(profile.id.as_str())
                            .context(fir_raw.id.as_str()),
                    );
                }
            }

            if firs.contains_key(&fir_raw.id) {
                let err: CoverageError = StructureError::Duplicate {
                    entity: "FIR".to_string(),
                    id: fir_raw.id.to_string(),
                }
                .into();
                tracing::warn!(?fir_raw, "Duplicate FIR ID");
                errors.push(err);
                continue;
            }

            match FlightInformationRegion::try_from(fir_raw.clone()) {
                Ok(fir) => firs.insert(fir.id.clone(), fir),
                Err(err) => {
                    let err: CoverageError = StructureError::Load {
                        entity: "FIR".to_string(),
                        id: fir_raw.id.to_string(),
                        reason: err.to_string(),
                    }
                    .into();
                    tracing::warn!(?err, ?fir_raw, "Failed to parse FIR");
                    errors.push(err);
                    continue;
                }
            };

            for position_raw in &fir_raw.positions {
                if positions.contains_key(&position_raw.id) {
                    let err: CoverageError = StructureError::Duplicate {
                        entity: "Position".to_string(),
                        id: position_raw.id.to_string(),
                    }
                    .into();
                    tracing::warn!(?position_raw, "Duplicate position ID");
                    errors.push(err.context(fir_raw.id.as_str()));
                    continue;
                }

                match Position::from_raw(position_raw.clone(), fir_raw.id.clone()) {
                    Ok(position) => positions.insert(position.id.clone(), position),
                    Err(err) => {
                        let err: CoverageError = StructureError::Load {
                            entity: "Position".to_string(),
                            id: position_raw.id.to_string(),
                            reason: err.to_string(),
                        }
                        .into();
                        tracing::warn!(?err, ?position_raw, "Failed to parse position");
                        errors.push(err.context(fir_raw.id.as_str()));
                        continue;
                    }
                };
            }

            for station_raw in &fir_raw.stations {
                if stations.contains_key(&station_raw.id) {
                    let err: CoverageError = StructureError::Duplicate {
                        entity: "Station".to_string(),
                        id: station_raw.id.to_string(),
                    }
                    .into();
                    tracing::warn!(?station_raw, "Duplicate station ID");
                    errors.push(err.context(fir_raw.id.as_str()));
                    continue;
                }

                let station =
                    match Station::from_raw(station_raw.clone(), fir_raw.id.clone(), &all_stations)
                    {
                        Ok(station) => station,
                        Err(errs) => {
                            for err in errs {
                                let err: CoverageError = StructureError::Load {
                                    entity: "Station".to_string(),
                                    id: station_raw.id.to_string(),
                                    reason: err.to_string(),
                                }
                                .into();
                                tracing::warn!(?err, ?station_raw, "Failed to parse station");
                                errors.push(err.context(fir_raw.id.as_str()));
                            }
                            continue;
                        }
                    };

                if station.controlled_by.is_empty() {
                    let err: CoverageError =
                        ValidationError::EmptyCoverage(station.id.to_string()).into();
                    tracing::warn!(?err, ?station_raw, "Station has no coverage");
                    errors.push(err.context(fir_raw.id.as_str()));
                    continue;
                }

                stations.insert(station.id.clone(), station);
            }

            for (profile_id, profile) in &fir_raw.profiles {
                if profiles.contains_key(profile_id) {
                    let err: CoverageError = StructureError::Duplicate {
                        entity: "Profile".to_string(),
                        id: profile_id.to_string(),
                    }
                    .into();
                    tracing::warn!(?profile, "Duplicate profile ID");
                    errors.push(err.context(fir_raw.id.as_str()));
                    continue;
                }
                profiles.insert(profile_id.clone(), profile.clone());
            }
        }

        for station in stations.values() {
            for position_id in &station.controlled_by {
                if let Some(position) = positions.get_mut(position_id) {
                    position.controlled_stations.insert(station.id.clone());
                } else {
                    let err: CoverageError = ValidationError::MissingReference {
                        field: "position_id".to_string(),
                        ref_id: position_id.to_string(),
                    }
                    .into();
                    tracing::warn!(?err, ?station, "Position referenced by station not found");
                    errors.push(
                        err.context(station.id.as_str())
                            .context(station.fir_id.as_str()),
                    );
                }
            }
        }

        if !errors.is_empty() {
            tracing::warn!(?errors, "Failed to load network");
            return Err(errors);
        }

        let network = Self {
            firs,
            positions,
            stations,
            profiles,
        };

        tracing::info!(?network, "Successfully loaded network");
        Ok(network)
    }

    pub fn get_profile(&self, profile_id: &ProfileId) -> Option<&Profile> {
        self.profiles.get(profile_id)
    }

    pub fn get_position(&self, position_id: &PositionId) -> Option<&Position> {
        self.positions.get(position_id)
    }

    pub fn positions_count(&self) -> usize {
        self.positions.len()
    }

    pub fn stations_count(&self) -> usize {
        self.stations.len()
    }

    pub fn profiles_count(&self) -> usize {
        self.profiles.len()
    }

    #[tracing::instrument(level = "trace", skip_all, fields(callsign = tracing::field::Empty, frequency = tracing::field::Empty, facility_type = tracing::field::Empty))]
    pub fn find_positions(
        &self,
        callsign: impl AsRef<str>,
        frequency: impl AsRef<str>,
        facility_type: impl Into<FacilityType>,
    ) -> Vec<&Position> {
        // Normalize callsign (standard relief pattern) and ensure uppercase for matching
        let callsign = callsign.as_ref().replace("__", "_").to_ascii_uppercase();
        let frequency = frequency.as_ref();
        let facility_type = facility_type.into();
        tracing::Span::current()
            .record("callsign", &callsign)
            .record("frequency", frequency)
            .record("facility_type", tracing::field::debug(&facility_type));

        // Check if a position with the exact callsign exists and the frequency and facility type match
        if let Some(position) = self.positions.get(callsign.as_str())
            && position.frequency == frequency
            && position.facility_type == facility_type
        {
            tracing::trace!(?position, "Found exact match for callsign");
            return vec![position];
        }

        // Find all positions with the same frequency and facility type that have a prefix matching the callsign
        let mut positions = self
            .positions
            .values()
            .filter(|p| {
                p.frequency == frequency
                    && p.facility_type == facility_type
                    && p.prefixes.iter().any(|pre| callsign.starts_with(pre))
            })
            .collect::<Vec<_>>();

        if positions.len() == 1 {
            // Non-standard relief/COO callsign, but only one matching position found --> successful match
            tracing::trace!(position = ?positions[0], "Found exact match for frequency and station type");
        } else if positions.is_empty() {
            // No matches found at all (frequency and facility type might yield results, but callsign
            // didn't match any defined prefixes and FIR from callsign doesn't match) --> no match
            tracing::trace!("No matches found");
        } else {
            // Multiple matches found, no automatic selection possible --> user has to select the correct one
            tracing::trace!(positions = positions.len(), "Found multiple matches");
        }

        positions.sort_by(|a, b| a.id.cmp(&b.id));
        positions
    }

    #[tracing::instrument(level = "trace", skip(self, online_positions), fields(online_positions = online_positions.len()))]
    pub fn covered_stations(
        &'_ self,
        client_position_id: Option<&PositionId>,
        online_positions: &HashSet<&PositionId>,
    ) -> Vec<CoveredStation<'_>> {
        let mut stations = self
            .stations
            .values()
            .filter_map(|station| {
                self.controlling_position(&station.id, online_positions)
                    .map(|position| {
                        let is_self_controlled = client_position_id == Some(&position.id);
                        CoveredStation {
                            station,
                            is_self_controlled,
                        }
                    })
            })
            .collect::<Vec<_>>();

        stations.sort_by(|a, b| a.station.id.cmp(&b.station.id));
        stations
    }

    #[tracing::instrument(level = "trace", skip(self, online_positions), fields(online_positions = online_positions.len()))]
    pub fn controlling_position(
        &self,
        station_id: &StationId,
        online_positions: &HashSet<&PositionId>,
    ) -> Option<&Position> {
        self.stations
            .get(station_id)?
            .controlled_by
            .iter()
            .find_map(|pos_id| {
                if online_positions.contains(pos_id) {
                    let position = self.positions.get(pos_id.as_str())?;
                    tracing::trace!(?position, "Found position with matching coverage");
                    Some(position)
                } else {
                    None
                }
            })
    }

    #[tracing::instrument(level = "trace", skip(self, online_positions), fields(online_positions = online_positions.len()))]
    pub fn coverage_changes(
        &self,
        from_position_id: Option<&PositionId>,
        to_position_id: Option<&PositionId>,
        online_positions: &HashSet<&PositionId>,
    ) -> Vec<StationChange> {
        let mut updated_positions = online_positions.clone();

        let changed = match (from_position_id, to_position_id) {
            (Some(from), Some(to)) if from == to => updated_positions.insert(to),
            (Some(from), Some(to)) => {
                let removed = updated_positions.remove(from);
                let inserted = updated_positions.insert(to);
                removed || inserted
            }
            (Some(from), None) => updated_positions.remove(from),
            (None, Some(to)) => updated_positions.insert(to),
            (None, None) => false,
        };

        if !changed {
            tracing::trace!("No change in online positions, returning empty coverage changes");
            return Vec::new();
        }

        self.coverage_diff(
            online_positions,
            &updated_positions.iter().copied().collect(),
        )
    }

    #[tracing::instrument(level = "trace", skip(self, from_online_positions, to_online_positions), fields(from_online_positions = from_online_positions.len(), to_online_positions = to_online_positions.len()))]
    pub fn coverage_diff(
        &self,
        from_online_positions: &HashSet<&PositionId>,
        to_online_positions: &HashSet<&PositionId>,
    ) -> Vec<StationChange> {
        let mut changes: Vec<StationChange> = Vec::new();

        let changed_positions = from_online_positions
            .symmetric_difference(to_online_positions)
            .copied()
            .collect::<Vec<_>>();

        if changed_positions.is_empty() {
            tracing::trace!("No change in online positions, returning empty coverage changes");
            return Vec::new();
        }

        let candidate_stations: HashSet<&StationId> = changed_positions
            .into_iter()
            .filter_map(|position_id| self.positions.get(position_id))
            .flat_map(|position| &position.controlled_stations)
            .collect();

        for station_id in candidate_stations {
            let Some(station) = self.stations.get(station_id) else {
                tracing::warn!(?station_id, "Candidate station not found");
                continue;
            };

            let before = self.controlling_position(&station.id, from_online_positions);
            let after = self.controlling_position(&station.id, to_online_positions);

            if before == after {
                continue;
            }

            match (before, after) {
                (None, Some(new_pos)) => {
                    tracing::trace!(?station, ?new_pos, "Station is now online");
                    changes.push(StationChange::Online {
                        station_id: station.id.clone(),
                        position_id: new_pos.id.clone(),
                    });
                }
                (Some(old_pos), None) => {
                    tracing::trace!(?station, ?old_pos, "Station is now offline");
                    changes.push(StationChange::Offline {
                        station_id: station.id.clone(),
                    });
                }
                (Some(old_pos), Some(new_pos)) => {
                    tracing::trace!(?station, ?old_pos, ?new_pos, "Station coverage changed");
                    changes.push(StationChange::Handoff {
                        station_id: station.id.clone(),
                        from_position_id: old_pos.id.clone(),
                        to_position_id: new_pos.id.clone(),
                    });
                }
                (None, None) => {}
            }
        }

        changes.sort();
        changes
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn relevant_stations(&self, selection: &ActiveProfile<ProfileId>) -> RelevantStations<'_> {
        match selection {
            ActiveProfile::Specific(profile_id) => {
                let Some(profile) = self.profiles.get(profile_id) else {
                    tracing::trace!("Profile not found");
                    return RelevantStations::None;
                };
                RelevantStations::Subset(&profile.relevant_station_ids)
            }
            ActiveProfile::Custom => RelevantStations::All,
            ActiveProfile::None => RelevantStations::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RelevantStations<'a> {
    All,
    Subset(&'a HashSet<StationId>),
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct CoveredStation<'a> {
    pub station: &'a Station,
    pub is_self_controlled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::ValidationError;
    use crate::coverage::test_support::TestFirBuilder;
    use pretty_assertions::{assert_eq, assert_matches};

    fn causes(error: &CoverageError, matcher: impl Fn(&CoverageError) -> bool) -> bool {
        if matcher(error) {
            return true;
        }
        match error {
            CoverageError::Context(ctx) => causes(&ctx.error, matcher),
            _ => false,
        }
    }

    fn create_minimal_valid_fir(dir: &std::path::Path, name: &str) {
        TestFirBuilder::new(name)
            .station(&format!("{name}_CTR"), &[&format!("{name}_CTR")])
            .position(&format!("{name}_CTR"), &[name], "199.998", "Enroute")
            .create(dir);
    }

    fn create_extended_valid_fir(dir: &std::path::Path) {
        TestFirBuilder::new("LOVV")
            .station(
                "LOVV_E2",
                &[
                    "LOVV_EU_CTR",
                    "LOVV_NU_CTR",
                    "LOVV_U_CTR",
                    "LOVV_E_CTR",
                    "LOVV_N_CTR",
                    "LOVV_CTR",
                    "LOVV_C_CTR",
                ],
            )
            .station(
                "LOVV_E1",
                &[
                    "LOVV_E_CTR",
                    "LOVV_N_CTR",
                    "LOVV_CTR",
                    "LOVV_C_CTR",
                    "LOVV_EU_CTR",
                    "LOVV_NU_CTR",
                    "LOVV_U_CTR",
                ],
            )
            .station(
                "LOWW_APP",
                &[
                    "LOWW_APP",
                    "LOWW_P_APP",
                    "LOWW_N_APP",
                    "LOWW_M_APP",
                    "LOVV_L_CTR",
                    "LOVV_E_CTR",
                    "LOVV_N_CTR",
                    "LOVV_CTR",
                    "LOVV_C_CTR",
                    "LOVV_EU_CTR",
                    "LOVV_NU_CTR",
                ],
            )
            .station_with_parent("LOWW_TWR", "LOWW_APP", &["LOWW_TWR", "LOWW_E_TWR"])
            .station_with_parent("LOWW_E_TWR", "LOWW_TWR", &["LOWW_E_TWR"])
            .station_with_parent("LOWW_GND", "LOWW_TWR", &["LOWW_GND", "LOWW_W_GND"])
            .station_with_parent("LOWW_W_GND", "LOWW_GND", &["LOWW_W_GND"])
            .station_with_parent("LOWW_DEL", "LOWW_GND", &["LOWW_DEL"])
            .position("LOVV_EU_CTR", &["LOVV"], "135.635", "CTR")
            .position("LOVV_NU_CTR", &["LOVV"], "119.880", "CTR")
            .position("LOVV_U_CTR", &["LOVV"], "131.350", "CTR")
            .position("LOVV_N_CTR", &["LOVV"], "134.350", "CTR")
            .position("LOVV_C_CTR", &["LOVV"], "132.160", "CTR")
            .position("LOVV_L_CTR", &["LOVV"], "129.200", "CTR")
            .position("LOVV_E_CTR", &["LOVV"], "134.440", "CTR")
            .position("LOVV_CTR", &["LOVV"], "132.600", "CTR")
            .position("LOWW_P_APP", &["LOWW"], "129.050", "APP")
            .position("LOWW_N_APP", &["LOWW"], "118.775", "APP")
            .position("LOWW_M_APP", &["LOWW"], "125.175", "APP")
            .position("LOWW_APP", &["LOWW"], "134.675", "APP")
            .position("LOWW_TWR", &["LOWW"], "119.400", "TWR")
            .position("LOWW_E_TWR", &["LOWW"], "123.800", "TWR")
            .position("LOWW_GND", &["LOWW"], "121.600", "GND")
            .position("LOWW_W_GND", &["LOWW"], "121.775", "GND")
            .position("LOWW_DEL", &["LOWW"], "122.125", "DEL")
            .create(dir);
    }

    #[test]
    fn load_from_dir_valid_single() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");

        let network = Network::load_from_dir(dir.path()).unwrap();
        assert_eq!(network.firs.len(), 1);
        assert!(network.firs.contains_key("LOVV"));
        assert_eq!(network.stations.len(), 1);
        assert_eq!(network.positions.len(), 1);
    }

    #[test]
    fn load_from_dir_valid_multiple() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "EDMM");

        let network = Network::load_from_dir(dir.path()).unwrap();
        assert_eq!(network.firs.len(), 2);
        assert!(network.firs.contains_key("LOVV"));
        assert!(network.firs.contains_key("EDMM"));
        assert_eq!(network.stations.len(), 2);
        assert_eq!(network.positions.len(), 2);
    }

    #[test]
    #[cfg_attr(target_os = "windows", ignore)]
    fn load_from_dir_duplicate_fir_id() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "lovv");

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_matches!(&errors[0], CoverageError::Structure(StructureError::Duplicate { entity, .. }) if entity == "FIR");
    }

    #[test]
    fn load_from_dir_duplicate_station_id_same_fir() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWW_TWR", &["LOWW_TWR"])
            .station("LOWW_TWR", &["LOWW_TWR"])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, .. }) if entity == "Station"))));
    }

    #[test]
    fn load_from_dir_duplicate_station_id_different_fir() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWW_TWR", &["LOWW_TWR"])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        TestFirBuilder::new("EDMM")
            .station("LOWW_TWR", &["EDDM_S_TWR"])
            .position("EDDM_S_TWR", &["EDDM"], "120.505", "Tower")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, .. }) if entity == "Station"))));
    }

    #[test]
    fn load_from_dir_duplicate_position_id_same_fir() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWW_TWR", &["LOWW_TWR"])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, .. }) if entity == "Position"))));
    }

    #[test]
    fn load_from_dir_duplicate_position_id_different_fir() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWW_TWR", &["LOWW_TWR"])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        TestFirBuilder::new("EDMM")
            .station("EDDM_S_TWR", &["EDDM_S_TWR"])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, .. }) if entity == "Position"))));
    }

    #[test]
    fn load_from_dir_empty_coverage() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWW_TWR", &[])
            .position("LOWW_TWR", &["LOWW"], "119.400", "Tower")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(causes(
            &errors[0],
            |x| matches!(x, CoverageError::Validation(ValidationError::EmptyCoverage(station)) if station == "LOWW_TWR")
        ));
    }

    #[test]
    fn load_from_dir_error() {
        let dir = tempfile::tempdir().unwrap();

        // FIR 1: Malformed TOML
        let fir1 = dir.path().join("FIR1");
        std::fs::create_dir(&fir1).unwrap();
        std::fs::write(fir1.join("stations.toml"), "invalid").unwrap();
        std::fs::write(fir1.join("positions.toml"), "").unwrap();

        // FIR 2: Duplicate station within same FIR file
        TestFirBuilder::new("FIR2")
            .station("A", &["A"])
            .station("A", &["A"])
            .station("B", &["B"])
            .position("B", &["B"], "199.998", "Center")
            .create(dir.path());

        // FIR 3: Duplicate station/position within FIR 2 file
        TestFirBuilder::new("FIR3")
            .station("B", &["B", "C"])
            .position("B", &["B"], "199.998", "Tower")
            .position("C", &["C"], "199.998", "Ground")
            .create(dir.path());

        let errors = Network::load_from_dir(dir.path()).expect_err("should not load from dir");
        assert_eq!(errors.len(), 8);
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Load { entity, id, reason }) if entity == "FIR" && id.contains("FIR1") && reason.contains("stations.toml")))));
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Load { entity, id, reason }) if entity == "FIR" && id.contains("FIR1") && reason.contains("positions.toml")))));
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, id }) if entity == "Station" && id == "A"))));
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, id }) if entity == "Station" && id == "B"))));
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Structure(StructureError::Duplicate { entity, id }) if entity == "Position" && id == "B"))));
        assert!(errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Validation(ValidationError::MissingReference { field, ref_id }) if field == "position_id" && ref_id == "A"))));
    }

    #[test]
    fn find_positions_callsign_match() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "EDMM");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOVV_CTR", "199.998", FacilityType::Enroute);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].id.as_str(), "LOVV_CTR");
    }

    #[test]
    fn find_positions_relief_callsign_match() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "EDMM");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOVV__CTR", "199.998", FacilityType::Enroute);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].id.as_str(), "LOVV_CTR");
    }

    #[test]
    fn find_positions_prefix_match() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("ENOR")
            .station("ENCN_TWR", &["ENCN_TWR"])
            .station("ENDU_TWR", &["ENDU_TWR"])
            .station("ENAL_TWR", &["ENAL_TWR"])
            .station("ENBO_TWR", &["ENBO_TWR"])
            .station("ENKR_TWR", &["ENKR_TWR"])
            .position("ENKR_TWR", &["ENKR"], "118.105", "TWR")
            .position("ENCN_TWR", &["ENCN"], "118.105", "TWR")
            .position("ENAL_TWR", &["ENAL"], "118.105", "TWR")
            .position("ENBO_TWR", &["ENBO"], "118.105", "TWR")
            .position("ENDU_TWR", &["ENDU"], "118.105", "TWR")
            .create(dir.path());

        TestFirBuilder::new("EBBU")
            .station("ELLX_TWR", &["ELLX_TWR"])
            .position("ELLX_TWR", &["ELLX"], "118.105", "TWR")
            .create(dir.path());

        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("ENBO_X_TWR", "118.105", FacilityType::Tower);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].id.as_str(), "ENBO_TWR");
    }

    #[test]
    fn find_positions_different_frequency() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "EDMM");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOVV_CTR", "121.500", FacilityType::Enroute);
        assert!(positions.is_empty());
    }

    #[test]
    fn find_positions_different_facility_type() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        create_minimal_valid_fir(dir.path(), "EDMM");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOVV_CTR", "199.998", FacilityType::TrafficFlow);
        assert!(positions.is_empty());
    }

    #[test]
    fn find_positions_different_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "EDMM");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOVV_CTR", "199.998", FacilityType::Enroute);
        assert!(positions.is_empty());
    }

    #[test]
    fn find_positions_multiple_matches() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWI_E_APP", &["LOWI_E_APP"])
            .station("LOWI_S_APP", &["LOWI_S_APP"])
            .position("LOWI_S_APP", &["LOWI"], "128.975", "APP")
            .position("LOWI_E_APP", &["LOWI"], "128.975", "Approach")
            .create(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOWI_X_APP", "128.975", FacilityType::Approach);
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].id.as_str(), "LOWI_E_APP");
        assert_eq!(positions[1].id.as_str(), "LOWI_S_APP");
    }

    #[test]
    fn find_positions_callsign_match_identical_frequency() {
        let dir = tempfile::tempdir().unwrap();
        TestFirBuilder::new("LOVV")
            .station("LOWI_E_APP", &["LOWI_E_APP"])
            .station("LOWI_S_APP", &["LOWI_S_APP"])
            .position("LOWI_S_APP", &["LOWI"], "128.975", "APP")
            .position("LOWI_E_APP", &["LOWI"], "128.975", "Approach")
            .create(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let positions = network.find_positions("LOWI_S_APP", "128.975", FacilityType::Approach);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].id.as_str(), "LOWI_S_APP");
    }

    #[test]
    fn relevant_stations_specific_found() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        std::fs::write(
            fir_path.join("stations.toml"),
            "[[stations]]\nid=\"S1\"\ncontrolled_by=[\"P1\"]",
        )
        .unwrap();
        std::fs::write(
            fir_path.join("positions.toml"),
            "[[positions]]\nid=\"P1\"\nprefixes=[\"P1\"]\nfrequency=\"118.000\"\nfacility_type=\"Tower\"",
        )
        .unwrap();

        let profile = r#"
            id = "P"
            type = "Geo"
            direction = "row"
            [[children]]
            label = ["B"]
            size = 10.0
            page.keys = [{ label = ["K"], station_id = "S1" }]
            page.rows = 1
        "#;
        std::fs::write(fir_path.join("profile.toml"), profile).unwrap();

        let network = Network::load_from_dir(dir.path()).unwrap();
        let result = network.relevant_stations(&ActiveProfile::Specific(ProfileId::from("P")));

        assert_matches!(result, RelevantStations::Subset(ids) if ids.contains(&StationId::from("S1")));
    }

    #[test]
    fn relevant_stations_specific_not_found() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let result =
            network.relevant_stations(&ActiveProfile::Specific(ProfileId::from("Missing")));
        assert_eq!(result, RelevantStations::None);
    }

    #[test]
    fn relevant_stations_custom() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let result = network.relevant_stations(&ActiveProfile::Custom);
        assert_eq!(result, RelevantStations::All);
    }

    #[test]
    fn relevant_stations_none() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let result = network.relevant_stations(&ActiveProfile::None);
        assert_eq!(result, RelevantStations::None);
    }

    #[test]
    fn controlling_position_found() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let station_id = StationId::from("LOVV_CTR");

        let pos = network.controlling_position(&station_id, &online.iter().collect());
        assert!(pos.is_some());
        assert_eq!(pos.unwrap().id.as_str(), "LOVV_CTR");
    }

    #[test]
    fn controlling_position_multiple_covering() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let mut online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let station_id = StationId::from("LOWW_DEL");

        let mut pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOVV_CTR"));

        online.insert(PositionId::from("LOVV_E_CTR"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOVV_E_CTR"));

        online.insert(PositionId::from("LOWW_DEL"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOWW_DEL"));

        online.remove("LOWW_DEL");
        online.insert(PositionId::from("LOWW_W_GND"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOWW_W_GND"));

        online.insert(PositionId::from("LOWW_GND"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOWW_GND"));

        online.remove("LOWW_GND");
        online.remove("LOWW_W_GND");
        online.insert(PositionId::from("LOWW_APP"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOWW_APP"));

        online.remove("LOVV_CTR");
        online.remove("LOVV_E_CTR");
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert_eq!(pos.map(|p| p.id.as_str()), Some("LOWW_APP"));

        online.remove("LOWW_APP");
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert!(pos.is_none());

        online.insert(PositionId::from("EDMM_RDG_CTR"));
        pos = network.controlling_position(&station_id, &online.iter().collect());
        assert!(pos.is_none());
    }

    #[test]
    fn controlling_position_none() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = HashSet::new();
        let station_id = StationId::from("LOVV_CTR");

        let pos = network.controlling_position(&station_id, &online.iter().collect());
        assert!(pos.is_none());
    }

    #[test]
    fn controlling_position_unknown() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let station_id = StationId::from("EDMM_RDG_CTR");

        let pos = network.controlling_position(&station_id, &online.iter().collect());
        assert!(pos.is_none());
    }

    #[test]
    fn covered_stations_basic() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let covered = network.covered_stations(None, &online.iter().collect());

        assert_eq!(covered.len(), 1);
        assert_eq!(covered[0].station.id.as_str(), "LOVV_CTR");
        assert!(!covered[0].is_self_controlled);
    }

    #[test]
    fn covered_stations_complex() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let mut online = [
            "LOVV_CTR",
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_W_GND",
            "EDMM_RDG_CTR",
        ]
        .into_iter()
        .map(PositionId::from)
        .collect::<HashSet<_>>();
        let mut covered = network.covered_stations(None, &online.iter().collect());
        let mut covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        let mut expected_ids = vec![
            "LOVV_E1",
            "LOVV_E2",
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_E_TWR",
            "LOWW_GND",
            "LOWW_TWR",
            "LOWW_W_GND",
        ]
        .into_iter()
        .map(StationId::from)
        .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);

        online.remove("LOVV_CTR");
        expected_ids = vec![
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_E_TWR",
            "LOWW_GND",
            "LOWW_TWR",
            "LOWW_W_GND",
        ]
        .into_iter()
        .map(StationId::from)
        .collect::<Vec<_>>();
        covered = network.covered_stations(None, &online.iter().collect());
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);

        online.remove("LOWW_APP");
        expected_ids = vec!["LOWW_DEL", "LOWW_GND", "LOWW_W_GND"]
            .into_iter()
            .map(StationId::from)
            .collect::<Vec<_>>();
        covered = network.covered_stations(None, &online.iter().collect());
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);

        online.remove("LOWW_DEL");
        expected_ids = vec!["LOWW_DEL", "LOWW_GND", "LOWW_W_GND"]
            .into_iter()
            .map(StationId::from)
            .collect::<Vec<_>>();
        covered = network.covered_stations(None, &online.iter().collect());
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);

        online.insert(PositionId::from("LOWW_DEL"));
        online.remove("LOWW_W_GND");
        expected_ids = vec!["LOWW_DEL"]
            .into_iter()
            .map(StationId::from)
            .collect::<Vec<_>>();
        covered = network.covered_stations(None, &online.iter().collect());
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);

        online.remove("LOWW_DEL");
        covered = network.covered_stations(None, &online.iter().collect());
        assert!(covered.is_empty());
    }

    #[test]
    fn covered_stations_self_controlled() {
        let dir = tempfile::tempdir().unwrap();
        create_minimal_valid_fir(dir.path(), "LOVV");
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let mut covered = network.covered_stations(
            Some(&PositionId::from("LOVV_CTR")),
            &online.iter().collect(),
        );
        assert_eq!(covered.len(), 1);
        assert_eq!(covered[0].station.id.as_str(), "LOVV_CTR");
        assert!(covered[0].is_self_controlled);

        covered = network.covered_stations(
            Some(&PositionId::from("LOWW_DEL")),
            &online.iter().collect(),
        );
        assert_eq!(covered.len(), 1);
        assert_eq!(covered[0].station.id.as_str(), "LOVV_CTR");
        assert!(!covered[0].is_self_controlled);
    }

    #[test]
    fn covered_stations_self_controlled_complex() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let mut online = [
            "LOVV_CTR",
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_W_GND",
            "EDMM_RDG_CTR",
        ]
        .into_iter()
        .map(PositionId::from)
        .collect::<HashSet<_>>();
        let mut covered = network.covered_stations(
            Some(&PositionId::from("LOWW_APP")),
            &online.iter().collect(),
        );
        let mut covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        let mut self_controlled_ids = covered
            .iter()
            .filter(|s| s.is_self_controlled)
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        let expected_ids = vec![
            "LOVV_E1",
            "LOVV_E2",
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_E_TWR",
            "LOWW_GND",
            "LOWW_TWR",
            "LOWW_W_GND",
        ]
        .into_iter()
        .map(StationId::from)
        .collect::<Vec<_>>();
        let mut expected_self_controlled_ids = vec!["LOWW_APP", "LOWW_E_TWR", "LOWW_TWR"]
            .into_iter()
            .map(StationId::from)
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);
        assert_ne!(covered_ids, expected_self_controlled_ids);
        assert_eq!(self_controlled_ids, expected_self_controlled_ids);

        online.remove("LOWW_DEL");
        covered = network.covered_stations(
            Some(&PositionId::from("LOWW_APP")),
            &online.iter().collect(),
        );
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        self_controlled_ids = covered
            .iter()
            .filter(|s| s.is_self_controlled)
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);
        assert_ne!(covered_ids, expected_self_controlled_ids);
        assert_eq!(self_controlled_ids, expected_self_controlled_ids);

        online.remove("LOWW_W_GND");
        covered = network.covered_stations(
            Some(&PositionId::from("LOWW_APP")),
            &online.iter().collect(),
        );
        covered_ids = covered
            .iter()
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        self_controlled_ids = covered
            .iter()
            .filter(|s| s.is_self_controlled)
            .map(|s| s.station.id.clone())
            .collect::<Vec<_>>();
        expected_self_controlled_ids = vec![
            "LOWW_APP",
            "LOWW_DEL",
            "LOWW_E_TWR",
            "LOWW_GND",
            "LOWW_TWR",
            "LOWW_W_GND",
        ]
        .into_iter()
        .map(StationId::from)
        .collect::<Vec<_>>();
        assert_eq!(covered_ids, expected_ids);
        assert_ne!(covered_ids, expected_self_controlled_ids);
        assert_eq!(self_controlled_ids, expected_self_controlled_ids);
    }

    #[test]
    fn coverage_changes_coming_online() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = HashSet::new();
        let changes = network.coverage_changes(None, Some(&PositionId::from("LOVV_CTR")), &online);
        let expected_changes = vec![
            ("LOVV_E1", None, Some("LOVV_CTR")),
            ("LOVV_E2", None, Some("LOVV_CTR")),
            ("LOWW_APP", None, Some("LOVV_CTR")),
            ("LOWW_DEL", None, Some("LOVV_CTR")),
            ("LOWW_E_TWR", None, Some("LOVV_CTR")),
            ("LOWW_GND", None, Some("LOVV_CTR")),
            ("LOWW_TWR", None, Some("LOVV_CTR")),
            ("LOWW_W_GND", None, Some("LOVV_CTR")),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);
    }

    #[test]
    fn coverage_changes_going_offline() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let online = ["LOVV_CTR"]
            .into_iter()
            .map(PositionId::from)
            .collect::<HashSet<_>>();
        let changes = network.coverage_changes(
            Some(&PositionId::from("LOVV_CTR")),
            None,
            &online.iter().collect(),
        );
        let expected_changes = vec![
            ("LOVV_E1", Some("LOVV_CTR"), None),
            ("LOVV_E2", Some("LOVV_CTR"), None),
            ("LOWW_APP", Some("LOVV_CTR"), None),
            ("LOWW_DEL", Some("LOVV_CTR"), None),
            ("LOWW_E_TWR", Some("LOVV_CTR"), None),
            ("LOWW_GND", Some("LOVV_CTR"), None),
            ("LOWW_TWR", Some("LOVV_CTR"), None),
            ("LOWW_W_GND", Some("LOVV_CTR"), None),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);
    }

    #[test]
    fn coverage_changes_complex() {
        let dir = tempfile::tempdir().unwrap();
        create_extended_valid_fir(dir.path());
        let network = Network::load_from_dir(dir.path()).unwrap();

        let loww_del = PositionId::from("LOWW_DEL");
        let loww_gnd = PositionId::from("LOWW_GND");
        let loww_w_gnd = PositionId::from("LOWW_W_GND");
        let lovv_ctr = PositionId::from("LOVV_CTR");
        let edmm_alb_ctr = PositionId::from("EDMM_ALB_CTR");
        let edmm_rdg_ctr = PositionId::from("EDMM_RDG_CTR");

        let mut online = HashSet::new();
        let mut changes = network.coverage_changes(None, Some(&lovv_ctr), &online.iter().collect());
        let mut expected_changes = vec![
            ("LOVV_E1", None, Some("LOVV_CTR")),
            ("LOVV_E2", None, Some("LOVV_CTR")),
            ("LOWW_APP", None, Some("LOVV_CTR")),
            ("LOWW_DEL", None, Some("LOVV_CTR")),
            ("LOWW_E_TWR", None, Some("LOVV_CTR")),
            ("LOWW_GND", None, Some("LOVV_CTR")),
            ("LOWW_TWR", None, Some("LOVV_CTR")),
            ("LOWW_W_GND", None, Some("LOVV_CTR")),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.insert(lovv_ctr.clone());
        changes = network.coverage_changes(None, Some(&loww_del), &online.iter().collect());
        expected_changes = vec![("LOWW_DEL", Some("LOVV_CTR"), Some("LOWW_DEL"))]
            .into_iter()
            .map(StationChange::from)
            .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.insert(loww_del.clone());
        changes = network.coverage_changes(None, Some(&loww_del), &online.iter().collect());
        assert!(changes.is_empty());

        changes = network.coverage_changes(None, Some(&loww_gnd), &online.iter().collect());
        expected_changes = vec![
            ("LOWW_GND", Some("LOVV_CTR"), Some("LOWW_GND")),
            ("LOWW_W_GND", Some("LOVV_CTR"), Some("LOWW_GND")),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.insert(loww_gnd.clone());
        changes = network.coverage_changes(None, Some(&loww_w_gnd), &online.iter().collect());
        expected_changes = vec![("LOWW_W_GND", Some("LOWW_GND"), Some("LOWW_W_GND"))]
            .into_iter()
            .map(StationChange::from)
            .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.insert(loww_w_gnd.clone());
        changes = network.coverage_changes(Some(&loww_del), None, &online.iter().collect());
        expected_changes = vec![("LOWW_DEL", Some("LOWW_DEL"), Some("LOWW_GND"))]
            .into_iter()
            .map(StationChange::from)
            .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.remove(&loww_del);
        changes = network.coverage_changes(Some(&loww_gnd), None, &online.iter().collect());
        expected_changes = vec![
            ("LOWW_DEL", Some("LOWW_GND"), Some("LOWW_W_GND")),
            ("LOWW_GND", Some("LOWW_GND"), Some("LOWW_W_GND")),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.remove(&loww_gnd);
        changes = network.coverage_changes(Some(&lovv_ctr), None, &online.iter().collect());
        expected_changes = vec![
            ("LOVV_E1", Some("LOVV_CTR"), None),
            ("LOVV_E2", Some("LOVV_CTR"), None),
            ("LOWW_APP", Some("LOVV_CTR"), None),
            ("LOWW_E_TWR", Some("LOVV_CTR"), None),
            ("LOWW_TWR", Some("LOVV_CTR"), None),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        online.remove(&lovv_ctr);
        changes = network.coverage_changes(Some(&lovv_ctr), None, &online.iter().collect());
        assert!(changes.is_empty());

        changes = network.coverage_changes(None, Some(&edmm_rdg_ctr), &online.iter().collect());
        assert!(changes.is_empty());

        changes = network.coverage_changes(Some(&edmm_rdg_ctr), None, &online.iter().collect());
        assert!(changes.is_empty());

        // Position change with coverage change
        online.clear();
        online.insert(loww_gnd.clone());
        changes =
            network.coverage_changes(Some(&loww_gnd), Some(&loww_w_gnd), &online.iter().collect());
        expected_changes = vec![
            ("LOWW_DEL", Some("LOWW_GND"), Some("LOWW_W_GND")),
            ("LOWW_GND", Some("LOWW_GND"), Some("LOWW_W_GND")),
            ("LOWW_W_GND", Some("LOWW_GND"), Some("LOWW_W_GND")),
        ]
        .into_iter()
        .map(StationChange::from)
        .collect::<Vec<_>>();
        assert_eq!(changes, expected_changes);

        // Position change with no coverage change
        online.insert(edmm_alb_ctr.clone());
        changes = network.coverage_changes(
            Some(&edmm_rdg_ctr),
            Some(&edmm_alb_ctr),
            &online.iter().collect(),
        );
        assert!(changes.is_empty());
    }

    #[test]
    fn load_from_dir_cross_fir_references() {
        let dir = tempfile::tempdir().unwrap();

        // FIR 1: Defines station S
        TestFirBuilder::new("FIR1")
            .station("S", &["P1"])
            .position("P1", &["P"], "118.000", "Tower")
            .create(dir.path());

        // FIR 2: Has Profile 'P' referencing 'S'
        // Needs at least one station to be valid
        TestFirBuilder::new("FIR2")
            .station("DUMMY", &["DUMMY"])
            .position("DUMMY", &["D"], "199.998", "Tower")
            .create(dir.path());

        let fir2 = dir.path().join("FIR2");
        let profile = r#"
            id = "P"
            type = "Geo"
            direction = "row"
            [[children]]
            label = ["A"]
            size = 10.0
            [children.page]
            rows = 1
            [[children.page.keys]]
            label = ["K"]
            station_id = "S"
        "#;
        std::fs::write(fir2.join("profile.toml"), profile).unwrap();

        // Should succeed because S exists in FIR1
        let res = Network::load_from_dir(dir.path());
        res.expect("should load from dir");

        // Now verify invalid reference fails
        let profile_invalid = r#"
            id = "InvalidReference"
            type = "Geo"
            direction = "row"
            [[children]]
            label = ["A"]
            size = 10.0
            [children.page]
            rows = 1
            [[children.page.keys]]
            label = ["K"]
            station_id = "NON_EXISTENT"
        "#;
        std::fs::write(fir2.join("profile.toml"), profile_invalid).unwrap();

        let res = Network::load_from_dir(dir.path());
        assert_matches!(res, Err(errors) if errors.iter().any(|e| causes(e, |x| matches!(x, CoverageError::Validation(ValidationError::MissingReference { field, ref_id }) if field == "station_id" && ref_id == "NON_EXISTENT"))));
    }
}
