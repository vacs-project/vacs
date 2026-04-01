use crate::profile::{CustomButtonColor, DirectAccessPage};
use crate::vatsim::StationId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoPageContainer {
    /// The height of the container.
    ///
    /// Must either be defined as a percentage or a rem value (e.g. "100%", "5rem").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<String>,

    /// The width of the container.
    ///
    /// Must either be defined as a percentage or a rem value (e.g. "100%", "5rem").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,

    /// The padding for all sides (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,

    /// The left padding (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_left: Option<f64>,

    /// The right padding (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_right: Option<f64>,

    /// The top padding (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<f64>,

    /// The bottom padding (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<f64>,

    /// The gap between children (in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,

    /// The justification of the content along the main axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<JustifyContent>,

    /// The alignment of items along the cross-axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_items: Option<AlignItems>,

    /// The direction of the flex container.
    pub direction: FlexDirection,

    /// The children of this container.
    pub children: Vec<GeoNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JustifyContent {
    /// The items are packed flush to each other toward the start edge of the alignment container in the main axis.
    Start,
    /// The items are packed flush to each other toward the end edge of the alignment container in the main axis.
    End,
    /// The items are evenly distributed within the alignment container along the main axis.
    /// The spacing between each pair of adjacent items is the same.
    /// The first item is flush with the main-start edge, and the last item is flush with the main-end edge.
    SpaceBetween,
    /// The items are evenly distributed within the alignment container along the main axis.
    /// The spacing between each pair of adjacent items is the same.
    /// The empty space before the first and after the last item equals half of the space between each pair of adjacent items.
    /// If there is only one item, it will be centered.
    SpaceAround,
    /// The items are evenly distributed within the alignment container along the main axis.
    /// The spacing between each pair of adjacent items, the main-start edge and the first item, and the main-end edge and the last item, are all exactly the same.
    SpaceEvenly,
    /// The items are packed flush to each other toward the center of the alignment container along the main axis.
    Center,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlignItems {
    /// The items are packed flush to each other toward the start edge of the alignment container in the appropriate axis.
    Start,
    /// The items are packed flush to each other toward the end edge of the alignment container in the appropriate axis.
    End,
    /// The flex items' margin boxes are centered within the line on the cross-axis.
    /// If the cross-size of an item is larger than the flex container, it will overflow equally in both directions.
    Center,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlexDirection {
    /// The flex container's main axis is the same as the text direction.
    Row,
    /// The flex container's main axis is the same as the block axis.
    Col,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GeoNode {
    /// A recursive container for grouping other nodes.
    Container(GeoPageContainer),
    /// A clickable button with a label and action.
    Button(GeoPageButton),
    /// A visual divider between elements.
    Divider(GeoPageDivider),
}

/// A button on a GEO profile page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoPageButton {
    /// The text label displayed on the button.
    ///
    /// Will always contain between 0 and 3 lines of text.
    #[serde(deserialize_with = "crate::profile::string_or_vec")]
    pub label: Vec<String>,

    /// The size of the button (> 0, in rem).
    pub size: f64,

    /// The background color of the button, which may be overridden dynamically by state changes (e.g., incoming call). Defaults to gray if unspecified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<CustomButtonColor>,

    /// The optional direct access page that opens when this button is clicked.
    ///
    /// If [`GeoPageButton::page`] and [`GeoPageButton::station_id`] are `None`, the button will be displayed and clickable on the UI, but will otherwise be non-functional.
    /// This field is mutually exclusive with [`GeoPageButton::station_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<DirectAccessPage>,

    /// The optional station ID associated with this button.
    ///
    /// If [`GeoPageButton::page`] and [`GeoPageButton::station_id`] are `None`, the button will be displayed and clickable on the UI, but will otherwise be non-functional.
    /// This field is mutually exclusive with [`GeoPageButton::page`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_id: Option<StationId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoPageDivider {
    /// The orientation of the divider.
    pub orientation: GeoPageDividerOrientation,

    /// The thickness of the divider (> 0, in px).
    pub thickness: f64,

    /// The color of the divider (CSS color).
    pub color: String,

    /// The oversize of the divider (> 0, in rem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oversize: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GeoPageDividerOrientation {
    /// The divider runs horizontally.
    Horizontal,
    /// The divider runs vertically.
    Vertical,
}
