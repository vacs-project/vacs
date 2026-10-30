use crate::audio::source_type::SourceType;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use vacs_audio::device::DeviceType;

pub(crate) mod commands;
pub(crate) mod manager;
pub(crate) mod source_type;

pub const DEFAULT_RING_SOUND_LOOP_PAUSE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema), schemars(default))]
pub struct AudioConfig {
    /// Audio backend to use, for example `WASAPI`, `ALSA` or `CoreAudio`. Unset uses the
    /// platform default.
    pub host_name: Option<String>, // Name of audio backend host, None means default host
    /// Name of the microphone. Unset uses the system default.
    pub input_device_name: Option<String>, // None means default device
    /// Stable identifier of the microphone, used to find it again when its name changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_device_id: Option<String>, // Stable device ID for reliable matching, None means default device
    /// Name of the headset output for call audio. Unset uses the system default.
    pub output_device_name: Option<String>, // None means default device
    /// Stable identifier of the headset output, used to find it again when its name changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_device_id: Option<String>, // Stable device ID for reliable matching, None means default device
    /// Whether ring, chime and click sounds play on the speaker device instead of the headset.
    pub speaker_enabled: bool,
    /// Name of the speaker device. Unset uses the system default.
    pub speaker_device_name: Option<String>, // None means default device
    /// Stable identifier of the speaker device, used to find it again when its name changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_device_id: Option<String>, // Stable device ID for reliable matching, None means default device
    /// Microphone volume, from `0.0` to `1.0`.
    pub input_device_volume: f32,
    /// Gain applied to the microphone at full volume.
    pub input_device_volume_amp: f32,
    /// Call audio volume, from `0.0` to `1.0`.
    pub output_device_volume: f32,
    /// Gain applied to call audio at full volume.
    pub output_device_volume_amp: f32,
    /// Volume of the button click sound, from `0.0` to `1.0`.
    pub click_volume: f32,
    /// Volume of the ring and notification sounds, from `0.0` to `1.0`.
    pub chime_volume: f32,
    /// WAV file played for incoming calls instead of the built-in ring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_sound: Option<PathBuf>, // Custom WAV file, None means the built-in chime
    /// WAV file played for incoming priority calls instead of the built-in ring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_ring_sound: Option<PathBuf>, // Custom WAV file, None means the built-in chime
    /// Whether `ring_sound` repeats until the call is answered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ring_sound_loop: bool,
    /// Whether `priority_ring_sound` repeats until the call is answered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub priority_ring_sound_loop: bool,
    /// Pause between repetitions of a looping ring sound, in seconds. Defaults to 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_sound_loop_pause_seconds: Option<f32>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            host_name: None,
            input_device_name: None,
            input_device_id: None,
            output_device_name: None,
            output_device_id: None,
            speaker_enabled: false,
            speaker_device_name: None,
            speaker_device_id: None,
            input_device_volume: 0.5,
            input_device_volume_amp: 4.0,
            output_device_volume: 0.5,
            output_device_volume_amp: 2.0,
            click_volume: 0.5,
            chime_volume: 0.5,
            ring_sound: None,
            priority_ring_sound: None,
            ring_sound_loop: false,
            priority_ring_sound_loop: false,
            ring_sound_loop_pause_seconds: None,
        }
    }
}

impl AudioConfig {
    pub fn ring_sound(&self, ring_type: RingSoundType) -> Option<&Path> {
        match ring_type {
            RingSoundType::Ring => self.ring_sound.as_deref(),
            RingSoundType::PriorityRing => self.priority_ring_sound.as_deref(),
        }
    }

    pub fn set_ring_sound(&mut self, ring_type: RingSoundType, path: Option<PathBuf>) {
        match ring_type {
            RingSoundType::Ring => self.ring_sound = path,
            RingSoundType::PriorityRing => self.priority_ring_sound = path,
        }
    }

    pub fn ring_sound_loop(&self, ring_type: RingSoundType) -> bool {
        match ring_type {
            RingSoundType::Ring => self.ring_sound_loop,
            RingSoundType::PriorityRing => self.priority_ring_sound_loop,
        }
    }

    pub fn set_ring_sound_loop(&mut self, ring_type: RingSoundType, looped: bool) {
        match ring_type {
            RingSoundType::Ring => self.ring_sound_loop = looped,
            RingSoundType::PriorityRing => self.priority_ring_sound_loop = looped,
        }
    }

