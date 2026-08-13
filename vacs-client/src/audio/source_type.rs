use std::time::Duration;
use vacs_audio::sources::waveform::{Waveform, WaveformSource, WaveformTone};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SourceType {
    Ring,
    PriorityRing,
    Ringback,
    RingbackOneshot,
    Click,
    CallStart,
    CallEnd,
}

impl SourceType {
    pub(crate) fn into_waveform_source(
        self,
        sample_rate: f32,
        output_channels: usize,
        volume: f32,
    ) -> WaveformSource {
        match self {
            SourceType::Ring => WaveformSource::single(
                WaveformTone::new(497.0, Waveform::Triangle, 0.2),
                Duration::from_secs_f32(1.69),
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::PriorityRing => WaveformSource::new(
                [
                    (
                        WaveformTone::new(769.0, Waveform::Sine, 0.2),
                        Duration::from_millis(120),
                    ),
                    (
                        WaveformTone::new(628.0, Waveform::Triangle, 0.13),
                        Duration::from_millis(80),
                    ),
                    (
                        WaveformTone::new(492.0, Waveform::Triangle, 0.08),
                        Duration::from_millis(90),
                    ),
                ]
                .repeat(4),
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::Ringback => WaveformSource::single(
                WaveformTone::new(425.0, Waveform::Sine, 0.2),
                Duration::from_secs(1),
                Some(Duration::from_secs(4)),
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::RingbackOneshot => WaveformSource::single(
                WaveformTone::new(425.0, Waveform::Sine, 0.2),
                Duration::from_secs(1),
                None,
                Duration::from_millis(10),
                sample_rate,
                2,
                volume,
            ),
            SourceType::Click => WaveformSource::single(
                WaveformTone::new(4000.0, Waveform::Sine, 0.2),
                Duration::from_millis(20),
                None,
                Duration::from_millis(1),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::CallStart => WaveformSource::new(
                vec![
                    (
                        WaveformTone::new(600.0, Waveform::Sine, 0.2),
                        Duration::from_millis(100),
                    ),
                    (
                        WaveformTone::new(900.0, Waveform::Sine, 0.15),
                        Duration::from_millis(100),
                    ),
                ],
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::CallEnd => WaveformSource::new(
                vec![
                    (
                        WaveformTone::new(650.0, Waveform::Sine, 0.2),
                        Duration::from_millis(100),
                    ),
                    (
                        WaveformTone::new(450.0, Waveform::Sine, 0.15),
                        Duration::from_millis(100),
                    ),
                ],
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
        }
    }
}
