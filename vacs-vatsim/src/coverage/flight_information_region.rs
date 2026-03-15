use crate::coverage::position::{PositionConfigFile, PositionRaw};
use crate::coverage::profile::{FromRaw, Profile, ProfileRaw};
use crate::coverage::station::{StationConfigFile, StationRaw};
use crate::coverage::{CoverageError, IoError, ValidationError, Validator};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use vacs_protocol::profile::ProfileId;
use vacs_protocol::vatsim::{PositionId, StationId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct FlightInformationRegionId(String);

#[derive(Clone)]
pub struct FlightInformationRegion {
    pub id: FlightInformationRegionId,
    pub stations: HashSet<StationId>,
    pub positions: HashSet<PositionId>,
    pub profiles: HashSet<ProfileId>,
}

#[derive(Clone)]
pub(super) struct FlightInformationRegionRaw {
    pub id: FlightInformationRegionId,
    pub stations: Vec<StationRaw>,
    pub positions: Vec<PositionRaw>,
    pub profiles: HashMap<ProfileId, Profile>,
}

impl std::fmt::Debug for FlightInformationRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightInformationRegion")
            .field("id", &self.id)
            .field("stations", &self.stations.len())
            .field("positions", &self.positions.len())
            .field("profiles", &self.profiles.len())
            .finish()
    }
}

impl PartialEq for FlightInformationRegion {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for FlightInformationRegion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl std::fmt::Debug for FlightInformationRegionRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightInformationRegionRaw")
            .field("id", &self.id)
            .field("stations", &self.stations.len())
            .field("positions", &self.positions.len())
            .field("profiles", &self.profiles.len())
            .finish()
    }
}

impl Validator for FlightInformationRegionRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.id.is_empty() {
            return Err(ValidationError::Empty {
                field: "id".to_string(),
            }
            .into());
        }
        if self.stations.is_empty() {
            return Err(ValidationError::Empty {
                field: "stations".to_string(),
            }
            .into());
        }
        if self.positions.is_empty() {
            return Err(ValidationError::Empty {
                field: "positions".to_string(),
            }
            .into());
        }

        Ok(())
    }
}

impl FlightInformationRegionRaw {
    #[tracing::instrument(level = "trace", skip(dir), fields(dir = tracing::field::Empty))]
    pub fn load_from_dir(dir: impl AsRef<std::path::Path>) -> Result<Self, Vec<CoverageError>> {
        let path = dir.as_ref();
        tracing::Span::current().record("dir", tracing::field::debug(path));
        tracing::trace!("Loading FIR");

        let Some(dir_name) = path.file_name() else {
            tracing::warn!("Missing dir name");
            return Err(vec![
                IoError::Read {
                    path: path.into(),
                    reason: "missing dir name".to_string(),
                }
                .into(),
            ]);
        };
        let Some(dir_name) = dir_name.to_str() else {
            tracing::warn!("Invalid dir name");
            return Err(vec![
                IoError::Read {
                    path: path.into(),
                    reason: "invalid dir name".to_string(),
                }
                .into(),
            ]);
        };

        let mut errors = Vec::new();

        let stations = match Self::read_file::<StationConfigFile>(path, "stations") {
            Ok(config) => config.stations,
            Err(err) => {
                errors.push(err);
                Vec::new()
            }
        };

        let positions = match Self::read_file::<PositionConfigFile>(path, "positions") {
            Ok(config) => config.positions,
            Err(err) => {
                errors.push(err);
                Vec::new()
            }
        };

        let profiles = match Self::read_profiles(path) {
            Ok(profiles) => profiles,
            Err(err) => {
                errors.push(err);
                HashMap::new()
            }
        };

        if !errors.is_empty() {
            return Err(errors);
        }

        let fir_raw = Self {
            id: FlightInformationRegionId::from(dir_name),
            stations,
            positions,
            profiles,
        };

        tracing::trace!(?fir_raw, "Successfully loaded FIR");
        Ok(fir_raw)
    }

