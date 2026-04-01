use crate::coverage::{CoverageError, IoError, ReferenceValidator, ValidationError, Validator};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use vacs_protocol::profile::client_page::ClientPageConfig;
use vacs_protocol::profile::geo::{
    GeoNode, GeoPageButton, GeoPageContainer, GeoPageDivider, JustifyContent,
};
use vacs_protocol::profile::tabbed::Tab;
use vacs_protocol::profile::{
    CustomButtonColor, DirectAccessKey, DirectAccessPage, DirectAccessPageContent,
    Profile as ProtocolProfile, ProfileId, ProfileType,
};
use vacs_protocol::vatsim::StationId;

static GEO_PAGE_CONTAINER_SIZE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+(%|rem)$").unwrap());

#[derive(Clone)]
pub struct Profile {
    pub id: ProfileId,
    pub profile_type: ProfileType,
    pub relevant_station_ids: HashSet<StationId>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProfileRaw {
    pub id: ProfileId,
    #[serde(flatten)]
    pub profile_type: ProfileTypeRaw,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(super) enum ProfileTypeRaw {
    Geo(GeoPageContainerRaw),
    Tabbed { tabs: Vec<TabRaw> },
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TabRaw {
    #[serde(deserialize_with = "vacs_protocol::profile::string_or_vec")]
    pub label: Vec<String>,
    pub page: DirectAccessPageRaw,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GeoPageContainerRaw {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_left: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_right: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<JustifyContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_items: Option<vacs_protocol::profile::geo::AlignItems>,
    pub direction: vacs_protocol::profile::geo::FlexDirection,
    pub children: Vec<GeoNodeRaw>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum GeoNodeRaw {
    Container(GeoPageContainerRaw),
    Button(GeoPageButtonRaw),
    Divider(GeoPageDividerRaw),
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GeoPageButtonRaw {
    #[serde(deserialize_with = "vacs_protocol::profile::string_or_vec")]
    pub label: Vec<String>,
    pub size: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<CustomButtonColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<DirectAccessPageRaw>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_id: Option<StationId>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct GeoPageDividerRaw {
    pub orientation: vacs_protocol::profile::geo::GeoPageDividerOrientation,
    pub thickness: f64,
    pub color: String,
    pub oversize: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct DirectAccessPageRaw {
    pub rows: u8,
    #[serde(flatten)]
    pub content: DirectAccessPageContentRaw,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum DirectAccessPageContentRaw {
    Keys { keys: Vec<DirectAccessKeyRaw> },
    ClientPage { client_page: ClientPageConfig },
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct DirectAccessKeyRaw {
    #[serde(deserialize_with = "vacs_protocol::profile::string_or_vec")]
    pub label: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<CustomButtonColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_id: Option<StationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<DirectAccessPageRaw>,
}

impl Profile {
    pub fn load(path: &PathBuf) -> Result<Self, CoverageError> {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        let bytes = std::fs::read(path).map_err(|err| IoError::Read {
            path: path.into(),
            reason: err.to_string(),
        })?;

        let profile: ProfileRaw = match ext {
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
        }?;

        Profile::from_raw(profile)
    }
}

pub(super) trait FromRaw<T> {
    fn from_raw(raw: T) -> Result<Self, CoverageError>
    where
        Self: Sized;
}

trait StationIdCollector {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>);
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("id", &self.id)
            .field("profile_type", &self.profile_type)
            .field("relevant_stations", &self.relevant_station_ids.len())
            .finish()
    }
}

impl PartialEq for Profile {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for Profile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl FromRaw<ProfileRaw> for Profile {
    fn from_raw(profile_raw: ProfileRaw) -> Result<Self, CoverageError> {
        profile_raw.validate()?;

        let profile_type = match profile_raw.profile_type {
            ProfileTypeRaw::Geo(container) => {
                ProfileType::Geo(GeoPageContainer::from_raw(container)?)
            }
            ProfileTypeRaw::Tabbed { tabs } => ProfileType::Tabbed(
                tabs.into_iter()
                    .map(Tab::from_raw)
                    .collect::<Result<Vec<_>, CoverageError>>()?,
            ),
        };

        let mut relevant_station_ids = HashSet::new();
        profile_type.collect_station_ids(&mut relevant_station_ids);

        Ok(Self {
            id: profile_raw.id,
            profile_type,
            relevant_station_ids,
        })
    }
}

impl From<&Profile> for ProtocolProfile {
    fn from(profile: &Profile) -> Self {
        Self {
            id: profile.id.clone(),
            profile_type: profile.profile_type.clone(),
        }
    }
}

impl ReferenceValidator<StationId> for Profile {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        self.profile_type.validate_references(stations)
    }
}

impl ReferenceValidator<StationId> for ProfileType {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        match self {
            ProfileType::Geo(container) => container.validate_references(stations),
            ProfileType::Tabbed(tabs) => {
                for tab in tabs {
                    tab.validate_references(stations)?;
                }
                Ok(())
            }
        }
    }
}

impl StationIdCollector for ProfileType {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        match self {
            ProfileType::Geo(container) => container.collect_station_ids(ids),
            ProfileType::Tabbed(tabs) => {
                for page in tabs {
                    page.collect_station_ids(ids);
                }
            }
        }
    }
}

impl ReferenceValidator<StationId> for GeoPageContainer {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        for child in &self.children {
            child.validate_references(stations)?;
        }
        Ok(())
    }
}

impl StationIdCollector for GeoPageContainer {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        for child in &self.children {
            child.collect_station_ids(ids);
        }
    }
}

impl ReferenceValidator<StationId> for GeoNode {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        match self {
            GeoNode::Container(c) => c.validate_references(stations),
            GeoNode::Button(b) => b.validate_references(stations),
            GeoNode::Divider(_) => Ok(()),
        }
    }
}

impl StationIdCollector for GeoNode {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        match self {
            GeoNode::Container(c) => c.collect_station_ids(ids),
            GeoNode::Button(b) => b.collect_station_ids(ids),
            GeoNode::Divider(_) => {}
        }
    }
}

impl ReferenceValidator<StationId> for GeoPageButton {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        if let Some(page) = &self.page {
            page.validate_references(stations)?;
        }
        if let Some(station_id) = &self.station_id
            && !stations.contains(station_id)
        {
            return Err(ValidationError::MissingReference {
                field: "station_id".to_string(),
                ref_id: station_id.to_string(),
            }
            .into());
        }
        Ok(())
    }
}

impl StationIdCollector for GeoPageButton {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        if let Some(page) = &self.page {
            page.collect_station_ids(ids);
        }
        if let Some(station_id) = &self.station_id {
            ids.insert(station_id.clone());
        }
    }
}

