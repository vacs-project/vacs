use crate::FacilityType;
use crate::coverage::flight_information_region::FlightInformationRegionId;
use crate::coverage::{CoverageError, ReferenceValidator, ValidationError, Validator};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;
use vacs_protocol::profile::ProfileId;
use vacs_protocol::vatsim::{PositionId, StationId};

static FREQUENCY_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{3}\.\d{3}$").unwrap());

#[derive(Clone)]
pub struct Position {
    pub id: PositionId,
    pub prefixes: HashSet<String>,
    pub frequency: String,
    pub facility_type: FacilityType,
    pub profile_id: Option<ProfileId>,
    pub default_call_sources: Vec<StationId>,
    pub fir_id: FlightInformationRegionId,
    pub controlled_stations: HashSet<StationId>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PositionRaw {
    pub id: PositionId,
    pub prefixes: HashSet<String>,
    pub frequency: String,
    pub facility_type: FacilityType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<ProfileId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_call_sources: Vec<StationId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionConfigFile {
    pub positions: Vec<PositionRaw>,
}

impl std::fmt::Debug for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Position")
            .field("id", &self.id)
            .field("prefixes", &self.prefixes.len())
            .field("frequency", &self.frequency)
            .field("facility_type", &self.facility_type)
            .field("profile_id", &self.profile_id)
            .field("default_call_sources", &self.default_call_sources.len())
            .field("fir_id", &self.fir_id)
            .field("controlled_stations", &self.controlled_stations.len())
            .finish()
    }
}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Position {
    pub(super) fn from_raw(
        position_raw: PositionRaw,
        fir_id: impl Into<FlightInformationRegionId>,
    ) -> Result<Self, CoverageError> {
        position_raw.validate()?;

        Ok(Self {
            id: position_raw.id,
            prefixes: position_raw.prefixes,
            frequency: position_raw.frequency,
            facility_type: position_raw.facility_type,
            profile_id: position_raw.profile_id,
            default_call_sources: position_raw.default_call_sources,
            fir_id: fir_id.into(),
            controlled_stations: HashSet::new(),
        })
    }
}

impl std::fmt::Debug for PositionRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PositionRaw")
            .field("id", &self.id)
            .field("prefixes", &self.prefixes.len())
            .field("frequency", &self.frequency)
            .field("facility_type", &self.facility_type)
            .field("profile_id", &self.profile_id)
            .field("default_call_sources", &self.default_call_sources.len())
            .finish()
    }
}

impl Validator for PositionRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.id.is_empty() {
            return Err(ValidationError::Empty {
                field: "id".to_string(),
            }
            .into());
        }
        if self.prefixes.is_empty() || self.prefixes.iter().any(|p| p.is_empty()) {
            return Err(ValidationError::Empty {
                field: "prefixes".to_string(),
            }
            .into());
        }
        if self.frequency.is_empty() {
            return Err(ValidationError::Empty {
                field: "frequency".to_string(),
            }
            .into());
        } else if !FREQUENCY_REGEX.is_match(&self.frequency) {
            return Err(ValidationError::InvalidFormat {
                field: "frequency".to_string(),
                value: self.frequency.clone(),
                reason: "must match pattern XXX.XXX".to_string(),
            }
            .into());
        }
        if self.facility_type == FacilityType::Unknown {
            return Err(ValidationError::InvalidValue {
                field: "facility_type".to_string(),
                value: "Unknown".to_string(),
                reason: "must not be Unknown".to_string(),
            }
            .into());
        }
        if let Ok(id_facility_type) = self.id.as_str().parse::<FacilityType>()
            && id_facility_type != self.facility_type
        {
            return Err(ValidationError::InvalidValue {
                field: "facility_type".to_string(),
                value: self.facility_type.as_str().to_string(),
                reason: format!(
                    "id `{}` implies `{}`",
                    self.id.as_str(),
                    id_facility_type.as_str()
                ),
            }
            .into());
        }
        if self.profile_id.as_ref().is_some_and(|p| p.is_empty()) {
            return Err(ValidationError::Empty {
                field: "profile_id".to_string(),
            }
            .into());
        }

        let mut seen_default_call_sources = HashSet::new();
        for station_id in &self.default_call_sources {
            if !seen_default_call_sources.insert(station_id) {
                return Err(ValidationError::InvalidValue {
                    field: "default_call_sources".to_string(),
                    value: station_id.to_string(),
                    reason: "duplicate entry".to_string(),
                }
                .into());
            }
        }

        Ok(())
    }
}

