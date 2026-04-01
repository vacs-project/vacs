pub mod client_page;
pub mod geo;
pub mod tabbed;

use crate::profile::tabbed::Tab;
use crate::profile::{client_page::ClientPageConfig, geo::GeoPageContainer};
use crate::vatsim::StationId;
use serde::{Deserialize, Serialize};

/// Unique identifier for a vacs profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ProfileId(String);

/// Representation of a VACS profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// The unique identifier for this profile.
    pub id: ProfileId,

    /// The type of profile and its associated configuration.
    #[serde(flatten)]
    pub profile_type: ProfileType,
}

/// The specific configuration type of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileType {
    /// A GEO profile with a container-based layout.
    Geo(GeoPageContainer),

    /// A tabbed profile with pages accessible via tabs.
    ///
    /// The list of tabs will always be non-empty.
    Tabbed(Vec<Tab>),
}

/// A page containing direct access keys for stations or clients.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectAccessPage {
    /// The number of rows in the grid (> 0).
    ///
    /// The default layout is optimized for 6 rows. After a seventh row is added,
    /// the space in between the rows is slightly reduced and a scrollbar might
    /// appear automatically.
    pub rows: u8,

    /// The content of the page.
    #[serde(flatten)]
    pub content: DirectAccessPageContent,
}

/// The content of a direct access page.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum DirectAccessPageContent {
    /// A page containing a grid of direct access keys.
    #[serde(rename_all = "camelCase")]
    Keys {
        /// The list of keys on this page.
        ///
        /// Will always be non-empty.
        keys: Vec<DirectAccessKey>,
    },
    /// A specialized client page displaying a list of online clients.
    #[serde(rename_all = "camelCase")]
    ClientPage {
        /// Configuration for the Client page.
        client_page: ClientPageConfig,
    },
}

/// A single key on a direct access page.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectAccessKey {
    /// The text label displayed on the key.
    ///
    /// Will always contain between 0 and 3 lines of text.
    #[serde(deserialize_with = "string_or_vec")]
    pub label: Vec<String>,

    /// The background color of the key, which may be overridden dynamically by state changes (e.g., incoming call). Defaults to gray if unspecified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<CustomButtonColor>,

    /// The optional station ID associated with this key.
    ///
    /// If [`DirectAccessKey::station_id`] and [`DirectAccessKey::page`] are `None`, the DA key will be displayed on the UI but will be non-functional.
    /// This field is mutually exclusive with [`DirectAccessKey::page`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_id: Option<StationId>,

    /// The optional subpage associated with this key.
    ///
    /// If [`DirectAccessKey::station_id`] and [`DirectAccessKey::page`] are `None`, the DA key will be displayed on the UI but will be non-functional.
    /// This field is mutually exclusive with [`DirectAccessKey::station_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<DirectAccessPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomButtonColor {
    Clay,
    Blush,
    Lilac,
    Mint,
    Lavender,
    Taupe,
    Cadet,
    Steel,
    Umber,
    Lagoon,
}

pub fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Label {
        One(String),
        Many(Vec<String>),
    }

    Ok(match Label::deserialize(deserializer)? {
        Label::One(s) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![s]
            }
        }
        Label::Many(v) => v,
    })
}

/// Trait alias for types that can be used as a profile reference in [`ActiveProfile`].
///
/// This trait is sealed, ensuring only the appropriate types can be passed.
pub trait ProfileReference: crate::sealed::Sealed {}

impl crate::sealed::Sealed for ProfileId {}
impl ProfileReference for ProfileId {}

impl crate::sealed::Sealed for Profile {}
impl ProfileReference for Profile {}

/// Represents the currently active profile for a user session.
///
/// The active profile determines which stations are considered "relevant" and thus which
/// status updates (online/offline/handoff) are sent to the client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "profile")]
pub enum ActiveProfile<T: ProfileReference> {
    /// A specific, pre-defined profile is active.
    ///
    /// The client is restricted to the view defined by this profile, meaning only
    /// relevant stations and buttons configured in this profile are displayed and the
    /// appropriate station updates are sent.
    Specific(T),
    /// A custom, client-side profile selection is active.
    ///
    /// This typically corresponds to a "Show All" or "Custom" view where the set of
    /// relevant stations is determined dynamically by the client, or all stations are shown.
    Custom,
    /// No profile is currently active.
    ///
    /// In this state, the client will not receive any station updates, only general
    /// client information updates.
    #[default]
    None,
}

impl ProfileId {
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

impl std::fmt::Display for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for ProfileId {
    fn from(id: String) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl From<&str> for ProfileId {
    fn from(id: &str) -> Self {
        Self(id.to_ascii_uppercase())
    }
}

impl AsRef<str> for ProfileId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for ProfileId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<String> for ProfileId {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Profile({}, {})", self.id, self.profile_type)
    }
}

impl std::fmt::Display for ProfileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileType::Geo(container) => {
                write!(f, "Geo({} nodes)", container.children.len())
            }
            ProfileType::Tabbed(tabs) => {
                let labels: Vec<String> = tabs.iter().map(|t| t.label.join("/")).collect();
                write!(f, "Tabbed([{}])", labels.join(", "))
            }
        }
    }
}

impl<T: ProfileReference + std::fmt::Display> std::fmt::Display for ActiveProfile<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActiveProfile::Specific(profile) => write!(f, "Specific({profile})"),
            ActiveProfile::Custom => write!(f, "Custom"),
            ActiveProfile::None => write!(f, "None"),
        }
    }
}

impl PartialOrd for Profile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}
