#[cfg(feature = "coverage")]
pub mod coverage;
#[cfg(feature = "data-feed")]
pub mod data_feed;
#[cfg(feature = "slurper")]
pub mod slurper;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
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
    /// Visibility range in nautical miles, if the data source reports one.
    pub visual_range: Option<u32>,
}

impl ControllerInfo {
    /// Indexes controllers by CID.
    ///
    /// A CID can hold several connections at once, for example a controller
    /// staffing a position while also running TowerView as an observer. Only
    /// one of those is the position the controller is working: core facilities
    /// (ramp through flight service station) win over auxiliary ones (radio,
    /// traffic flow), which win over [`FacilityType::Unknown`]. Within a
    /// class, a confirmed visibility range beats a missing one, which beats a
    /// zero one, then the higher facility type wins. Remaining ties are
    /// broken by callsign, so the result never depends on the order of
    /// entries in the feed.
    ///
    /// Supervisor (`_SUP`) connections are dropped entirely.
    pub fn index_by_cid<I>(controllers: I) -> HashMap<ClientId, ControllerInfo>
    where
        I: IntoIterator<Item = ControllerInfo>,
    {
        fn rank(controller: &ControllerInfo) -> (u8, u8, FacilityType, &str) {
            let class = match controller.facility_type {
                FacilityType::Unknown => 0,
                FacilityType::TrafficFlow | FacilityType::Radio => 1,
                FacilityType::Ramp
                | FacilityType::Delivery
                | FacilityType::Ground
                | FacilityType::Tower
                | FacilityType::Approach
                | FacilityType::Departure
                | FacilityType::Enroute
                | FacilityType::FlightServiceStation => 2,
            };
            let range = match controller.visual_range {
                Some(0) => 0,
                None => 1,
                Some(_) => 2,
            };
            (
                class,
                range,
                controller.facility_type,
                controller.callsign.as_str(),
            )
        }

        let mut by_cid: HashMap<ClientId, ControllerInfo> = HashMap::new();

        for controller in controllers {
            if controller.callsign.ends_with("_SUP") {
                continue;
            }

            match by_cid.entry(controller.cid.clone()) {
                Entry::Occupied(mut entry) => {
                    if rank(&controller) > rank(entry.get()) {
                        entry.insert(controller);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(controller);
                }
            }
        }

        by_cid
    }
}

/// Enum representing the different VATSIM facility types as parsed from their respective callsign suffixes
/// (in accordance with the [VATSIM GCAP](https://vatsim.net/docs/policy/global-controller-administration-policy).
///
/// Variants are declared in ascending priority order: the derived [`Ord`] picks
/// the worked position among simultaneous connections, with auxiliary
/// facilities below the core controlling hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum FacilityType {
    #[default]
    Unknown,
    TrafficFlow,
    Radio,
    Ramp,
    Delivery,
    Ground,
    Tower,
    Approach,
    Departure,
    Enroute,
    FlightServiceStation,
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

    fn controller(cid: &str, callsign: &str) -> ControllerInfo {
        ControllerInfo {
            cid: ClientId::from(cid),
            callsign: callsign.to_string(),
            frequency: "119.400".to_string(),
            facility_type: FacilityType::from(callsign),
            visual_range: None,
        }
    }

    fn controller_with_range(
        cid: &str,
        callsign: &str,
        visual_range: Option<u32>,
    ) -> ControllerInfo {
        ControllerInfo {
            visual_range,
            ..controller(cid, callsign)
        }
    }

    #[test]
    fn index_by_cid_prefers_a_known_facility_type_over_an_observer() {
        for entries in [
            vec![
                controller("1000001", "LOWW_OBS"),
                controller("1000001", "LOWW_TWR"),
            ],
            vec![
                controller("1000001", "LOWW_TWR"),
                controller("1000001", "LOWW_OBS"),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid.len(), 1);
            assert_eq!(
                by_cid[&ClientId::from("1000001")].callsign,
                "LOWW_TWR",
                "the worked position must win over the observer connection"
            );
        }
    }

    #[test]
    fn index_by_cid_resolves_known_facility_duplicates_order_independently() {
        for entries in [
            vec![
                controller("1000001", "LOWW_GND"),
                controller("1000001", "LOWW_TWR"),
            ],
            vec![
                controller("1000001", "LOWW_TWR"),
                controller("1000001", "LOWW_GND"),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid.len(), 1);
            assert_eq!(
                by_cid[&ClientId::from("1000001")].callsign,
                "LOWW_TWR",
                "the highest facility type must win regardless of feed order"
            );
        }
    }