impl ReferenceValidator<StationId> for DirectAccessPage {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        self.content.validate_references(stations)
    }
}

impl ReferenceValidator<StationId> for DirectAccessPageContent {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        match self {
            DirectAccessPageContent::Keys { keys } => {
                for key in keys {
                    key.validate_references(stations)?;
                }
                Ok(())
            }
            DirectAccessPageContent::ClientPage { .. } => Ok(()),
        }
    }
}

impl StationIdCollector for DirectAccessPage {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        self.content.collect_station_ids(ids);
    }
}

impl StationIdCollector for DirectAccessPageContent {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        match self {
            DirectAccessPageContent::Keys { keys } => {
                for key in keys {
                    if let Some(station_id) = &key.station_id {
                        ids.insert(station_id.clone());
                    }
                    if let Some(page) = &key.page {
                        page.collect_station_ids(ids);
                    }
                }
            }
            DirectAccessPageContent::ClientPage { .. } => {}
        }
    }
}

impl ReferenceValidator<StationId> for DirectAccessKey {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        if let Some(station_id) = &self.station_id
            && !stations.contains(station_id)
        {
            return Err(ValidationError::MissingReference {
                field: "station_id".to_string(),
                ref_id: station_id.to_string(),
            }
            .into());
        }
        Ok(())
    }
}

impl Validator for TabRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.label.is_empty() || self.label.iter().all(|s| s.is_empty()) {
            return Err(ValidationError::Empty {
                field: "label".to_string(),
            }
            .into());
        } else if self.label.len() > 3 {
            return Err(ValidationError::InvalidValue {
                field: "label".to_string(),
                value: format!("{:?}", self.label),
                reason: "cannot have more than 3 lines".to_string(),
            }
            .into());
        }
        self.page.validate()?;
        Ok(())
    }
}

impl ReferenceValidator<StationId> for Tab {
    fn validate_references(&self, stations: &HashSet<&StationId>) -> Result<(), CoverageError> {
        self.page.validate_references(stations)?;
        Ok(())
    }
}

impl StationIdCollector for Tab {
    fn collect_station_ids(&self, ids: &mut HashSet<StationId>) {
        self.page.collect_station_ids(ids);
    }
}

impl FromRaw<TabRaw> for Tab {
    fn from_raw(raw: TabRaw) -> Result<Self, CoverageError> {
        raw.validate()?;
        Ok(Self {
            label: raw.label,
            page: DirectAccessPage::from_raw(raw.page)?,
        })
    }
}