impl ReferenceValidator<ProfileId> for PositionRaw {
    fn validate_references(&self, profiles: &HashSet<&ProfileId>) -> Result<(), CoverageError> {
        if let Some(profile_id) = &self.profile_id
            && !profiles.contains(profile_id)
        {
            return Err(ValidationError::MissingReference {
                field: "profile_id".to_string(),
                ref_id: profile_id.to_string(),
            }
            .into());
        }
        Ok(())
    }
}

impl ReferenceValidator<StationId> for PositionRaw {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        for station_id in &self.default_call_sources {
            if !stations.contains(station_id) {
                return Err(ValidationError::MissingReference {
                    field: "default_call_sources".to_string(),
                    ref_id: station_id.to_string(),
                }
                .into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_matches, assert_ne};

    #[test]
    fn position_raw_valid() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert!(raw.validate().is_ok());
    }

    #[test]
    fn position_raw_invalid_id() {
        let raw = PositionRaw {
            id: "".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "id"
        );
    }

    #[test]
    fn position_raw_invalid_prefixes() {
        // Empty hashset
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::new(),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "prefixes"
        );

        // Empty string in hashset
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "prefixes"
        );
    }

    #[test]
    fn position_raw_invalid_frequency() {
        // Empty
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "frequency"
        );

        // Bad format
        let bad_freqs = vec![
            "119.4", "119.40", "119.4000", "119,400", "abc.def", "119.40a",
        ];
        for freq in bad_freqs {
            let raw = PositionRaw {
                id: "LOWW_TWR".into(),
                prefixes: HashSet::from(["LOWW".to_string()]),
                frequency: freq.to_string(),
                facility_type: FacilityType::Tower,
                profile_id: Some(ProfileId::from("LOWW")),
                default_call_sources: Vec::new(),
            };
            assert_matches!(
                raw.validate(),
                Err(CoverageError::Validation(ValidationError::InvalidFormat { field, .. })) if field == "frequency",
                "Should fail for frequency: {freq}"
            );
        }
    }

    #[test]
    fn position_raw_invalid_facility_type() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Unknown,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, value, .. }))
                if field == "facility_type" && value == "Unknown"
        );
    }

    #[test]
    fn position_raw_invalid_profile_id() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "profile_id"
        );
    }

    #[test]
    fn position_conversion() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        let pos = Position::from_raw(raw, "LOVV").unwrap();
        assert_eq!(pos.id.as_str(), "LOWW_TWR");
        assert!(
            pos.profile_id
                .is_some_and(|id| id == ProfileId::from("LOWW"))
        );
        assert_eq!(pos.fir_id.as_str(), "LOVV");
    }

    #[test]
    fn position_equality() {
        let p1 = Position {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
            fir_id: FlightInformationRegionId::from("LOVV"),
            controlled_stations: HashSet::new(),
        };
        let p2 = Position {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::new(),                  // Different content
            frequency: "119.000".to_string(),          // Different content
            facility_type: FacilityType::Ground,       // Different content
            profile_id: Some(ProfileId::from("LOVV")), // Different content
            default_call_sources: Vec::from(["LOVV_N1".into()]), // Different content
            fir_id: FlightInformationRegionId::from("LOVV"),
            controlled_stations: HashSet::new(),
        };
        assert_eq!(p1, p2); // Should be equal because IDs are equal

        let p3 = Position {
            id: "LOWW_GND".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
            fir_id: FlightInformationRegionId::from("LOVV"),
            controlled_stations: HashSet::new(),
        };
        assert_ne!(p1, p3);
    }

    #[test]
    fn validate_profile_references() {
        let profile_id = ProfileId::from("LOWW");
        let other_profile_id = ProfileId::from("EDMM");
        let valid_profiles = HashSet::from([&profile_id, &other_profile_id]);

        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(profile_id.clone()),
            default_call_sources: Vec::new(),
        };
        assert!(raw.validate_references(&valid_profiles).is_ok());

        let raw_missing = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("UNKNOWN")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw_missing.validate_references(&valid_profiles),
            Err(CoverageError::Validation(ValidationError::MissingReference { field, ref_id }))
            if field == "profile_id" && ref_id == "UNKNOWN"
        );

        let raw_none = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: None,
            default_call_sources: Vec::new(),
        };
        assert!(raw_none.validate_references(&valid_profiles).is_ok());
    }

    #[test]
    fn validate_default_call_source_references() {
        let station_id = StationId::from("LOVV_N1");
        let other_station_id = StationId::from("LOVV_S1");
        let valid_stations = HashSet::from([&station_id, &other_station_id]);

        let raw = PositionRaw {
            id: "LOVV_CTR".into(),
            prefixes: HashSet::from(["LOVV".to_string()]),
            frequency: "132.600".to_string(),
            facility_type: FacilityType::Enroute,
            profile_id: Some(ProfileId::from("LOVV")),
            default_call_sources: valid_stations.iter().cloned().cloned().collect(),
        };
        assert!(raw.validate_references(&valid_stations).is_ok());

        let raw_missing = PositionRaw {
            id: "LOVV_CTR".into(),
            prefixes: HashSet::from(["LOVV".to_string()]),
            frequency: "132.600".to_string(),
            facility_type: FacilityType::Enroute,
            profile_id: Some(ProfileId::from("LOVV")),
            default_call_sources: Vec::from([StationId::from("UNKNOWN")]),
        };
        assert_matches!(
            raw_missing.validate_references(&valid_stations),
            Err(CoverageError::Validation(ValidationError::MissingReference { field, ref_id }))
            if field == "default_call_sources" && ref_id == "UNKNOWN"
        );

        let raw_none = PositionRaw {
            id: "LOVV_CTR".into(),
            prefixes: HashSet::from(["LOVV".to_string()]),
            frequency: "132.600".to_string(),
            facility_type: FacilityType::Enroute,
            profile_id: Some(ProfileId::from("LOVV")),
            default_call_sources: Vec::new(),
        };
        assert!(raw_none.validate_references(&valid_stations).is_ok());
    }

    #[test]
    fn position_raw_id_facility_type_mismatch() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Ground,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, .. }))
                if field == "facility_type"
        );
    }

    #[test]
    fn position_raw_id_facility_type_match() {
        let raw = PositionRaw {
            id: "LOWW_TWR".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::Tower,
            profile_id: Some(ProfileId::from("LOWW")),
            default_call_sources: Vec::new(),
        };
        assert!(raw.validate().is_ok());
    }

    #[test]
    fn position_raw_id_no_facility_suffix() {
        let raw = PositionRaw {
            id: "LOWW_ATIS".into(),
            prefixes: HashSet::from(["LOWW".to_string()]),
            frequency: "122.955".to_string(),
            facility_type: FacilityType::Approach,
            profile_id: None,
            default_call_sources: Vec::new(),
        };
        assert!(raw.validate().is_ok());

        let raw = PositionRaw {
            id: "LON_SC".into(),
            prefixes: HashSet::from(["LON".to_string()]),
            frequency: "132.605".to_string(),
            facility_type: FacilityType::Enroute,
            profile_id: None,
            default_call_sources: Vec::new(),
        };
        assert!(raw.validate().is_ok());
    }

    #[test]
    fn position_raw_duplicate_default_call_source() {
        let raw = PositionRaw {
            id: "LOVV_CTR".into(),
            prefixes: HashSet::from(["LOVV".to_string()]),
            frequency: "132.600".to_string(),
            facility_type: FacilityType::Enroute,
            profile_id: Some(ProfileId::from("LOVV")),
            default_call_sources: Vec::from([
                StationId::from("LOVV_N1"),
                StationId::from("LOVV_B1"),
                StationId::from("LOVV_N1"),
            ]),
        };
        assert_matches!(
            raw.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, value, reason })) if field == "default_call_sources" && value == "LOVV_N1" && reason == "duplicate entry"
        );
    }
}
