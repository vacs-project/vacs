#[cfg(feature = "coverage")]
pub mod coverage;
#[cfg(feature = "data-feed")]
pub mod data_feed;
#[cfg(feature = "slurper")]
pub mod slurper;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;
use thiserror::Error;
use vacs_protocol::vatsim::ClientId;

#[cfg(any(feature = "data-feed", feature = "slurper"))]
/// User-Agent string used for all HTTP requests.
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown facility type: {0}")]
    UnknownFacilityType(String),
    #[error(transparent)]
    #[cfg(feature = "coverage")]
    Coverage(#[from] coverage::CoverageError),
    #[error(transparent)]
    #[cfg(feature = "slurper")]
    Slurper(#[from] slurper::SlurperError),
    #[error(transparent)]
    #[cfg(feature = "data-feed")]
    DataFeed(#[from] data_feed::DataFeedError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControllerInfo {
    pub cid: ClientId,
    pub callsign: String,
    pub frequency: String,
    pub facility_type: FacilityType,
}

/// Enum representing the different VATSIM facility types as parsed from their respective callsign suffixes
/// (in accordance with the [VATSIM GCAP](https://vatsim.net/docs/policy/global-controller-administration-policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum FacilityType {
    #[default]
    Unknown,
    Ramp,
    Delivery,
    Ground,
    Tower,
    Approach,
    Departure,
    Enroute,
    FlightServiceStation,
    Radio,
    TrafficFlow,
}

impl FacilityType {
    pub const ALL: &[Self] = &[
        FacilityType::Ramp,
        FacilityType::Delivery,
        FacilityType::Ground,
        FacilityType::Tower,
        FacilityType::Approach,
        FacilityType::Departure,
        FacilityType::Enroute,
        FacilityType::FlightServiceStation,
        FacilityType::Radio,
        FacilityType::TrafficFlow,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            FacilityType::Ramp => "RMP",
            FacilityType::Delivery => "DEL",
            FacilityType::Ground => "GND",
            FacilityType::Tower => "TWR",
            FacilityType::Approach => "APP",
            FacilityType::Departure => "DEP",
            FacilityType::Enroute => "CTR",
            FacilityType::FlightServiceStation => "FSS",
            FacilityType::Radio => "RDO",
            FacilityType::TrafficFlow => "FMP",
            FacilityType::Unknown => "UNKNOWN",
        }
    }

    pub fn from_vatsim_facility(facility: u8) -> Self {
        FacilityType::try_from(facility).unwrap_or_default()
    }
}

impl FromStr for FacilityType {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let s = s.to_ascii_uppercase();
        let facility_suffix = s.split('_').next_back().unwrap_or_default();
        match facility_suffix {
            "RMP" | "RAMP" => Ok(FacilityType::Ramp),
            "DEL" | "DELIVERY" => Ok(FacilityType::Delivery),
            "GND" | "GROUND" => Ok(FacilityType::Ground),
            "TWR" | "TOWER" => Ok(FacilityType::Tower),
            "APP" | "APPROACH" => Ok(FacilityType::Approach),
            "DEP" | "DEPARTURE" => Ok(FacilityType::Departure),
            "CTR" | "CENTER" | "ENROUTE" => Ok(FacilityType::Enroute),
            "FSS" | "FLIGHTSERVICESTATION" => Ok(FacilityType::FlightServiceStation),
            "RDO" | "RADIO" => Ok(FacilityType::Radio),
            "TMU" | "TRAFFICMANAGEMENTUNIT" | "FMP" | "FLOWMANAGEMENTPOSITION" | "TRAFFICFLOW" => {
                Ok(FacilityType::TrafficFlow)
            }
            other => Err(Error::UnknownFacilityType(other.to_string())),
        }
    }
}

impl From<&str> for FacilityType {
    fn from(value: &str) -> Self {
        value.parse().unwrap_or_default()
    }
}

impl From<String> for FacilityType {
    fn from(value: String) -> Self {
        value.as_str().parse().unwrap_or_default()
    }
}

impl TryFrom<u8> for FacilityType {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(FacilityType::FlightServiceStation),
            2 => Ok(FacilityType::Delivery),
            3 => Ok(FacilityType::Ground),
            4 => Ok(FacilityType::Tower),
            5 => Ok(FacilityType::Approach),
            6 => Ok(FacilityType::Enroute),
            other => Err(Error::UnknownFacilityType(other.to_string())),
        }
    }
}