impl FromRaw<GeoPageContainerRaw> for GeoPageContainer {
    fn from_raw(raw: GeoPageContainerRaw) -> Result<Self, CoverageError> {
        raw.validate()?;
        Ok(Self {
            height: raw.height,
            width: raw.width,
            padding: raw.padding,
            padding_left: raw.padding_left,
            padding_right: raw.padding_right,
            padding_top: raw.padding_top,
            padding_bottom: raw.padding_bottom,
            gap: raw.gap,
            justify_content: raw.justify_content,
            align_items: raw.align_items,
            direction: raw.direction,
            children: raw
                .children
                .into_iter()
                .map(GeoNode::from_raw)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl FromRaw<GeoNodeRaw> for GeoNode {
    fn from_raw(raw: GeoNodeRaw) -> Result<Self, CoverageError> {
        match raw {
            GeoNodeRaw::Container(c) => Ok(GeoNode::Container(GeoPageContainer::from_raw(c)?)),
            GeoNodeRaw::Button(b) => Ok(GeoNode::Button(GeoPageButton::from_raw(b)?)),
            GeoNodeRaw::Divider(d) => Ok(GeoNode::Divider(GeoPageDivider::from_raw(d)?)),
        }
    }
}

impl FromRaw<GeoPageButtonRaw> for GeoPageButton {
    fn from_raw(raw: GeoPageButtonRaw) -> Result<Self, CoverageError> {
        raw.validate()?;
        Ok(Self {
            label: raw.label,
            size: raw.size,
            color: raw.color,
            page: raw.page.map(DirectAccessPage::from_raw).transpose()?,
            station_id: raw.station_id,
        })
    }
}

impl FromRaw<GeoPageDividerRaw> for GeoPageDivider {
    fn from_raw(raw: GeoPageDividerRaw) -> Result<Self, CoverageError> {
        raw.validate()?;
        Ok(Self {
            orientation: raw.orientation,
            thickness: raw.thickness,
            color: raw.color,
            oversize: raw.oversize,
        })
    }
}

impl FromRaw<DirectAccessPageRaw> for DirectAccessPage {
    fn from_raw(raw: DirectAccessPageRaw) -> Result<Self, CoverageError> {
        Ok(Self {
            rows: raw.rows,
            content: DirectAccessPageContent::from_raw(raw.content)?,
        })
    }
}

impl FromRaw<DirectAccessPageContentRaw> for DirectAccessPageContent {
    fn from_raw(raw: DirectAccessPageContentRaw) -> Result<Self, CoverageError> {
        match raw {
            DirectAccessPageContentRaw::Keys { keys } => Ok(DirectAccessPageContent::Keys {
                keys: keys
                    .into_iter()
                    .map(DirectAccessKey::try_from)
                    .collect::<Result<_, _>>()?,
            }),
            DirectAccessPageContentRaw::ClientPage { client_page } => {
                Ok(DirectAccessPageContent::ClientPage { client_page })
            }
        }
    }
}

impl TryFrom<DirectAccessKeyRaw> for DirectAccessKey {
    type Error = CoverageError;
    fn try_from(raw: DirectAccessKeyRaw) -> Result<Self, Self::Error> {
        raw.validate()?;
        Ok(Self {
            label: raw.label,
            color: raw.color,
            station_id: raw.station_id,
            page: raw.page.map(DirectAccessPage::from_raw).transpose()?,
        })
    }
}

impl Validator for ProfileRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.id.is_empty() {
            return Err(ValidationError::Empty {
                field: "id".to_string(),
            }
            .into());
        }
        self.profile_type.validate()?;
        Ok(())
    }
}

impl Validator for ProfileTypeRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        match self {
            ProfileTypeRaw::Geo(container) => container.validate(),
            ProfileTypeRaw::Tabbed { tabs } => {
                if tabs.is_empty() {
                    return Err(ValidationError::Empty {
                        field: "tabs".to_string(),
                    }
                    .into());
                }
                for tab in tabs {
                    tab.validate()?;
                }
                Ok(())
            }
        }
    }
}

