use serde::{Deserialize, Serialize};
use vacs_audio::device::DeviceType;

pub(crate) mod commands;
pub(crate) mod manager;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioHosts {
    selected: String,
    all: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevices {
    preferred: Option<String>,
    picked: Option<String>,
    default: String,
    all: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeType {
    Input,
    Output,
    Click,
    Chime,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioVolumes {
    input: f32,
    output: f32,
    click: f32,
    chime: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientAudioDeviceType {
    Input,
    Output,
    Speaker,
}

impl From<ClientAudioDeviceType> for DeviceType {
    fn from(value: ClientAudioDeviceType) -> Self {
        match value {
            ClientAudioDeviceType::Input => DeviceType::Input,
            ClientAudioDeviceType::Output => DeviceType::Output,
            ClientAudioDeviceType::Speaker => DeviceType::Output,
        }
    }
}