    const FILE_EXTENSIONS: &'static [&'static str] = &["toml", "json"];
    fn read_file<T: for<'de> Deserialize<'de>>(
        dir: &std::path::Path,
        kind: &str,
    ) -> Result<T, CoverageError> {
        let path = Self::FILE_EXTENSIONS
            .iter()
            .find_map(|ext| {
                let path = dir.join(std::path::Path::new(kind).with_extension(ext));
                if path.is_file() { Some(path) } else { None }
            })
            .ok_or_else(|| IoError::Read {
                path: dir.into(),
                reason: format!("No {kind} file found"),
            })?;

        Self::parse_file(&path)
    }

    #[tracing::instrument(level = "trace", err)]
    fn parse_file<T: for<'de> Deserialize<'de>>(
        path: &std::path::Path,
    ) -> Result<T, CoverageError> {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        tracing::trace!(?ext, "Reading file");

        let bytes = std::fs::read(path).map_err(|err| IoError::Read {
            path: path.into(),
            reason: err.to_string(),
        })?;

        tracing::trace!(?ext, length = bytes.len(), "Parsing file");
        match ext {
            "toml" => toml::from_slice(&bytes).map_err(|err| IoError::Parse {
                path: path.into(),
                reason: err.to_string(),
            }),
            "json" => serde_json::from_slice(&bytes).map_err(|err| IoError::Parse {
                path: path.into(),
                reason: err.to_string(),
            }),
            _ => {
                tracing::warn!(?ext, "Unsupported file extension");
                Err(IoError::Read {
                    path: path.into(),
                    reason: format!("unsupported file extension: {ext}"),
                })
            }
        }
        .map_err(Into::into)
    }

    #[tracing::instrument(level = "trace", err)]
    fn read_profiles(
        base_dir: &std::path::Path,
    ) -> Result<HashMap<ProfileId, Profile>, CoverageError> {
        let mut profiles = HashMap::new();

        if let Ok(profile_raw) = Self::read_file::<ProfileRaw>(base_dir, "profile") {
            tracing::trace!(?profile_raw.id, "Loaded profile from file");
            profiles.insert(profile_raw.id.clone(), Profile::from_raw(profile_raw)?);
        }

        let profiles_dir = base_dir.join("profiles");
        if profiles_dir.is_dir() {
            let entries = std::fs::read_dir(&profiles_dir).map_err(|err| IoError::Read {
                path: profiles_dir.to_path_buf(),
                reason: err.to_string(),
            })?;

            for entry in entries {
                let entry = entry.map_err(|err| IoError::Read {
                    path: profiles_dir.clone(),
                    reason: err.to_string(),
                })?;
                let path = entry.path();
                if !path.is_file() {
                    tracing::trace!(?path, "Skipping non-directory entry");
                    continue;
                }

                let profile_raw = Self::parse_file::<ProfileRaw>(&path)?;
                tracing::trace!(?profile_raw.id, ?path, "Loaded profile from directory");
                profiles.insert(profile_raw.id.clone(), Profile::from_raw(profile_raw)?);
            }
        }

        tracing::trace!(profiles = profiles.len(), "Loaded profiles");
        Ok(profiles)
    }
}

impl TryFrom<FlightInformationRegionRaw> for FlightInformationRegion {
    type Error = CoverageError;
    fn try_from(value: FlightInformationRegionRaw) -> Result<Self, Self::Error> {
        value.validate()?;

        Ok(Self {
            id: value.id,
            stations: value.stations.iter().map(|s| s.id.clone()).collect(),
            positions: value.positions.iter().map(|p| p.id.clone()).collect(),
            profiles: value.profiles.keys().cloned().collect(),
        })
    }
}