impl Validator for GeoPageContainerRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if let Some(height) = &self.height
            && !GEO_PAGE_CONTAINER_SIZE_REGEX.is_match(height)
        {
            return Err(ValidationError::InvalidFormat {
                field: "height".to_string(),
                value: height.clone(),
                reason: "must either be provided as percentage or rem".to_string(),
            }
            .into());
        }
        if let Some(width) = &self.width
            && !GEO_PAGE_CONTAINER_SIZE_REGEX.is_match(width)
        {
            return Err(ValidationError::InvalidFormat {
                field: "width".to_string(),
                value: width.clone(),
                reason: "must either be provided as percentage or rem".to_string(),
            }
            .into());
        }
        if let Some(padding) = self.padding
            && padding < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "padding".to_string(),
                value: padding.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if let Some(padding_left) = self.padding_left
            && padding_left < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "padding_left".to_string(),
                value: padding_left.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if let Some(padding_right) = self.padding_right
            && padding_right < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "padding_right".to_string(),
                value: padding_right.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if let Some(padding_top) = self.padding_top
            && padding_top < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "padding_top".to_string(),
                value: padding_top.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if let Some(padding_bottom) = self.padding_bottom
            && padding_bottom < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "padding_bottom".to_string(),
                value: padding_bottom.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if let Some(gap) = self.gap
            && gap < 0.0f64
        {
            return Err(ValidationError::OutOfRange {
                field: "gap".to_string(),
                value: gap.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if self.children.is_empty() {
            return Err(ValidationError::Empty {
                field: "children".to_string(),
            }
            .into());
        }
        for child in &self.children {
            child.validate()?;
        }
        Ok(())
    }
}

impl Validator for GeoNodeRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        match self {
            GeoNodeRaw::Container(c) => c.validate(),
            GeoNodeRaw::Button(b) => b.validate(),
            GeoNodeRaw::Divider(d) => d.validate(),
        }
    }
}

impl Validator for GeoPageButtonRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.label.is_empty() {
            return Err(ValidationError::Empty {
                field: "label".to_string(),
            }
            .into());
        }
        if self.label.len() > 3 {
            return Err(ValidationError::InvalidValue {
                field: "label".to_string(),
                value: format!("{:?}", self.label),
                reason: "cannot have more than 3 lines".to_string(),
            }
            .into());
        }
        if self.size < 0.0f64 {
            return Err(ValidationError::OutOfRange {
                field: "size".to_string(),
                value: self.size.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }

        if self.station_id.is_some() && self.page.is_some() {
            return Err(ValidationError::MutuallyExclusive {
                fields: ["station_id", "page"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            }
            .into());
        }

        if let Some(page) = &self.page {
            page.validate()?;
        }
        Ok(())
    }
}

impl Validator for GeoPageDividerRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.thickness <= 0.0f64 {
            return Err(ValidationError::OutOfRange {
                field: "thickness".to_string(),
                value: self.thickness.to_string(),
                min: 0.0f64.to_string(),
                max: None,
            }
            .into());
        }
        if self.color.is_empty() {
            return Err(ValidationError::Empty {
                field: "color".to_string(),
            }
            .into());
        }
        Ok(())
    }
}

impl Validator for DirectAccessPageRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.rows == 0 {
            return Err(ValidationError::OutOfRange {
                field: "rows".to_string(),
                value: self.rows.to_string(),
                min: 1.to_string(),
                max: None,
            }
            .into());
        }
        self.content.validate()
    }
}

impl Validator for DirectAccessPageContentRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        match self {
            DirectAccessPageContentRaw::Keys { keys } => {
                for key in keys {
                    key.validate()?;
                }
                Ok(())
            }
            DirectAccessPageContentRaw::ClientPage { client_page } => client_page.validate(),
        }
    }
}

impl Validator for DirectAccessKeyRaw {
    fn validate(&self) -> Result<(), CoverageError> {
        if self.label.len() > 3 {
            return Err(ValidationError::InvalidValue {
                field: "label".to_string(),
                value: format!("{:?}", self.label),
                reason: "cannot have more than 3 lines".to_string(),
            }
            .into());
        }

        if self.station_id.is_some() && self.page.is_some() {
            return Err(ValidationError::MutuallyExclusive {
                fields: ["station_id", "page"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            }
            .into());
        }

        if let Some(page) = &self.page {
            page.validate()?;
        }

        Ok(())
    }
}

impl Validator for ClientPageConfig {
    fn validate(&self) -> Result<(), CoverageError> {
        Ok(())
    }
}

impl std::fmt::Debug for ProfileRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileRaw")
            .field("id", &self.id)
            .field("profile_type", &self.profile_type)
            .finish()
    }
}

impl std::fmt::Debug for ProfileTypeRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geo(container) => f.debug_tuple("Geo").field(container).finish(),
            Self::Tabbed { tabs } => f.debug_tuple("Tabbed").field(&tabs.len()).finish(),
        }
    }
}

impl std::fmt::Debug for GeoPageContainerRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoPageContainerRaw")
            .field("direction", &self.direction)
            .field("children", &self.children.len())
            .finish()
    }
}

