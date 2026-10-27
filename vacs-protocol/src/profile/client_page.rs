use serde::{Deserialize, Serialize};

/// Mode for controlling how frequencies are displayed on DA keys of the Client page.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum FrequencyDisplayMode {
    /// Hide frequencies for all clients.
    HideAll,
    /// Always show frequencies for all clients.
    #[default]
    ShowAll,
}

/// Mode for controlling how DA keys are grouped on the Client page.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ClientGroupMode {
    /// Don't group.
    None,
    /// Group by the first two letters (FIR) of the display name.
    Fir,
    /// Group by the first four letters (ICAO code) of the display name.
    Icao,
    /// First, group by the first two letters (FIR), then by the first four letters (ICAO code) of the display name.
    #[default]
    FirAndIcao,
}

/// Configuration for the Client page, displaying all currently connected clients.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema), schemars(default))]
pub struct ClientPageConfig {
    /// Optional list of callsign patterns to include.
    ///
    /// - If this list is empty, all clients are eligible to be shown (subject to `exclude`).
    /// - If this list is not empty, only clients matching at least one pattern are eligible to be shown.
    ///
    /// Glob syntax is supported: `"LO*"`, `"LOWW_*"`, `"*_APP"`, …
    /// Matching is case-insensitive.
    ///
    /// Example:
    ///   `["LO*", "EDDM_*", "EDMM_*"]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,

    /// Optional list of callsign patterns to exclude.
    ///
    /// - Clients matching any pattern here are never shown, even if they match an `include` rule.
    ///
    /// Glob syntax is supported: `"LO*"`, `"LOWW_*"`, `"*_APP"`, …
    /// Matching is case-insensitive.
    ///
    /// Example:
    ///   `["*_TWR", "*_GND", "*_DEL"]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// Optional ordered list of callsign patterns used to assign priority.
    ///
    /// The *first* matching pattern in the list determines the client's
    /// priority bucket. Earlier entries = higher priority.
    ///
    /// Glob syntax is supported: `"LO*"`, `"LOWW_*"`, `"*_APP"`, …
    /// Matching is case-insensitive.
    ///
    /// Example:
    ///   `["LOVV_*", "LOWW_*_APP", "LOWW_*_TWR", "LOWW_*"]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priority: Vec<String>,

    /// Control how frequencies are displayed on the DA keys.
    ///
    /// - `ShowAll`: Show frequency for all clients (default).
    /// - `HideAll`: Never show frequencies.
    #[serde(default)]
    pub frequencies: FrequencyDisplayMode,

    /// Control how DA keys are grouped.
    ///
    /// - `None`: Don't group.
    /// - `Fir`: Group by the first two letters (FIR) of the display name.
    /// - `FirAndIcao`: First, group by the first two letters (FIR), then by the first four letters
    ///   (ICAO code) of the display name (default).
    /// - `Icao`: Group by the first four letters (ICAO code) of the display name.
    #[serde(default)]
    pub grouping: ClientGroupMode,
}

impl Default for ClientPageConfig {
    fn default() -> Self {
        Self {
            include: vec![],
            exclude: vec![],
            priority: ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
                .into_iter()
                .map(String::from)
                .collect(),
            frequencies: FrequencyDisplayMode::ShowAll,
            grouping: ClientGroupMode::FirAndIcao,
        }
    }
}