impl Serialize for FacilityType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FacilityType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FacilityType::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn facility_type_parse_valid() {
        assert_eq!(
            FacilityType::from_str("LOWW_DEL").unwrap(),
            FacilityType::Delivery
        );
        assert_eq!(
            FacilityType::from_str("LOWW_RMP").unwrap(),
            FacilityType::Ramp
        );
        assert_eq!(
            FacilityType::from_str("LOWW_GND").unwrap(),
            FacilityType::Ground
        );
        assert_eq!(
            FacilityType::from_str("LOWW_TWR").unwrap(),
            FacilityType::Tower
        );
        assert_eq!(
            FacilityType::from_str("LOWW_APP").unwrap(),
            FacilityType::Approach
        );
        assert_eq!(
            FacilityType::from_str("LOWW_DEP").unwrap(),
            FacilityType::Departure
        );
        assert_eq!(
            FacilityType::from_str("LOVV_CTR").unwrap(),
            FacilityType::Enroute
        );
        assert_eq!(
            FacilityType::from_str("LOVV_FSS").unwrap(),
            FacilityType::FlightServiceStation
        );
        assert_eq!(
            FacilityType::from_str("LOAV_RDO").unwrap(),
            FacilityType::Radio
        );
        assert_eq!(
            FacilityType::from_str("LOWW_FMP").unwrap(),
            FacilityType::TrafficFlow
        );
    }

    #[test]
    fn facility_type_parse_case_insensitive() {
        assert_eq!(
            FacilityType::from_str("loww_twr").unwrap(),
            FacilityType::Tower
        );
        assert_eq!(
            FacilityType::from_str("LOVV_ctr").unwrap(),
            FacilityType::Enroute
        );
    }

    #[test]
    fn facility_type_parse_full_names() {
        assert_eq!(
            FacilityType::from_str("Delivery").unwrap(),
            FacilityType::Delivery
        );
        assert_eq!(
            FacilityType::from_str("DELIVERY").unwrap(),
            FacilityType::Delivery
        );
        assert_eq!(FacilityType::from_str("Ramp").unwrap(), FacilityType::Ramp);
        assert_eq!(
            FacilityType::from_str("Ground").unwrap(),
            FacilityType::Ground
        );
        assert_eq!(
            FacilityType::from_str("Tower").unwrap(),
            FacilityType::Tower
        );
        assert_eq!(
            FacilityType::from_str("Approach").unwrap(),
            FacilityType::Approach
        );
        assert_eq!(
            FacilityType::from_str("Departure").unwrap(),
            FacilityType::Departure
        );
        assert_eq!(
            FacilityType::from_str("Enroute").unwrap(),
            FacilityType::Enroute
        );
        assert_eq!(
            FacilityType::from_str("FlightServiceStation").unwrap(),
            FacilityType::FlightServiceStation
        );
        assert_eq!(
            FacilityType::from_str("Radio").unwrap(),
            FacilityType::Radio
        );
        assert_eq!(
            FacilityType::from_str("TrafficFlow").unwrap(),
            FacilityType::TrafficFlow
        );
        assert_eq!(
            FacilityType::from_str("FlowManagementPosition").unwrap(),
            FacilityType::TrafficFlow
        );
    }

    #[test]
    fn facility_type_parse_unknown() {
        assert!(matches!(
            FacilityType::from_str("UNKNOWN_FOO"),
            Err(Error::UnknownFacilityType(_))
        ));
    }

    #[test]
    fn facility_type_from_u8() {
        assert_eq!(
            FacilityType::try_from(1).unwrap(),
            FacilityType::FlightServiceStation
        );
        assert_eq!(FacilityType::try_from(2).unwrap(), FacilityType::Delivery);
        assert_eq!(FacilityType::try_from(3).unwrap(), FacilityType::Ground);
        assert_eq!(FacilityType::try_from(4).unwrap(), FacilityType::Tower);
        assert_eq!(FacilityType::try_from(5).unwrap(), FacilityType::Approach);
        assert_eq!(FacilityType::try_from(6).unwrap(), FacilityType::Enroute);
        assert!(FacilityType::try_from(0).is_err());
        assert!(FacilityType::try_from(7).is_err());
    }

    #[test]
    fn facility_type_serialization() {
        assert_eq!(FacilityType::Delivery.as_str(), "DEL");
        assert_eq!(FacilityType::Ramp.as_str(), "RMP");
        assert_eq!(FacilityType::Ground.as_str(), "GND");
        assert_eq!(FacilityType::Tower.as_str(), "TWR");
        assert_eq!(FacilityType::Approach.as_str(), "APP");
        assert_eq!(FacilityType::Departure.as_str(), "DEP");
        assert_eq!(FacilityType::Enroute.as_str(), "CTR");
        assert_eq!(FacilityType::FlightServiceStation.as_str(), "FSS");
        assert_eq!(FacilityType::Radio.as_str(), "RDO");
        assert_eq!(FacilityType::TrafficFlow.as_str(), "FMP");
    }
}