impl std::fmt::Debug for GeoNodeRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeoNodeRaw::Container(c) => c.fmt(f),
            GeoNodeRaw::Button(b) => b.fmt(f),
            GeoNodeRaw::Divider(d) => d.fmt(f),
        }
    }
}

impl std::fmt::Debug for DirectAccessPageRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectAccessPageRaw")
            .field("rows", &self.rows)
            .field("content", &self.content)
            .finish()
    }
}

impl std::fmt::Debug for DirectAccessPageContentRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keys { keys } => f.debug_struct("Keys").field("keys", &keys.len()).finish(),
            Self::ClientPage { client_page } => f
                .debug_struct("ClientPage")
                .field("client_page", client_page)
                .finish(),
        }
    }
}

impl std::fmt::Debug for GeoPageButtonRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoPageButtonRaw")
            .field("label", &self.label.len())
            .field("size", &self.size)
            .field("page", &self.page)
            .finish()
    }
}

impl std::fmt::Debug for GeoPageDividerRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoPageDividerRaw")
            .field("orientation", &self.orientation)
            .field("thickness", &self.thickness)
            .finish()
    }
}

impl std::fmt::Debug for DirectAccessKeyRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectAccessKeyRaw")
            .field("label", &self.label.len())
            .field("station_id", &self.station_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::{CoverageError, ValidationError};
    use pretty_assertions::assert_matches;
    use vacs_protocol::profile::{client_page::ClientPageConfig, geo::FlexDirection};

    #[test]
    fn profile_raw_validation() {
        let valid_geo = ProfileRaw {
            id: ProfileId::from("geo"),
            profile_type: ProfileTypeRaw::Geo(GeoPageContainerRaw {
                height: None,
                width: None,
                padding: None,
                padding_left: None,
                padding_right: None,
                padding_top: None,
                padding_bottom: None,
                gap: None,
                justify_content: None,
                align_items: None,
                direction: FlexDirection::Row,
                children: vec![GeoNodeRaw::Button(GeoPageButtonRaw {
                    label: vec!["L".to_string()],
                    color: None,
                    size: 1.0,
                    page: None,
                    station_id: None,
                })],
            }),
        };
        assert!(valid_geo.validate().is_ok());

        let empty_id = ProfileRaw {
            id: ProfileId::from(""),
            profile_type: valid_geo.profile_type.clone(),
        };
        assert_matches!(
            empty_id.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "id"
        );
    }

    #[test]
    fn profile_type_geo_validation() {
        let empty = ProfileTypeRaw::Geo(GeoPageContainerRaw {
            height: None,
            width: None,
            padding: None,
            padding_left: None,
            padding_right: None,
            padding_top: None,
            padding_bottom: None,
            gap: None,
            justify_content: None,
            align_items: None,
            direction: FlexDirection::Row,
            children: vec![],
        });
        assert_matches!(
            empty.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "children"
        );
    }

    #[test]
    fn profile_type_tabbed_validation() {
        let valid = ProfileTypeRaw::Tabbed {
            tabs: vec![TabRaw {
                label: vec!["tab1".to_string()],
                page: DirectAccessPageRaw {
                    rows: 1,
                    content: DirectAccessPageContentRaw::Keys { keys: vec![] },
                },
            }],
        };
        assert!(valid.validate().is_ok());

        let empty = ProfileTypeRaw::Tabbed { tabs: vec![] };
        assert_matches!(
            empty.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "tabs"
        );

        let long_label = ProfileTypeRaw::Tabbed {
            tabs: vec![TabRaw {
                label: vec![
                    "tab1".to_string(),
                    "tab2".to_string(),
                    "tab3".to_string(),
                    "tab4".to_string(),
                ],
                page: DirectAccessPageRaw {
                    rows: 1,
                    content: DirectAccessPageContentRaw::Keys { keys: vec![] },
                },
            }],
        };
        assert_matches!(
            long_label.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, .. })) if field == "label"
        );
    }

    #[test]
    fn geo_page_button_validation() {
        let valid = GeoPageButtonRaw {
            label: vec!["L".to_string()],
            color: None,
            size: 10.0f64,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
            station_id: None,
        };
        assert!(valid.validate().is_ok());

        let empty_label = GeoPageButtonRaw {
            label: vec![],
            color: None,
            size: 10.0f64,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
            station_id: None,
        };
        assert_matches!(
            empty_label.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "label"
        );

        let long_label = GeoPageButtonRaw {
            label: vec![
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
            ],
            color: None,
            size: 10.0f64,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
            station_id: None,
        };
        assert_matches!(
            long_label.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, .. })) if field == "label"
        );

        let negative_size = GeoPageButtonRaw {
            label: vec!["L".to_string()],
            color: None,
            size: -10.0f64,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
            station_id: None,
        };
        assert_matches!(
            negative_size.validate(),
            Err(CoverageError::Validation(ValidationError::OutOfRange { field, .. })) if field == "size"
        );

        let mutually_exclusive = GeoPageButtonRaw {
            label: vec!["L".to_string()],
            color: None,
            size: 10.0f64,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
            station_id: Some(StationId::from("S1")),
        };
        assert_matches!(
            mutually_exclusive.validate(),
            Err(CoverageError::Validation(ValidationError::MutuallyExclusive { fields }))
                if fields.contains(&"station_id".to_string()) && fields.contains(&"page".to_string())
        );

        let station_id_only = GeoPageButtonRaw {
            label: vec!["L".to_string()],
            color: None,
            size: 10.0f64,
            page: None,
            station_id: Some(StationId::from("S1")),
        };
        assert!(station_id_only.validate().is_ok());
    }

    #[test]
    fn direct_access_page_validation() {
        let valid = DirectAccessPageRaw {
            rows: 1,
            content: DirectAccessPageContentRaw::Keys { keys: vec![] },
        };
        assert!(valid.validate().is_ok());

        let invalid_rows = DirectAccessPageRaw {
            rows: 0,
            content: DirectAccessPageContentRaw::Keys { keys: vec![] },
        };
        assert_matches!(
            invalid_rows.validate(),
            Err(CoverageError::Validation(ValidationError::OutOfRange{field, ..})) if field == "rows"
        );
    }

    #[test]
    fn direct_access_key_validation() {
        let valid = DirectAccessKeyRaw {
            label: vec!["L".to_string()],
            color: None,
            station_id: Some(StationId::from("S1")),
            page: None,
        };
        assert!(valid.validate().is_ok());

        let valid = DirectAccessKeyRaw {
            label: vec!["L".to_string()],
            color: None,
            station_id: None,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
        };
        assert!(valid.validate().is_ok());

        let valid = DirectAccessKeyRaw {
            label: vec!["L".to_string()],
            color: None,
            station_id: None,
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::ClientPage {
                    client_page: ClientPageConfig::default(),
                },
            }),
        };
        assert!(valid.validate().is_ok());

        let invalid_fields = DirectAccessKeyRaw {
            label: vec!["L".to_string()],
            color: None,
            station_id: Some(StationId::from("S1")),
            page: Some(DirectAccessPageRaw {
                rows: 1,
                content: DirectAccessPageContentRaw::Keys { keys: vec![] },
            }),
        };
        assert_matches!(
            invalid_fields.validate(),
            Err(CoverageError::Validation(ValidationError::MutuallyExclusive { fields }))
                if fields.contains(&"station_id".to_string()) && fields.contains(&"page".to_string())
        );
    }

    #[test]
    fn profile_relevant_stations() {
        let raw = ProfileRaw {
            id: ProfileId::from("test"),
            profile_type: ProfileTypeRaw::Geo(GeoPageContainerRaw {
                height: None,
                width: None,
                padding: None,
                padding_left: None,
                padding_right: None,
                padding_top: None,
                padding_bottom: None,
                gap: None,
                justify_content: None,
                align_items: None,
                direction: FlexDirection::Row,
                children: vec![
                    GeoNodeRaw::Button(GeoPageButtonRaw {
                        label: vec!["B1".to_string()],
                        color: None,
                        size: 10.0,
                        page: Some(DirectAccessPageRaw {
                            rows: 1,
                            content: DirectAccessPageContentRaw::Keys {
                                keys: vec![DirectAccessKeyRaw {
                                    label: vec!["K1".to_string()],
                                    color: None,
                                    station_id: Some(StationId::from("S1")),
                                    page: None,
                                }],
                            },
                        }),
                        station_id: None,
                    }),
                    GeoNodeRaw::Button(GeoPageButtonRaw {
                        label: vec!["B2".to_string()],
                        color: None,
                        size: 10.0,
                        page: Some(DirectAccessPageRaw {
                            rows: 1,
                            content: DirectAccessPageContentRaw::Keys {
                                keys: vec![
                                    DirectAccessKeyRaw {
                                        label: vec!["K2".to_string()],
                                        color: None,
                                        station_id: Some(StationId::from("S2")),
                                        page: None,
                                    },
                                    DirectAccessKeyRaw {
                                        label: vec!["K3".to_string()],
                                        color: None,
                                        station_id: Some(StationId::from("S1")), // Duplicate
                                        page: None,
                                    },
                                    DirectAccessKeyRaw {
                                        label: vec!["K4".to_string()],
                                        color: None,
                                        station_id: None,
                                        page: None,
                                    },
                                ],
                            },
                        }),
                        station_id: None,
                    }),
                ],
            }),
        };

        let profile = Profile::from_raw(raw).expect("Should be valid");
        let expected = HashSet::from([StationId::from("S1"), StationId::from("S2")]);
        assert_eq!(profile.relevant_station_ids, expected);
    }

    #[test]
    fn validate_references() {
        let station_id = StationId::from("S1");
        let other_station_id = StationId::from("S2");
        let valid_stations = HashSet::from([&station_id, &other_station_id]);

        let raw = ProfileRaw {
            id: ProfileId::from("test"),
            profile_type: ProfileTypeRaw::Geo(GeoPageContainerRaw {
                height: None,
                width: None,
                padding: None,
                padding_left: None,
                padding_right: None,
                padding_top: None,
                padding_bottom: None,
                gap: None,
                justify_content: None,
                align_items: None,
                direction: FlexDirection::Row,
                children: vec![GeoNodeRaw::Button(GeoPageButtonRaw {
                    label: vec!["L".to_string()],
                    color: None,
                    size: 10.0,
                    page: Some(DirectAccessPageRaw {
                        rows: 1,
                        content: DirectAccessPageContentRaw::Keys {
                            keys: vec![DirectAccessKeyRaw {
                                label: vec!["K1".to_string()],
                                color: None,
                                station_id: Some(station_id.clone()),
                                page: None,
                            }],
                        },
                    }),
                    station_id: None,
                })],
            }),
        };
        let profile = Profile::from_raw(raw).expect("Should be valid");
        assert!(profile.validate_references(&valid_stations).is_ok());

        let raw_missing = ProfileRaw {
            id: ProfileId::from("test3"),
            profile_type: ProfileTypeRaw::Geo(GeoPageContainerRaw {
                height: None,
                width: None,
                padding: None,
                padding_left: None,
                padding_right: None,
                padding_top: None,
                padding_bottom: None,
                gap: None,
                justify_content: None,
                align_items: None,
                direction: FlexDirection::Row,
                children: vec![GeoNodeRaw::Button(GeoPageButtonRaw {
                    label: vec!["L".to_string()],
                    color: None,
                    size: 10.0,
                    page: Some(DirectAccessPageRaw {
                        rows: 1,
                        content: DirectAccessPageContentRaw::Keys {
                            keys: vec![DirectAccessKeyRaw {
                                label: vec!["K3".to_string()],
                                color: None,
                                station_id: Some(StationId::from("MISSING")),
                                page: None,
                            }],
                        },
                    }),
                    station_id: None,
                })],
            }),
        };
        let profile_missing = Profile::from_raw(raw_missing).expect("Should be valid");
        assert_matches!(
            profile_missing.validate_references(&valid_stations),
            Err(CoverageError::Validation(ValidationError::MissingReference { field, ref_id }))
            if field == "station_id" && ref_id == "MISSING"
        );

        let raw_none = ProfileRaw {
            id: ProfileId::from("test4"),
            profile_type: ProfileTypeRaw::Geo(GeoPageContainerRaw {
                height: None,
                width: None,
                padding: None,
                padding_left: None,
                padding_right: None,
                padding_top: None,
                padding_bottom: None,
                gap: None,
                justify_content: None,
                align_items: None,
                direction: FlexDirection::Row,
                children: vec![GeoNodeRaw::Button(GeoPageButtonRaw {
                    label: vec!["L".to_string()],
                    color: None,
                    size: 10.0,
                    page: Some(DirectAccessPageRaw {
                        rows: 1,
                        content: DirectAccessPageContentRaw::Keys {
                            keys: vec![DirectAccessKeyRaw {
                                label: vec!["K4".to_string()],
                                color: None,
                                station_id: None,
                                page: None,
                            }],
                        },
                    }),
                    station_id: None,
                })],
            }),
        };
        let profile_none = Profile::from_raw(raw_none).expect("Should be valid");
        assert!(profile_none.validate_references(&valid_stations).is_ok());
    }

    #[test]
    fn tab_deserialization_string() {
        let json = r#"{
            "label": "My Tab",
            "page": {
                "rows": 1,
                "keys": []
            }
        }"#;
        let tab: TabRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(tab.label, vec!["My Tab".to_string()]);
    }

    #[test]
    fn tab_deserialization_vec_1_to_3() {
        // 1 element
        let json = r#"{
            "label": ["Line 1"],
            "page": { "rows": 1, "keys": [] }
        }"#;
        let tab: TabRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(tab.label, vec!["Line 1".to_string()]);

        // 3 elements
        let json = r#"{
            "label": ["Line 1", "Line 2", "Line 3"],
            "page": { "rows": 1, "keys": [] }
        }"#;
        let tab: TabRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(
            tab.label,
            vec![
                "Line 1".to_string(),
                "Line 2".to_string(),
                "Line 3".to_string()
            ]
        );
    }

    #[test]
    fn tab_validation_bounds() {
        // Empty vec (0 elements) - invalid
        let json = r#"{
            "label": [],
            "page": { "rows": 1, "keys": [] }
        }"#;
        let tab: TabRaw = serde_json::from_str(json).expect("valid structure");
        assert_matches!(
            tab.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "label"
        );

        // Too many elements (4) - invalid
        let json = r#"{
            "label": ["1", "2", "3", "4"],
            "page": { "rows": 1, "keys": [] }
        }"#;
        let tab: TabRaw = serde_json::from_str(json).expect("valid structure");
        assert_matches!(
            tab.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, .. })) if field == "label"
        );
    }

    #[test]
    fn direct_access_key_deserialization_string() {
        let json = r#"{
            "label": "My Key"
        }"#;
        let key: DirectAccessKeyRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(key.label, vec!["My Key".to_string()]);
    }

    #[test]
    fn direct_access_key_deserialization_vec_0_to_3() {
        // 0 elements
        let json = r#"{
            "label": []
        }"#;
        let key: DirectAccessKeyRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(key.label, Vec::<String>::new());
        assert!(key.validate().is_ok());

        // 3 elements
        let json = r#"{
            "label": ["1", "2", "3"]
        }"#;
        let key: DirectAccessKeyRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(
            key.label,
            vec!["1".to_string(), "2".to_string(), "3".to_string()]
        );
        assert!(key.validate().is_ok());
    }

    #[test]
    fn direct_access_key_validation_bounds() {
        // Too many elements (4) - invalid
        let json = r#"{
            "label": ["1", "2", "3", "4"]
        }"#;
        let key: DirectAccessKeyRaw = serde_json::from_str(json).expect("valid structure");
        assert_matches!(
            key.validate(),
            Err(CoverageError::Validation(ValidationError::InvalidValue { field, .. })) if field == "label"
        );
    }

    #[test]
    fn tab_deserialization_empty_string() {
        let json = r#"{
            "label": "",
            "page": {
                "rows": 1,
                "keys": []
            }
        }"#;
        // Should deserialize to empty vector, which then fails validation because Tab requires 1-3 lines
        let tab: TabRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(tab.label, Vec::<String>::new());
        assert_matches!(
            tab.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "label"
        );

        let json = r#"{
            "label": [""],
            "page": {
                "rows": 1,
                "keys": []
            }
        }"#;
        // Should deserialize to vector with only empty string, which then fails validation because Tab requires 1-3 lines
        let tab: TabRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(tab.label, vec!["".to_string()]);
        assert_matches!(
            tab.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "label"
        );
    }

    #[test]
    fn direct_access_key_deserialization_empty_string() {
        let json = r#"{
            "label": ""
        }"#;
        // Should deserialize to empty vector, which is valid for DA key
        let key: DirectAccessKeyRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(key.label, Vec::<String>::new());
        assert!(key.validate().is_ok());
    }

    #[test]
    fn geo_page_button_deserialization_string() {
        let json = r#"{
            "label": "My Button",
            "size": 10.0
        }"#;
        let button: GeoPageButtonRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(button.label, vec!["My Button".to_string()]);
        assert!(button.validate().is_ok());
    }

    #[test]
    fn geo_page_button_deserialization_vec() {
        let json = r#"{
            "label": ["1", "2", "3"],
            "size": 10.0
        }"#;
        let button: GeoPageButtonRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(
            button.label,
            vec!["1".to_string(), "2".to_string(), "3".to_string()]
        );
        assert!(button.validate().is_ok());
    }

    #[test]
    fn geo_page_button_deserialization_empty_string() {
        let json = r#"{
            "label": "",
            "size": 10.0
        }"#;
        // Should deserialize to empty vector, which fails GeoPageButton validation (requires >= 1 line)
        let button: GeoPageButtonRaw = serde_json::from_str(json).expect("valid json");
        assert_eq!(button.label, Vec::<String>::new());
        assert_matches!(
            button.validate(),
            Err(CoverageError::Validation(ValidationError::Empty { field })) if field == "label"
        );
    }
}