    /// The pause between repetitions of that ring's custom sound, or `None` when it plays once.
    pub fn ring_sound_loop_pause(&self, ring_type: RingSoundType) -> Option<Duration> {
        self.ring_sound_loop(ring_type).then(|| {
            self.ring_sound_loop_pause_seconds
                .filter(|seconds| seconds.is_finite())
                .and_then(|seconds| Duration::try_from_secs_f32(seconds.max(0.0)).ok())
                .unwrap_or(DEFAULT_RING_SOUND_LOOP_PAUSE)
        })
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PersistedAudioConfig {
    pub audio: AudioConfig,
}

impl From<AudioConfig> for PersistedAudioConfig {
    fn from(audio: AudioConfig) -> Self {
        Self { audio }
    }
}

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

/// Whether starting the input level meter registered the caller as one of its users, which
/// it must then release again. Not the case while a call holds the microphone.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputLevelMeter {
    pub user_added: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RingSoundType {
    Ring,
    PriorityRing,
}

impl RingSoundType {
    /// The source that plays the ring once, for previews, even while the ring itself loops.
    pub fn oneshot_source_type(self) -> SourceType {
        match self {
            RingSoundType::Ring => SourceType::RingOneshot,
            RingSoundType::PriorityRing => SourceType::PriorityRingOneshot,
        }
    }
}

impl From<RingSoundType> for SourceType {
    fn from(value: RingSoundType) -> Self {
        match value {
            RingSoundType::Ring => SourceType::Ring,
            RingSoundType::PriorityRing => SourceType::PriorityRing,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RingSound {
    path: String,
    /// False when the configured file could not be loaded, so the built-in chime plays instead.
    available: bool,
    /// Whether the sound repeats until every incoming call has been answered or has ended.
    #[serde(rename = "loop")]
    looped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RingSounds {
    #[serde(skip_serializing_if = "Option::is_none")]
    ring: Option<RingSound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority_ring: Option<RingSound>,
}

impl RingSounds {
    pub fn new(config: &AudioConfig, is_loaded: impl Fn(RingSoundType) -> bool) -> Self {
        let entry = |ring_type: RingSoundType| {
            config.ring_sound(ring_type).map(|path| RingSound {
                path: path.to_string_lossy().into_owned(),
                available: is_loaded(ring_type),
                looped: config.ring_sound_loop(ring_type),
            })
        };
        Self {
            ring: entry(RingSoundType::Ring),
            priority_ring: entry(RingSoundType::PriorityRing),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackDeviceType {
    Output,
    Speaker,
}

impl std::fmt::Display for PlaybackDeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaybackDeviceType::Output => write!(f, "output"),
            PlaybackDeviceType::Speaker => write!(f, "speaker"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_config_without_ring_sounds_deserializes_to_defaults() {
        let toml = r#"
            speaker_enabled = false
            input_device_volume = 0.5
            input_device_volume_amp = 4.0
            output_device_volume = 0.5
            output_device_volume_amp = 2.0
            click_volume = 0.5
            chime_volume = 0.5
        "#;
        let config: AudioConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.ring_sound, None);
        assert_eq!(config.priority_ring_sound, None);
    }

    #[test]
    fn ring_sounds_round_trip_through_toml() {
        let mut config = AudioConfig::default();
        config.set_ring_sound(
            RingSoundType::PriorityRing,
            Some(PathBuf::from("/tmp/prio.wav")),
        );

        let serialized = toml::to_string(&config).unwrap();
        assert!(
            !serialized
                .lines()
                .any(|line| line.starts_with("ring_sound"))
        );
        assert!(serialized.contains("priority_ring_sound = \"/tmp/prio.wav\""));

        let parsed: AudioConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.ring_sound(RingSoundType::Ring), None);
        assert_eq!(
            parsed.ring_sound(RingSoundType::PriorityRing),
            Some(Path::new("/tmp/prio.wav"))
        );
    }

    #[test]
    fn windows_path_round_trips_through_toml() {
        let mut config = AudioConfig::default();
        let path = PathBuf::from(r"C:\Users\me\Sounds\ring.wav");
        config.set_ring_sound(RingSoundType::Ring, Some(path.clone()));

        let parsed: AudioConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(parsed.ring_sound(RingSoundType::Ring), Some(path.as_path()));
    }

    #[test]
    fn ring_sound_type_deserializes_from_the_frontend_names() {
        assert_eq!(
            serde_json::from_value::<RingSoundType>(serde_json::json!("ring")).unwrap(),
            RingSoundType::Ring
        );
        assert_eq!(
            serde_json::from_value::<RingSoundType>(serde_json::json!("priorityRing")).unwrap(),
            RingSoundType::PriorityRing
        );
    }

    #[test]
    fn persisted_config_round_trips_the_ring_sound_through_the_config_layer() {
        let mut config = AudioConfig::default();
        config.set_ring_sound(RingSoundType::Ring, Some(PathBuf::from("/tmp/ring.wav")));
        let written = toml::to_string_pretty(&PersistedAudioConfig::from(config)).unwrap();

        let read: AudioConfig = config::Config::builder()
            .add_source(config::File::from_str(&written, config::FileFormat::Toml))
            .build()
            .unwrap()
            .get("audio")
            .unwrap();
        assert_eq!(
            read.ring_sound(RingSoundType::Ring),
            Some(Path::new("/tmp/ring.wav"))
        );
        assert_eq!(read.ring_sound(RingSoundType::PriorityRing), None);
    }

    #[test]
    fn ring_sounds_payload_omits_unset_entries_and_flags_unloaded_files() {
        let config = AudioConfig::default();
        let json = serde_json::to_value(RingSounds::new(&config, |_| true)).unwrap();
        assert_eq!(json, serde_json::json!({}));

        let mut config = AudioConfig::default();
        config.set_ring_sound(RingSoundType::Ring, Some(PathBuf::from("/tmp/ring.wav")));
        let json = serde_json::to_value(RingSounds::new(&config, |_| false)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"ring": {"path": "/tmp/ring.wav", "available": false, "loop": false}})
        );
    }

    #[test]
    fn ring_sounds_payload_reports_the_loop_flag_per_ring() {
        let mut config = AudioConfig::default();
        config.set_ring_sound(RingSoundType::Ring, Some(PathBuf::from("/tmp/ring.wav")));
        config.set_ring_sound(
            RingSoundType::PriorityRing,
            Some(PathBuf::from("/tmp/prio.wav")),
        );
        config.set_ring_sound_loop(RingSoundType::PriorityRing, true);

        let json = serde_json::to_value(RingSounds::new(&config, |_| true)).unwrap();
        assert_eq!(json["ring"]["loop"], false);
        assert_eq!(json["priorityRing"]["loop"], true);
    }

    #[test]
    fn ring_sound_loop_round_trips_through_toml_and_is_omitted_when_off() {
        let serialized = toml::to_string(&AudioConfig::default()).unwrap();
        assert!(!serialized.contains("loop"));

        let mut config = AudioConfig::default();
        config.set_ring_sound_loop(RingSoundType::Ring, true);
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("ring_sound_loop = true"));

        let parsed: AudioConfig = toml::from_str(&serialized).unwrap();
        assert!(parsed.ring_sound_loop(RingSoundType::Ring));
        assert!(!parsed.ring_sound_loop(RingSoundType::PriorityRing));
    }

    #[test]
    fn ring_sound_loop_pause_defaults_and_rejects_invalid_values() {
        let mut config = AudioConfig::default();
        assert_eq!(config.ring_sound_loop_pause(RingSoundType::Ring), None);

        config.set_ring_sound_loop(RingSoundType::Ring, true);
        let pause = |config: &AudioConfig| config.ring_sound_loop_pause(RingSoundType::Ring);
        assert_eq!(pause(&config), Some(DEFAULT_RING_SOUND_LOOP_PAUSE));
        assert_eq!(
            config.ring_sound_loop_pause(RingSoundType::PriorityRing),
            None
        );

        config.ring_sound_loop_pause_seconds = Some(1.5);
        assert_eq!(pause(&config), Some(Duration::from_millis(1500)));

        config.ring_sound_loop_pause_seconds = Some(-2.0);
        assert_eq!(pause(&config), Some(Duration::ZERO));

        for invalid in [f32::NAN, f32::INFINITY, 1e30] {
            config.ring_sound_loop_pause_seconds = Some(invalid);
            assert_eq!(
                pause(&config),
                Some(DEFAULT_RING_SOUND_LOOP_PAUSE),
                "{invalid}"
            );
        }
    }
}