    #[test]
    fn index_by_cid_ranks_auxiliary_facilities_below_the_core_hierarchy() {
        for entries in [
            vec![
                controller("1000001", "LOVV_FMP"),
                controller("1000001", "LOVV_CTR"),
            ],
            vec![
                controller("1000001", "LOVV_CTR"),
                controller("1000001", "LOVV_FMP"),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOVV_CTR");
        }
    }

    #[test]
    fn index_by_cid_ranks_flight_service_station_above_enroute() {
        for entries in [
            vec![
                controller("1000001", "LOVV_CTR"),
                controller("1000001", "LOVV_FSS"),
            ],
            vec![
                controller("1000001", "LOVV_FSS"),
                controller("1000001", "LOVV_CTR"),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOVV_FSS");
        }
    }

    #[test]
    fn index_by_cid_breaks_facility_ties_by_callsign() {
        for entries in [
            vec![
                controller("1000001", "LOWW_TWR"),
                controller("1000001", "LOWW_W_TWR"),
            ],
            vec![
                controller("1000001", "LOWW_W_TWR"),
                controller("1000001", "LOWW_TWR"),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOWW_W_TWR");
        }
    }

    #[test]
    fn index_by_cid_deprioritizes_zero_visibility_range_connections() {
        for entries in [
            vec![
                controller_with_range("1000001", "LOWW_APP", Some(50)),
                controller_with_range("1000001", "LOWW_DEP", Some(0)),
            ],
            vec![
                controller_with_range("1000001", "LOWW_DEP", Some(0)),
                controller_with_range("1000001", "LOWW_APP", Some(50)),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOWW_APP");
        }
    }

    #[test]
    fn index_by_cid_prefers_a_zero_range_core_facility_over_an_auxiliary() {
        for entries in [
            vec![
                controller_with_range("1000001", "LOWW_GND", Some(0)),
                controller_with_range("1000001", "LOWW_FMP", Some(50)),
            ],
            vec![
                controller_with_range("1000001", "LOWW_FMP", Some(50)),
                controller_with_range("1000001", "LOWW_GND", Some(0)),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOWW_GND");
        }
    }

    #[test]
    fn index_by_cid_prefers_a_confirmed_range_over_a_missing_one() {
        for entries in [
            vec![
                controller_with_range("1000001", "LOWW_TWR", Some(50)),
                controller_with_range("1000001", "LOWW_APP", None),
            ],
            vec![
                controller_with_range("1000001", "LOWW_APP", None),
                controller_with_range("1000001", "LOWW_TWR", Some(50)),
            ],
        ] {
            let by_cid = ControllerInfo::index_by_cid(entries);

            assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOWW_TWR");
        }
    }

    #[test]
    fn index_by_cid_keeps_a_zero_range_connection_over_an_observer() {
        let by_cid = ControllerInfo::index_by_cid(vec![
            controller_with_range("1000001", "LOWW_TWR", Some(0)),
            controller_with_range("1000001", "LOWW_OBS", Some(300)),
        ]);

        assert_eq!(by_cid[&ClientId::from("1000001")].callsign, "LOWW_TWR");
    }

    #[test]
    fn index_by_cid_keeps_an_observer_when_it_is_the_only_connection() {
        let by_cid = ControllerInfo::index_by_cid(vec![controller("1000001", "LOWW_OBS")]);

        assert_eq!(
            by_cid[&ClientId::from("1000001")].facility_type,
            FacilityType::Unknown
        );
    }

    #[test]
    fn index_by_cid_drops_supervisor_connections() {
        let by_cid = ControllerInfo::index_by_cid(vec![
            controller("1000001", "XX_SUP"),
            controller("1000000", "LOWW_TWR"),
        ]);

        assert_eq!(by_cid.len(), 1);
        assert!(!by_cid.contains_key(&ClientId::from("1000001")));
    }

    #[test]
    fn index_by_cid_keeps_separate_cids_apart() {
        let by_cid = ControllerInfo::index_by_cid(vec![
            controller("1000001", "LOWW_TWR"),
            controller("1000000", "LOWW_GND"),
        ]);

        assert_eq!(by_cid.len(), 2);
    }

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
