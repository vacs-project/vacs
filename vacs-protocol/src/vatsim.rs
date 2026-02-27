use serde::{Deserialize, Serialize};

/// Unique identifier for a VATSIM client (CID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ClientId(String);

/// Unique identifier for a VATSIM position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct PositionId(String);

/// Unique identifier for a VATSIM station.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct StationId(String);

/// Represents a change in station status (online, offline, or handoff).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StationChange {
    /// A station has come online.
    #[serde(rename_all = "camelCase")]
    Online {
        /// The ID of the station that came online.
        station_id: StationId,
        /// The ID of the position that controls the station.
        position_id: PositionId,
    },
    /// A station has been handed off from one position to another.
    #[serde(rename_all = "camelCase")]
    Handoff {
        /// The ID of the station being handed off.
        station_id: StationId,
        /// The ID of the position handing off control over the station.
        from_position_id: PositionId,
        /// The ID of the position receiving control over the station.
        to_position_id: PositionId,
    },
    /// A station has gone offline.
    #[serde(rename_all = "camelCase")]
    Offline {
        /// The ID of the station that went offline.
        station_id: StationId,
    },
}

impl StationChange {
    /// Returns the station ID affected by this change.
    pub fn station_id(&self) -> &StationId {
        match self {
            Self::Online { station_id, .. }
            | Self::Handoff { station_id, .. }
            | Self::Offline { station_id } => station_id,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            StationChange::Online { .. } => "online",
            StationChange::Offline { .. } => "offline",
            StationChange::Handoff { .. } => "handoff",
        }
    }
}

impl ClientId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for ClientId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for ClientId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl From<i32> for ClientId {
    fn from(id: i32) -> Self {
        Self(id.to_string())
    }
}

impl AsRef<str> for ClientId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for ClientId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<String> for ClientId {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl PositionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for PositionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for PositionId {
    fn from(id: String) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl From<&str> for PositionId {
    fn from(id: &str) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl AsRef<str> for PositionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for PositionId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<String> for PositionId {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl StationId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for StationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for StationId {
    fn from(id: String) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl From<&str> for StationId {
    fn from(id: &str) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl AsRef<str> for StationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for StationId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<String> for StationId {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl<S, P> From<(S, Option<P>, Option<P>)> for StationChange
where
    S: Into<StationId>,
    P: Into<PositionId>,
{
    fn from((station_id, from, to): (S, Option<P>, Option<P>)) -> Self {
        match (from, to) {
            (None, Some(to)) => Self::Online {
                station_id: station_id.into(),
                position_id: to.into(),
            },
            (Some(_), None) => Self::Offline {
                station_id: station_id.into(),
            },
            (Some(from), Some(to)) => Self::Handoff {
                station_id: station_id.into(),
                from_position_id: from.into(),
                to_position_id: to.into(),
            },
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_id_creation() {
        let id = StationId::from("loww_twr");
        assert_eq!(id.as_str(), "LOWW_TWR");
        assert_eq!(id.to_string(), "LOWW_TWR");
        assert!(!id.is_empty());

        let empty = StationId::from("");
        assert!(empty.is_empty());
    }

    #[test]
    fn station_id_equality() {
        let id1 = StationId::from("LOWW_TWR");
        let id2 = StationId::from("loww_twr");
        assert_eq!(id1, id2);
    }

    #[test]
    fn position_id_creation() {
        let id = PositionId::from("loww_twr");
        assert_eq!(id.as_str(), "LOWW_TWR");
        assert_eq!(id.to_string(), "LOWW_TWR");
        assert!(!id.is_empty());

        let empty = PositionId::from("");
        assert!(empty.is_empty());
    }

    #[test]
    fn position_id_equality() {
        let id1 = PositionId::from("LOWW_TWR");
        let id2 = PositionId::from("loww_twr");
        assert_eq!(id1, id2);
    }
}
