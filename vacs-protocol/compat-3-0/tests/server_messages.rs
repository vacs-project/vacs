//! Every message a current server can send must be readable by a released 3.0.0 client.
//!
//! `vacs_protocol_3_0` is `vacs-protocol` at tag `vacs-client-v3.0.0`, the parser that client
//! runs. Each `ServerMessage` sample must deserialize with it to the same variant and values, or to
//! `Unknown` if listed in `MESSAGES_ADDED_SINCE_3_0`. Reasons listed in `REASONS_ADDED_SINCE_3_0`
//! must land in their 3.0.0 `Unknown` fallback, no others may. `every_variant!` is an exhaustive
//! match, so a new variant of any enum reachable from `ServerMessage` does not compile until it
//! has a sample here.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use vacs_protocol::profile::client_page::{
    ClientGroupMode, ClientPageConfig, FrequencyDisplayMode,
};
use vacs_protocol::profile::geo::{
    AlignItems, FlexDirection, GeoNode, GeoPageButton, GeoPageContainer, GeoPageDivider,
    GeoPageDividerOrientation, JustifyContent,
};
use vacs_protocol::profile::tabbed::Tab;
use vacs_protocol::profile::{
    ActiveProfile, CustomButtonColor, DirectAccessKey, DirectAccessPage, DirectAccessPageContent,
    Profile, ProfileId, ProfileType, ProfileView,
};
use vacs_protocol::vatsim::{ClientId, PositionId, StationChange, StationId};
use vacs_protocol::ws::client::CallRejectReason;
use vacs_protocol::ws::server::{
    CallCancelReason, CallCancelled, CallInvitation, CallUpdate, ClientConnected,
    ClientDisconnected, ClientInfo, ClientList, DisconnectReason, Disconnected, LoginFailure,
    LoginFailureReason, ServerMessage, SessionInfo, SessionProfile, StationChanges, StationInfo,
    StationList,
};
use vacs_protocol::ws::shared::{
    CallEnd, CallError, CallErrorReason, CallId, CallSource, CallTarget, Error, ErrorReason,
    UnknownReason, WebrtcAnswer, WebrtcIceCandidate, WebrtcOffer,
};
use vacs_protocol_3_0::ws as released;

const MESSAGES_ADDED_SINCE_3_0: &[&str] = &[];

/// Entries are `Enum::wireTag`, for example `CallErrorReason::someNewReason`.
const REASONS_ADDED_SINCE_3_0: &[&str] = &[];

const MAX_WIRE_IN_REPORT: usize = 2000;

macro_rules! every_variant {
    ($ty:ty { $($pat:pat => $sample:expr),+ $(,)? }) => {{
        let samples: Vec<$ty> = vec![$({
            let sample: $ty = $sample;
            assert!(
                matches!(&sample, $pat),
                "the sample for `{}` does not match it",
                stringify!($pat),
            );
            sample
        }),+];
        for sample in &samples {
            match sample {
                $($pat => {})+
            }
        }
        samples
    }};
}

fn client(n: usize) -> ClientId {
    ClientId::new(format!("{}", 1_000_000 + n))
}

fn position(id: &str) -> PositionId {
    PositionId::from(id)
}

fn station(id: &str) -> StationId {
    StationId::from(id)
}