impl FlightInformationRegionId {
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for FlightInformationRegionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for FlightInformationRegionId {
    fn from(value: String) -> Self {
        FlightInformationRegionId(value.to_ascii_uppercase())
    }
}

impl From<&str> for FlightInformationRegionId {
    fn from(value: &str) -> Self {
        FlightInformationRegionId(value.to_ascii_uppercase())
    }
}

impl std::borrow::Borrow<str> for FlightInformationRegionId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_matches};

    #[test]
    fn fir_id_creation() {
        let id = FlightInformationRegionId::from("lovv");
        assert_eq!(id.as_str(), "LOVV");
        assert_eq!(id.to_string(), "LOVV");
        assert!(!id.is_empty());

        let empty = FlightInformationRegionId::from("");
        assert!(empty.is_empty());
    }

    #[test]
    fn fir_id_equality() {
        let id1 = FlightInformationRegionId::from("LOVV");
        let id2 = FlightInformationRegionId::from("lovv");
        assert_eq!(id1, id2);
    }

    #[test]
    fn fir_raw_valid() {
        let raw = FlightInformationRegionRaw {
            id: "LOVV".into(),
            stations: vec![StationRaw {
                id: "LOWW_TWR".into(),
                parent_id: None,
                controlled_by: vec![],
            }],
            positions: vec![PositionRaw {
                id: "LOWW_TWR".into(),
                prefixes: HashSet::from(["LOWW".to_string()]),
                frequency: "119.400".to_string(),
                facility_type: crate::FacilityType::Tower,
                profile_id: Some(ProfileId::from("LOWW")),
                default_call_sources: Vec::new(),
            }],
            profiles: HashMap::new(),
        };
        assert!(raw.validate().is_ok());
    }

    #[test]
    fn fir_raw_invalid_id() {
        let raw = FlightInformationRegionRaw {
            id: "".into(),
            stations: vec![StationRaw {
                id: "LOWW_TWR".into(),
                parent_id: None,
                controlled_by: vec![],
            }],
            positions: vec![PositionRaw {
                id: "LOWW_TWR".into(),
                prefixes: HashSet::from(["LOWW".to_string()]),
                frequency: "119.400".to_string(),
                facility_type: crate::FacilityType::Tower,
                profile_id: Some(ProfileId::from("LOWW")),
                default_call_sources: Vec::new(),
            }],
            profiles: HashMap::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty{field})) if field == "id"
        );
    }

    #[test]
    fn fir_raw_invalid_stations() {
        let raw = FlightInformationRegionRaw {
            id: "LOVV".into(),
            stations: vec![],
            positions: vec![PositionRaw {
                id: "LOWW_TWR".into(),
                prefixes: HashSet::from(["LOWW".to_string()]),
                frequency: "119.400".to_string(),
                facility_type: crate::FacilityType::Tower,
                profile_id: Some(ProfileId::from("LOWW")),
                default_call_sources: Vec::new(),
            }],
            profiles: HashMap::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty{field})) if field == "stations"
        );
    }

    #[test]
    fn fir_raw_invalid_positions() {
        let raw = FlightInformationRegionRaw {
            id: "LOVV".into(),
            stations: vec![StationRaw {
                id: "LOWW_TWR".into(),
                parent_id: None,
                controlled_by: vec![],
            }],
            positions: vec![],
            profiles: HashMap::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty{field})) if field == "positions"
        );
    }

    #[test]
    fn fir_conversion() {
        let raw = FlightInformationRegionRaw {
            id: "LOVV".into(),
            stations: vec![StationRaw {
                id: "LOWW_TWR".into(),
                parent_id: None,
                controlled_by: vec![],
            }],
            positions: vec![PositionRaw {
                id: "LOWW_TWR".into(),
                prefixes: HashSet::from(["LOWW".to_string()]),
                frequency: "119.400".to_string(),
                facility_type: crate::FacilityType::Tower,
                profile_id: Some(ProfileId::from("LOWW")),
                default_call_sources: Vec::new(),
            }],
            profiles: HashMap::new(),
        };
        let fir = FlightInformationRegion::try_from(raw).unwrap();
        assert_eq!(fir.id.as_str(), "LOVV");
        assert!(fir.stations.contains(&StationId::from("LOWW_TWR")));
        assert!(fir.positions.contains(&PositionId::from("LOWW_TWR")));
    }

    #[test]
    fn fir_equality() {
        let f1 = FlightInformationRegion {
            id: "LOVV".into(),
            stations: HashSet::new(),
            positions: HashSet::new(),
            profiles: HashSet::new(),
        };
        let f2 = FlightInformationRegion {
            id: "LOVV".into(),
            stations: HashSet::from(["LOWW_TWR".into()]),
            positions: HashSet::from(["LOWW_TWR".into()]),
            profiles: HashSet::new(),
        };
        assert_eq!(f1, f2); // Should be equal because only IDs check
    }

    #[test]
    fn load_from_dir_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let stations_toml = r#"
            [[stations]]
            id = "LOWW_TWR"
        "#;
        std::fs::write(fir_path.join("stations.toml"), stations_toml).unwrap();

        let positions_toml = r#"
            [[positions]]
            id = "LOWW_TWR"
            prefixes = ["LOWW"]
            frequency = "119.400"
            facility_type = "Tower"
            profile_id = "LOWW"
        "#;
        std::fs::write(fir_path.join("positions.toml"), positions_toml).unwrap();

        let raw = FlightInformationRegionRaw::load_from_dir(&fir_path).expect("Should load");
        assert_eq!(raw.id.as_str(), "LOVV");
        assert_eq!(raw.stations.len(), 1);
        assert_eq!(raw.stations[0].id.as_str(), "LOWW_TWR");
        assert_eq!(raw.positions.len(), 1);
        assert_eq!(raw.positions[0].id.as_str(), "LOWW_TWR");
    }

    #[test]
    fn load_from_dir_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let stations_json = r#"{
            "stations": [
                {
                    "id": "LOWW_TWR"
                }
            ]
        }"#;
        std::fs::write(fir_path.join("stations.json"), stations_json).unwrap();

        let positions_json = r#"{
            "positions": [
                {
                    "id": "LOWW_TWR",
                    "prefixes": ["LOWW"],
                    "frequency": "119.400",
                    "facility_type": "Tower",
                    "profile_id": "LOWW"
                }
            ]
        }"#;
        std::fs::write(fir_path.join("positions.json"), positions_json).unwrap();

        let raw = FlightInformationRegionRaw::load_from_dir(&fir_path).expect("Should load");
        assert_eq!(raw.id.as_str(), "LOVV");
        assert_eq!(raw.stations.len(), 1);
        assert_eq!(raw.stations[0].id.as_str(), "LOWW_TWR");
        assert_eq!(raw.positions.len(), 1);
        assert_eq!(raw.positions[0].id.as_str(), "LOWW_TWR");
    }

    #[test]
    fn load_from_dir_mixed_toml_json() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let stations_toml = r#"
            [[stations]]
            id = "LOWW_TWR"
        "#;
        std::fs::write(fir_path.join("stations.toml"), stations_toml).unwrap();

        let positions_json = r#"{
            "positions": [
                {
                    "id": "LOWW_TWR",
                    "prefixes": ["LOWW"],
                    "frequency": "119.400",
                    "facility_type": "Tower",
                    "profile_id": "LOWW"
                }
            ]
        }"#;
        std::fs::write(fir_path.join("positions.json"), positions_json).unwrap();

        let raw = FlightInformationRegionRaw::load_from_dir(&fir_path).expect("Should load");
        assert_eq!(raw.id.as_str(), "LOVV");
        assert_eq!(raw.stations.len(), 1);
        assert_eq!(raw.stations[0].id.as_str(), "LOWW_TWR");
        assert_eq!(raw.positions.len(), 1);
        assert_eq!(raw.positions[0].id.as_str(), "LOWW_TWR");
    }

    #[test]
    fn load_from_dir_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        // No files
        let res = FlightInformationRegionRaw::load_from_dir(&fir_path);
        // Should have errors for missing stations and positions
        assert_matches!(res, Err(errors) if errors.iter().any(|e| matches!(e, CoverageError::Io(IoError::Read { reason, .. }) if reason.contains("No stations file found")))
            && errors.iter().any(|e| matches!(e, CoverageError::Io(IoError::Read { reason, .. }) if reason.contains("No positions file found"))));

        // Only stations
        let stations_toml = r#"
            [[stations]]
            id = "LOWW_TWR"
            controlled_by = []
        "#;
        std::fs::write(fir_path.join("stations.toml"), stations_toml).unwrap();

        let res = FlightInformationRegionRaw::load_from_dir(&fir_path);
        assert_matches!(res, Err(errors) if errors.iter().any(|e| matches!(e, CoverageError::Io(IoError::Read { reason, .. }) if reason.contains("No positions file found"))));

        // Only positions
        let positions_toml = r#"
            [[positions]]
            id = "LOWW_TWR"
            prefixes = ["LOWW"]
            frequency = "119.400"
            facility_type = "Tower"
            profile_id = "LOWW"
        "#;
        std::fs::write(fir_path.join("positions.toml"), positions_toml).unwrap();
        std::fs::remove_file(fir_path.join("stations.toml")).unwrap();

        let res = FlightInformationRegionRaw::load_from_dir(&fir_path);
        assert_matches!(res, Err(errors) if errors.iter().any(|e| matches!(e, CoverageError::Io(IoError::Read { reason, .. }) if reason.contains("No stations file found"))));
    }

    #[test]
    fn load_from_dir_complex_chain() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        let stations_toml = r#"
            [[stations]]
            id = "LOVV_CTR"
            controlled_by = ["LOVV_CTR"]

            [[stations]]
            id = "LOWW_APP"
            parent_id = "LOVV_CTR"
            controlled_by = ["LOWW_APP", "LOWW_B_APP", "LOWW_P_APP"]

            [[stations]]
            id = "LOWW_TWR"
            parent_id = "LOWW_APP"
            controlled_by = ["LOWW_TWR", "LOWW_E_TWR"]

            [[stations]]
            id = "LOWW_E_TWR"
            parent_id = "LOWW_TWR"
            controlled_by = ["LOWW_E_TWR", "LOWW_TWR"]

            [[stations]]
            id = "LOWW_GND"
            parent_id = "LOWW_E_TWR"
            controlled_by = ["LOWW_GND", "LOWW_W_GND"]

            [[stations]]
            id = "LOWW_DEL"
            parent_id = "LOWW_GND"
            controlled_by = ["LOWW_DEL"]
        "#;
        std::fs::write(fir_path.join("stations.toml"), stations_toml).unwrap();

        let positions_toml = r#"
            [[positions]]
            id = "LOVV_CTR"
            prefixes = ["LOVV"]
            frequency = "135.200"
            facility_type = "Enroute"
            profile_id = "LOVV"

            [[positions]]
            id = "LOWW_APP"
            prefixes = ["LOWW", "LOVV"]
            frequency = "119.400"
            facility_type = "Approach"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_B_APP"
            prefixes = ["LOWW"]
            frequency = "118.500"
            facility_type = "Approach"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_P_APP"
            prefixes = ["LOWW"]
            frequency = "128.950"
            facility_type = "Approach"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_TWR"
            prefixes = ["LOWW"]
            frequency = "119.400"
            facility_type = "Tower"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_E_TWR"
            prefixes = ["LOWW"]
            frequency = "118.775"
            facility_type = "Tower"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_GND"
            prefixes = ["LOWW"]
            frequency = "121.600"
            facility_type = "Ground"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_W_GND"
            prefixes = ["LOWW"]
            frequency = "121.775"
            facility_type = "Ground"
            profile_id = "LOWW"

            [[positions]]
            id = "LOWW_DEL"
            prefixes = ["LOWW"]
            frequency = "122.950"
            facility_type = "Delivery"
            profile_id = "LOWW"
        "#;
        std::fs::write(fir_path.join("positions.toml"), positions_toml).unwrap();

        let raw = FlightInformationRegionRaw::load_from_dir(&fir_path).expect("Should load");
        let fir = FlightInformationRegion::try_from(raw.clone()).expect("Should convert");

        let all_stations: std::collections::HashMap<_, _> =
            raw.stations.iter().map(|s| (s.id.clone(), s)).collect();
        let leaf = all_stations
            .get(&StationId::from("LOWW_DEL"))
            .expect("LOWW_DEL should exist");

        let actual_ids = leaf.resolve_controlled_by(&all_stations).unwrap();

        let expected_ids: Vec<PositionId> = vec![
            "LOWW_DEL",
            "LOWW_GND",
            "LOWW_W_GND",
            "LOWW_E_TWR",
            "LOWW_TWR",
            "LOWW_APP",
            "LOWW_B_APP",
            "LOWW_P_APP",
            "LOVV_CTR",
        ]
        .into_iter()
        .map(PositionId::from)
        .collect();

        assert_eq!(actual_ids, expected_ids);
        assert_eq!(fir.positions.len(), 9);
        assert_eq!(fir.stations.len(), 6);
    }

    #[test]
    fn load_from_dir_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        std::fs::write(fir_path.join("stations.toml"), "invalid toml").unwrap();

        let res = FlightInformationRegionRaw::load_from_dir(&fir_path);
        assert_matches!(res, Err(errors) if matches!(errors[0], CoverageError::Io(IoError::Parse { .. })));
    }

    #[test]
    fn load_from_dir_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        std::fs::write(fir_path.join("stations.json"), "invalid json").unwrap();

        let res = FlightInformationRegionRaw::load_from_dir(&fir_path);
        assert_matches!(res, Err(errors) if matches!(errors[0], CoverageError::Io(IoError::Parse { .. })));
    }

    #[test]
    fn load_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let fir_path = dir.path().join("LOVV");
        std::fs::create_dir(&fir_path).unwrap();

        // Dummy stations/positions
        std::fs::write(
            fir_path.join("stations.toml"),
            "[[stations]]\nid=\"S\"\ncontrolled_by=[]",
        )
        .unwrap();
        std::fs::write(
            fir_path.join("positions.toml"),
            "[[positions]]\nid=\"P\"\nprefixes=[]\nfrequency=\"118.0\"\nfacility_type=\"Tower\"\nprofile_id=\"Other\"",
        )
        .unwrap();

        // profile.toml (Default)
        let default_profile = r#"
            id = "Default"
            type = "Geo"
            direction = "row"
            [[children]]
            label = ["A"]
            size = 10.0
            page.keys = []
            page.rows = 1
        "#;
        std::fs::write(fir_path.join("profile.toml"), default_profile).unwrap();

        // profiles/other.toml
        let profiles_dir = fir_path.join("profiles");
        std::fs::create_dir(&profiles_dir).unwrap();
        let other_profile = r#"
            id = "Other"
            type = "Geo"
            direction = "row"
            [[children]]
            label = ["B"]
            size = 20.0
            page.keys = []
            page.rows = 1
        "#;
        std::fs::write(profiles_dir.join("other.toml"), other_profile).unwrap();

        let raw = FlightInformationRegionRaw::load_from_dir(&fir_path).expect("Should load");
        assert_eq!(raw.profiles.len(), 2);

        let ids: Vec<_> = raw.profiles.keys().map(|i| i.as_str()).collect();
        assert!(ids.contains(&"Default"));
        assert!(ids.contains(&"Other"));
    }
}