fn call_id() -> CallId {
    CallId::from(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

fn unknown_reason() -> UnknownReason {
    UnknownReason(json!({"someFutureReason": {"detail": 1}}))
}

fn labels() -> Vec<String> {
    vec!["LOVV".to_string(), "CTR".to_string()]
}

fn call_targets() -> Vec<CallTarget> {
    every_variant!(CallTarget {
        CallTarget::Client(_) => CallTarget::Client(client(1)),
        CallTarget::Position(_) => CallTarget::Position(position("LOWW_TWR")),
        CallTarget::Station(_) => CallTarget::Station(station("LOWW_TWR")),
    })
}

fn target_set() -> HashSet<CallTarget> {
    call_targets().into_iter().collect()
}

fn participants() -> HashMap<ClientId, CallTarget> {
    call_targets()
        .into_iter()
        .enumerate()
        .map(|(i, target)| (client(10 + i), target))
        .collect()
}

fn call_source() -> CallSource {
    CallSource {
        client_id: client(2),
        position_id: Some(position("LOVV_CTR")),
        station_id: Some(station("LOVV_CTR")),
    }
}

fn login_failure_reasons() -> Vec<LoginFailureReason> {
    every_variant!(LoginFailureReason {
        LoginFailureReason::Unauthorized => LoginFailureReason::Unauthorized,
        LoginFailureReason::DuplicateId => LoginFailureReason::DuplicateId,
        LoginFailureReason::InvalidCredentials => LoginFailureReason::InvalidCredentials,
        LoginFailureReason::NoActiveVatsimConnection => LoginFailureReason::NoActiveVatsimConnection,
        LoginFailureReason::AmbiguousVatsimPosition(_) => LoginFailureReason::AmbiguousVatsimPosition(
            vec![position("LOVV_CTR"), position("LOVV_E_CTR")],
        ),
        LoginFailureReason::InvalidVatsimPosition => LoginFailureReason::InvalidVatsimPosition,
        LoginFailureReason::Timeout => LoginFailureReason::Timeout,
        LoginFailureReason::IncompatibleProtocolVersion => {
            LoginFailureReason::IncompatibleProtocolVersion
        },
        LoginFailureReason::Unknown(_) => LoginFailureReason::Unknown(unknown_reason()),
    })
}

fn disconnect_reasons() -> Vec<DisconnectReason> {
    every_variant!(DisconnectReason {
        DisconnectReason::Terminated => DisconnectReason::Terminated,
        DisconnectReason::NoActiveVatsimConnection => DisconnectReason::NoActiveVatsimConnection,
        DisconnectReason::AmbiguousVatsimPosition(_) => DisconnectReason::AmbiguousVatsimPosition(
            vec![position("LOVV_CTR"), position("LOVV_E_CTR")],
        ),
        DisconnectReason::Unknown(_) => DisconnectReason::Unknown(unknown_reason()),
    })
}

fn call_error_reasons() -> Vec<CallErrorReason> {
    every_variant!(CallErrorReason {
        CallErrorReason::TargetsNotFound(_) => CallErrorReason::TargetsNotFound(target_set()),
        CallErrorReason::AlreadyParticipant(_) => {
            CallErrorReason::AlreadyParticipant(CallTarget::Station(station("LOWW_TWR")))
        },
        CallErrorReason::CallNotFound => CallErrorReason::CallNotFound,
        CallErrorReason::CallActive => CallErrorReason::CallActive,
        CallErrorReason::WebrtcFailure(_) => CallErrorReason::WebrtcFailure(client(3)),
        CallErrorReason::AudioFailure(_) => CallErrorReason::AudioFailure(client(3)),
        CallErrorReason::CallFailure => CallErrorReason::CallFailure,
        CallErrorReason::SignalingFailure(_) => CallErrorReason::SignalingFailure(client(3)),
        CallErrorReason::AutoHangup => CallErrorReason::AutoHangup,
        CallErrorReason::PeerConnectionFailed(_) => CallErrorReason::PeerConnectionFailed(client(3)),
        CallErrorReason::NotConferenceLeader(_) => {
            CallErrorReason::NotConferenceLeader(CallTarget::Position(position("LOWW_TWR")))
        },
        CallErrorReason::NotParticipant => CallErrorReason::NotParticipant,
        CallErrorReason::MaxConferenceSizeReached(_) => {
            CallErrorReason::MaxConferenceSizeReached(target_set())
        },
        CallErrorReason::Other => CallErrorReason::Other,
        CallErrorReason::Unknown(_) => CallErrorReason::Unknown(unknown_reason()),
    })
}

fn call_reject_reasons() -> Vec<CallRejectReason> {
    every_variant!(CallRejectReason {
        CallRejectReason::Busy => CallRejectReason::Busy,
        CallRejectReason::Unknown(_) => CallRejectReason::Unknown(unknown_reason()),
    })
}

fn call_cancel_reasons() -> Vec<CallCancelReason> {
    let mut reasons = every_variant!(CallCancelReason {
        CallCancelReason::AnsweredElsewhere(_) => CallCancelReason::AnsweredElsewhere(client(4)),
        CallCancelReason::CallerCancelled => CallCancelReason::CallerCancelled,
        CallCancelReason::Disconnected => CallCancelReason::Disconnected,
        CallCancelReason::Errored(_) => CallCancelReason::Errored(CallErrorReason::AutoHangup),
        CallCancelReason::Rejected(_) => CallCancelReason::Rejected(CallRejectReason::Busy),
        CallCancelReason::Unknown(_) => CallCancelReason::Unknown(unknown_reason()),
    });
    reasons.extend(
        call_error_reasons()
            .into_iter()
            .map(CallCancelReason::Errored),
    );
    reasons.extend(
        call_reject_reasons()
            .into_iter()
            .map(CallCancelReason::Rejected),
    );
    reasons
}

fn error_reasons() -> Vec<ErrorReason> {
    every_variant!(ErrorReason {
        ErrorReason::MalformedMessage => ErrorReason::MalformedMessage,
        ErrorReason::Internal(_) => ErrorReason::Internal("internal".to_string()),
        ErrorReason::PeerConnection => ErrorReason::PeerConnection,
        ErrorReason::UnexpectedMessage(_) => ErrorReason::UnexpectedMessage("callAccept".to_string()),
        ErrorReason::RateLimited { .. } => ErrorReason::RateLimited {
            targets: target_set(),
            retry_after_secs: 30,
        },
        ErrorReason::ClientNotFound => ErrorReason::ClientNotFound,
        ErrorReason::Unknown(_) => ErrorReason::Unknown(unknown_reason()),
    })
}

fn station_changes() -> Vec<StationChange> {
    every_variant!(StationChange {
        StationChange::Online { .. } => StationChange::Online {
            station_id: station("LOWW_TWR"),
            position_id: position("LOWW_TWR"),
        },
        StationChange::Handoff { .. } => StationChange::Handoff {
            station_id: station("LOWW_APP"),
            from_position_id: position("LOVV_CTR"),
            to_position_id: position("LOWW_APP"),
        },
        StationChange::Offline { .. } => StationChange::Offline {
            station_id: station("LOWW_GND"),
        },
    })
}

fn button_colors() -> Vec<CustomButtonColor> {
    every_variant!(CustomButtonColor {
        CustomButtonColor::Clay => CustomButtonColor::Clay,
        CustomButtonColor::Blush => CustomButtonColor::Blush,
        CustomButtonColor::Lilac => CustomButtonColor::Lilac,
        CustomButtonColor::Mint => CustomButtonColor::Mint,
        CustomButtonColor::Lavender => CustomButtonColor::Lavender,
        CustomButtonColor::Taupe => CustomButtonColor::Taupe,
        CustomButtonColor::Cadet => CustomButtonColor::Cadet,
        CustomButtonColor::Steel => CustomButtonColor::Steel,
        CustomButtonColor::Umber => CustomButtonColor::Umber,
        CustomButtonColor::Lagoon => CustomButtonColor::Lagoon,
        CustomButtonColor::Snow => CustomButtonColor::Snow,
        CustomButtonColor::Azure => CustomButtonColor::Azure,
    })
}

fn frequency_display_modes() -> Vec<FrequencyDisplayMode> {
    every_variant!(FrequencyDisplayMode {
        FrequencyDisplayMode::HideAll => FrequencyDisplayMode::HideAll,
        FrequencyDisplayMode::ShowAll => FrequencyDisplayMode::ShowAll,
    })
}

fn client_group_modes() -> Vec<ClientGroupMode> {
    every_variant!(ClientGroupMode {
        ClientGroupMode::None => ClientGroupMode::None,
        ClientGroupMode::Fir => ClientGroupMode::Fir,
        ClientGroupMode::Icao => ClientGroupMode::Icao,
        ClientGroupMode::FirAndIcao => ClientGroupMode::FirAndIcao,
    })
}

fn justify_contents() -> Vec<JustifyContent> {
    every_variant!(JustifyContent {
        JustifyContent::Start => JustifyContent::Start,
        JustifyContent::End => JustifyContent::End,
        JustifyContent::SpaceBetween => JustifyContent::SpaceBetween,
        JustifyContent::SpaceAround => JustifyContent::SpaceAround,
        JustifyContent::SpaceEvenly => JustifyContent::SpaceEvenly,
        JustifyContent::Center => JustifyContent::Center,
    })
}

fn align_items() -> Vec<AlignItems> {
    every_variant!(AlignItems {
        AlignItems::Start => AlignItems::Start,
        AlignItems::End => AlignItems::End,
        AlignItems::Center => AlignItems::Center,
    })
}

fn flex_directions() -> Vec<FlexDirection> {
    every_variant!(FlexDirection {
        FlexDirection::Row => FlexDirection::Row,
        FlexDirection::Col => FlexDirection::Col,
    })
}

fn divider_orientations() -> Vec<GeoPageDividerOrientation> {
    every_variant!(GeoPageDividerOrientation {
        GeoPageDividerOrientation::Horizontal => GeoPageDividerOrientation::Horizontal,
        GeoPageDividerOrientation::Vertical => GeoPageDividerOrientation::Vertical,
    })
}

fn profile_views() -> Vec<ProfileView> {
    every_variant!(ProfileView {
        ProfileView::Page => ProfileView::Page,
        ProfileView::Split => ProfileView::Split,
        ProfileView::Cycle => ProfileView::Cycle,
    })
}

fn cycle<T: Clone>(values: &[T], i: usize) -> T {
    values[i % values.len()].clone()
}

fn client_page_configs() -> Vec<ClientPageConfig> {
    let frequencies = frequency_display_modes();
    let groupings = client_group_modes();
    (0..frequencies.len().max(groupings.len()))
        .map(|i| ClientPageConfig {
            include: vec!["LO*".to_string()],
            exclude: vec!["*_DEL".to_string()],
            priority: vec!["LOVV_*".to_string(), "LOWW_*".to_string()],
            frequencies: cycle(&frequencies, i),
            grouping: cycle(&groupings, i),
        })
        .collect()
}

fn leaf_pages() -> Vec<DirectAccessPage> {
    let keys = DirectAccessPageContent::Keys {
        keys: vec![
            DirectAccessKey {
                label: labels(),
                color: Some(CustomButtonColor::Mint),
                station_id: Some(station("LOWW_TWR")),
                station_ids: None,
                page: None,
            },
            DirectAccessKey {
                label: labels(),
                color: None,
                station_id: None,
                station_ids: Some(vec![station("LOWW_TWR"), station("LOWW_APP")]),
                page: None,
            },
        ],
    };
    let client_page = DirectAccessPageContent::ClientPage {
        client_page: client_page_configs().remove(0),
    };
    vec![
        DirectAccessPage {
            rows: 2,
            content: keys,
        },
        DirectAccessPage {
            rows: 3,
            content: client_page,
        },
    ]
}

fn keys() -> Vec<DirectAccessKey> {
    let colors = button_colors();
    let subpages = leaf_pages();
    (0..colors.len().max(subpages.len()))
        .map(|i| DirectAccessKey {
            label: labels(),
            color: Some(cycle(&colors, i)),
            station_id: Some(station("LOVV_CTR")),
            station_ids: None,
            page: Some(cycle(&subpages, i)),
        })
        .collect()
}

fn page_contents() -> Vec<DirectAccessPageContent> {
    let mut contents = every_variant!(DirectAccessPageContent {
        DirectAccessPageContent::Keys { .. } => DirectAccessPageContent::Keys { keys: keys() },
        DirectAccessPageContent::ClientPage { .. } => DirectAccessPageContent::ClientPage {
            client_page: client_page_configs().remove(0),
        },
    });
    contents.extend(
        client_page_configs()
            .into_iter()
            .map(|client_page| DirectAccessPageContent::ClientPage { client_page }),
    );
    contents
}

fn pages() -> Vec<DirectAccessPage> {
    page_contents()
        .into_iter()
        .map(|content| DirectAccessPage { rows: 6, content })
        .collect()
}

fn tabs() -> Vec<Tab> {
    pages()
        .into_iter()
        .map(|page| Tab {
            label: labels(),
            page,
        })
        .collect()
}

fn geo_button(color: CustomButtonColor, page: DirectAccessPage) -> GeoPageButton {
    GeoPageButton {
        label: labels(),
        size: 4.5,
        color: Some(color),
        page: Some(page),
        station_id: Some(station("LOVV_CTR")),
        station_ids: Some(vec![station("LOVV_CTR"), station("LOWW_APP")]),
    }
}

fn geo_divider(orientation: GeoPageDividerOrientation) -> GeoPageDivider {
    GeoPageDivider {
        orientation,
        thickness: 2.0,
        color: "#ffffff".to_string(),
        oversize: Some(0.5),
    }
}

fn geo_container(
    justify_content: JustifyContent,
    align_items: AlignItems,
    direction: FlexDirection,
    children: Vec<GeoNode>,
) -> GeoPageContainer {
    GeoPageContainer {
        height: Some("100%".to_string()),
        width: Some("5rem".to_string()),
        padding: Some(1.0),
        padding_left: Some(0.25),
        padding_right: Some(0.5),
        padding_top: Some(0.75),
        padding_bottom: Some(1.25),
        gap: Some(0.5),
        justify_content: Some(justify_content),
        align_items: Some(align_items),
        direction,
        children,
    }
}

fn geo_nodes() -> Vec<GeoNode> {
    let mut nodes = every_variant!(GeoNode {
        GeoNode::Container(_) => GeoNode::Container(geo_container(
            JustifyContent::Center,
            AlignItems::Center,
            FlexDirection::Col,
            vec![],
        )),
        GeoNode::Button(_) => {
            GeoNode::Button(geo_button(CustomButtonColor::Clay, leaf_pages().remove(0)))
        },
        GeoNode::Divider(_) => {
            GeoNode::Divider(geo_divider(GeoPageDividerOrientation::Horizontal))
        },
    });

    let justify = justify_contents();
    let align = align_items();
    let directions = flex_directions();
    nodes.extend(
        (0..justify.len().max(align.len()).max(directions.len())).map(|i| {
            GeoNode::Container(geo_container(
                cycle(&justify, i),
                cycle(&align, i),
                cycle(&directions, i),
                vec![GeoNode::Divider(geo_divider(
                    GeoPageDividerOrientation::Vertical,
                ))],
            ))
        }),
    );

    let colors = button_colors();
    let pages = pages();
    nodes.extend(
        (0..colors.len().max(pages.len()))
            .map(|i| GeoNode::Button(geo_button(cycle(&colors, i), cycle(&pages, i)))),
    );

    nodes.extend(
        divider_orientations()
            .into_iter()
            .map(|orientation| GeoNode::Divider(geo_divider(orientation))),
    );
    nodes
}

fn profile_types() -> Vec<ProfileType> {
    every_variant!(ProfileType {
        ProfileType::Geo(_) => ProfileType::Geo(Box::new(geo_container(
            JustifyContent::SpaceBetween,
            AlignItems::Start,
            FlexDirection::Row,
            geo_nodes(),
        ))),
        ProfileType::Tabbed(_) => ProfileType::Tabbed(tabs()),
    })
}

fn profiles() -> Vec<Profile> {
    let types = profile_types();
    profile_views()
        .into_iter()
        .flat_map(|view| {
            types.iter().cloned().map(move |profile_type| Profile {
                id: ProfileId::from("LOVV"),
                view,
                profile_type,
            })
        })
        .collect()
}

fn active_profiles() -> Vec<ActiveProfile<Profile>> {
    let mut active = every_variant!(ActiveProfile<Profile> {
        ActiveProfile::Specific(_) => ActiveProfile::Specific(profiles().remove(0)),
        ActiveProfile::Custom => ActiveProfile::Custom,
        ActiveProfile::None => ActiveProfile::None,
    });
    active.extend(profiles().into_iter().map(ActiveProfile::Specific));
    active
}

fn session_profiles() -> Vec<SessionProfile> {
    let mut session = every_variant!(SessionProfile {
        SessionProfile::Unchanged => SessionProfile::Unchanged,
        SessionProfile::Changed(_) => SessionProfile::Changed(ActiveProfile::Custom),
    });
    session.extend(active_profiles().into_iter().map(SessionProfile::Changed));
    session
}

fn client_info(n: usize) -> ClientInfo {
    ClientInfo {
        id: client(n),
        display_name: "LOVV_CTR".to_string(),
        frequency: "134.440".to_string(),
        position_id: Some(position("LOVV_CTR")),
    }
}

fn session_info(profile: SessionProfile) -> SessionInfo {
    SessionInfo {
        client: client_info(5),
        profile,
        default_call_sources: vec![station("LOVV_CTR"), station("LOWW_APP")],
        max_conf_size: Some(6),
    }
}

fn call_cancelled(reason: CallCancelReason) -> CallCancelled {
    CallCancelled {
        call_id: call_id(),
        targets: target_set(),
        reason,
    }
}

fn call_error(reason: CallErrorReason) -> CallError {
    CallError {
        call_id: call_id(),
        reason,
        message: Some("details".to_string()),
    }
}

fn error(reason: ErrorReason) -> Error {
    Error {
        reason,
        client_id: Some(client(6)),
        call_id: Some(call_id()),
    }
}

fn server_messages() -> Vec<ServerMessage> {
    let mut messages = every_variant!(ServerMessage {
        ServerMessage::LoginFailure(_) => LoginFailure {
            reason: LoginFailureReason::Unauthorized,
        }
        .into(),
        ServerMessage::CallInvitation(_) => CallInvitation {
            call_id: call_id(),
            source: call_source(),
            target: CallTarget::Station(station("LOWW_TWR")),
            invited_targets: target_set(),
            joined_participants: participants(),
            conference_leader: Some(client(2)),
            prio: true,
        }
        .into(),
        ServerMessage::CallEnd(_) => CallEnd {
            call_id: call_id(),
            ending_client_id: client(2),
        }
        .into(),
        ServerMessage::CallUpdate(_) => ServerMessage::CallUpdate(CallUpdate {
            call_id: call_id(),
            invited_targets: target_set(),
            joined_participants: participants(),
            conference_leader: Some(client(2)),
        }),
        ServerMessage::CallCancelled(_) => call_cancelled(CallCancelReason::CallerCancelled).into(),
        ServerMessage::CallError(_) => call_error(CallErrorReason::CallFailure).into(),
        ServerMessage::WebrtcOffer(_) => WebrtcOffer {
            call_id: call_id(),
            from_client_id: client(2),
            to_client_id: client(3),
            sdp: "v=0".to_string(),
        }
        .into(),
        ServerMessage::WebrtcAnswer(_) => WebrtcAnswer {
            call_id: call_id(),
            from_client_id: client(3),
            to_client_id: client(2),
            sdp: "v=0".to_string(),
        }
        .into(),
        ServerMessage::WebrtcIceCandidate(_) => WebrtcIceCandidate {
            call_id: call_id(),
            from_client_id: client(2),
            to_client_id: client(3),
            candidate: "candidate:1 1 udp 2130706431 192.0.2.1 50000 typ host".to_string(),
        }
        .into(),
        ServerMessage::ClientInfo(_) => client_info(5).into(),
        ServerMessage::SessionInfo(_) => session_info(SessionProfile::Unchanged).into(),
        ServerMessage::ClientConnected(_) => ClientConnected {
            client: client_info(7),
        }
        .into(),
        ServerMessage::ClientDisconnected(_) => ClientDisconnected {
            client_id: client(7),
        }
        .into(),
        ServerMessage::ClientList(_) => ClientList {
            clients: vec![client_info(7), client_info(8)],
        }
        .into(),
        ServerMessage::StationList(_) => StationList {
            stations: vec![
                StationInfo {
                    id: station("LOVV_CTR"),
                    own: true,
                },
                StationInfo {
                    id: station("LOWW_TWR"),
                    own: false,
                },
            ],
        }
        .into(),
        ServerMessage::StationChanges(_) => StationChanges {
            changes: station_changes(),
        }
        .into(),
        ServerMessage::Disconnected(_) => Disconnected {
            reason: DisconnectReason::Terminated,
        }
        .into(),
        ServerMessage::Error(_) => error(ErrorReason::Internal("internal".to_string())).into(),
        ServerMessage::Unknown => ServerMessage::Unknown,
    });

    messages.extend(
        login_failure_reasons()
            .into_iter()
            .map(|reason| LoginFailure { reason }.into()),
    );
    messages.extend(
        disconnect_reasons()
            .into_iter()
            .map(|reason| Disconnected { reason }.into()),
    );
    messages.extend(
        call_cancel_reasons()
            .into_iter()
            .map(|reason| call_cancelled(reason).into()),
    );
    messages.extend(
        call_error_reasons()
            .into_iter()
            .map(|reason| call_error(reason).into()),
    );
    messages.extend(
        error_reasons()
            .into_iter()
            .map(|reason| error(reason).into()),
    );
    messages.extend(
        session_profiles()
            .into_iter()
            .map(|profile| session_info(profile).into()),
    );
    messages
}

/// Whether every value the 3.0.0 client reads back is the one the server sent. Arrays compare
/// as unordered because `HashSet`s serialize in iteration order.
fn is_projection(read: &Value, sent: &Value) -> bool {
    match (read, sent) {
        (Value::Object(read), Value::Object(sent)) => read
            .iter()
            .all(|(key, value)| sent.get(key).is_some_and(|sent| is_projection(value, sent))),
        (Value::Array(read), Value::Array(sent)) => {
            read.len() == sent.len()
                && read
                    .iter()
                    .all(|value| sent.iter().any(|sent| is_projection(value, sent)))
        }
        _ => read == sent,
    }
}

fn wire_tag(reason: &impl Serialize) -> String {
    match serde_json::to_value(reason).expect("reasons serialize") {
        Value::String(tag) => tag,
        Value::Object(map) if map.len() == 1 => map.keys().next().cloned().unwrap_or_default(),
        other => other.to_string(),
    }
}

fn check_landing(
    enum_name: &str,
    reason: &impl Serialize,
    sent_unknown: bool,
    read_unknown: bool,
) -> Result<(), String> {
    let key = format!("{enum_name}::{}", wire_tag(reason));
    let added = sent_unknown || REASONS_ADDED_SINCE_3_0.contains(&key.as_str());
    match (added, read_unknown) {
        (false, true) => Err(format!(
            "{key} lands in the 3.0.0 `Unknown` fallback: its wire form changed, or it is new and \
             belongs in REASONS_ADDED_SINCE_3_0"
        )),
        (true, false) => Err(format!(
            "{key} is listed as added since 3.0.0, but 3.0.0 parses it as a known reason"
        )),
        _ => Ok(()),
    }
}

fn check_reasons(
    sent: &ServerMessage,
    read: &released::server::ServerMessage,
) -> Result<(), String> {
    use released::client::CallRejectReason as ReadReject;
    use released::server::{
        CallCancelReason as ReadCancel, DisconnectReason as ReadDisconnect,
        LoginFailureReason as ReadLogin, ServerMessage as Read,
    };
    use released::shared::{CallErrorReason as ReadCallError, ErrorReason as ReadError};

    fn call_error_landing(sent: &CallErrorReason, read: &ReadCallError) -> Result<(), String> {
        check_landing(
            "CallErrorReason",
            sent,
            matches!(sent, CallErrorReason::Unknown(_)),
            matches!(read, ReadCallError::Unknown(_)),
        )
    }

    match (sent, read) {
        (ServerMessage::LoginFailure(sent), Read::LoginFailure(read)) => check_landing(
            "LoginFailureReason",
            &sent.reason,
            matches!(sent.reason, LoginFailureReason::Unknown(_)),
            matches!(read.reason, ReadLogin::Unknown(_)),
        ),
        (ServerMessage::Disconnected(sent), Read::Disconnected(read)) => check_landing(
            "DisconnectReason",
            &sent.reason,
            matches!(sent.reason, DisconnectReason::Unknown(_)),
            matches!(read.reason, ReadDisconnect::Unknown(_)),
        ),
        (ServerMessage::CallError(sent), Read::CallError(read)) => {
            call_error_landing(&sent.reason, &read.reason)
        }
        (ServerMessage::Error(sent), Read::Error(read)) => check_landing(
            "ErrorReason",
            &sent.reason,
            matches!(sent.reason, ErrorReason::Unknown(_)),
            matches!(read.reason, ReadError::Unknown(_)),
        ),
        (ServerMessage::CallCancelled(sent), Read::CallCancelled(read)) => {
            check_landing(
                "CallCancelReason",
                &sent.reason,
                matches!(sent.reason, CallCancelReason::Unknown(_)),
                matches!(read.reason, ReadCancel::Unknown(_)),
            )?;
            match (&sent.reason, &read.reason) {
                (CallCancelReason::Errored(sent), ReadCancel::Errored(read)) => {
                    call_error_landing(sent, read)
                }
                (CallCancelReason::Rejected(sent), ReadCancel::Rejected(read)) => check_landing(
                    "CallRejectReason",
                    sent,
                    matches!(sent, CallRejectReason::Unknown(_)),
                    matches!(read, ReadReject::Unknown(_)),
                ),
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

fn check(message: &ServerMessage) -> Result<(), String> {
    let wire = message.serialize().expect("server messages serialize");
    let fail = |why: String| {
        let shown: String = wire.chars().take(MAX_WIRE_IN_REPORT).collect();
        format!("{}: {why}\n  wire: {shown}", message.variant())
    };

    let read = released::server::ServerMessage::deserialize(&wire)
        .map_err(|err| fail(format!("3.0.0 fails to deserialize it: {err}")))?;

    if MESSAGES_ADDED_SINCE_3_0.contains(&message.variant()) {
        return match read {
            released::server::ServerMessage::Unknown => Ok(()),
            other => Err(fail(format!(
                "listed as added since 3.0.0, but 3.0.0 parses it as {}",
                other.variant()
            ))),
        };
    }
    if read.variant() != message.variant() {
        return Err(fail(format!("3.0.0 parses it as {}", read.variant())));
    }

    let sent: Value = serde_json::from_str(&wire).expect("wire is JSON");
    let read_back = serde_json::to_value(&read).expect("3.0.0 messages serialize");
    if !is_projection(&read_back, &sent) {
        return Err(fail(format!(
            "3.0.0 reads back different values: {read_back}"
        )));
    }

    check_reasons(message, &read).map_err(fail)
}

#[test]
fn every_server_message_is_readable_by_a_3_0_client() {
    let messages = server_messages();
    let failures: Vec<String> = messages.iter().filter_map(|m| check(m).err()).collect();
    assert!(
        failures.is_empty(),
        "{} of {} server messages break a 3.0.0 client:\n\n{}",
        failures.len(),
        messages.len(),
        failures.join("\n\n"),
    );
}
