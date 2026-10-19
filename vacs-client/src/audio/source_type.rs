use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use vacs_audio::error::AudioError;
use vacs_audio::sources::AudioSource;
use vacs_audio::sources::wav::{WavClip, WavSource};
use vacs_audio::sources::waveform::{Waveform, WaveformSegment, WaveformSource, WaveformTone};

/// Longest custom ring sound accepted. The clip is held in memory fully decoded and plays from
/// the start for every incoming call, so anything longer than this is a mistake rather than a
/// ringtone.
pub const MAX_RING_SOUND_DURATION: Duration = Duration::from_secs(30);
/// Shorter clips are almost certainly truncated files and would ring silently.
pub const MIN_RING_SOUND_DURATION: Duration = Duration::from_millis(100);
/// Peak amplitude of the built-in chimes, so a custom file sits at the same level on the chime
/// volume slider whatever it was mastered at.
const RING_SOUND_PEAK: f32 = 0.2;

#[derive(Debug, Error)]
pub enum RingSoundError {
    #[error("{path}: the file is {duration:.1} s long, the maximum is {} s", MAX_RING_SOUND_DURATION.as_secs())]
    TooLong { path: String, duration: f32 },
    #[error("{path}: the file is shorter than {} ms", MIN_RING_SOUND_DURATION.as_millis())]
    TooShort { path: String },
    #[error("{path}: the file contains only silence")]
    Silent { path: String },
    #[error("{path}: {message}")]
    Decode { path: String, message: String },
    #[error("decoding task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Decodes a custom ring sound and rejects files that would not work as one.
pub(crate) fn load_ring_clip(path: &Path) -> Result<WavClip, RingSoundError> {
    let display = || path.display().to_string();
    let decode = |err: anyhow::Error| RingSoundError::Decode {
        path: display(),
        message: err.to_string(),
    };

    let duration = WavClip::probe_duration(path).map_err(decode)?;
    if duration > MAX_RING_SOUND_DURATION {
        return Err(RingSoundError::TooLong {
            path: display(),
            duration: duration.as_secs_f32(),
        });
    }
    if duration < MIN_RING_SOUND_DURATION {
        return Err(RingSoundError::TooShort { path: display() });
    }

    let clip = WavClip::load(path).map_err(decode)?;
    if clip.peak() == 0.0 {
        return Err(RingSoundError::Silent { path: display() });
    }

    Ok(clip.normalized_to_peak(RING_SOUND_PEAK))
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SourceType {
    Ring,
    PriorityRing,
    RingOneshot,
    PriorityRingOneshot,
    Ringback,
    RingbackOneshot,
    Click,
    CallStart,
    CallEnd,
    ParticipantJoined,
    ParticipantLeft,
}

impl SourceType {
    /// Builds a ring source: the custom `clip` when one is configured, repeating after
    /// `loop_pause` when that is set, otherwise the built-in chime, which always plays once.
    pub(crate) fn into_ring_source(
        self,
        clip: Option<&WavClip>,
        loop_pause: Option<Duration>,
        sample_rate: u32,
        output_channels: usize,
        volume: f32,
    ) -> Result<Box<dyn AudioSource>, AudioError> {
        debug_assert!(matches!(
            self,
            SourceType::Ring
                | SourceType::PriorityRing
                | SourceType::RingOneshot
                | SourceType::PriorityRingOneshot
        ));

        match clip {
            Some(clip) => {
                let source =
                    WavSource::from_clip(clip, sample_rate, output_channels, volume, None, None)
                        .map_err(AudioError::Other)?;
                Ok(Box::new(match loop_pause {
                    Some(pause) => source.looped(pause),
                    None => source,
                }))
            }
            None => Ok(Box::new(self.into_waveform_source(
                sample_rate as f32,
                output_channels,
                volume,
            ))),
        }
    }

    pub(crate) fn into_waveform_source(
        self,
        sample_rate: f32,
        output_channels: usize,
        volume: f32,
    ) -> WaveformSource {
        match self {
            SourceType::RingOneshot => {
                SourceType::Ring.into_waveform_source(sample_rate, output_channels, volume)
            }
            SourceType::PriorityRingOneshot => {
                SourceType::PriorityRing.into_waveform_source(sample_rate, output_channels, volume)
            }
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
            SourceType::ParticipantJoined => WaveformSource::new(
                vec![
                    WaveformSegment::new(
                        WaveformTone::new(660.0, Waveform::Sine, 0.10),
                        Duration::from_millis(65),
                    ),
                    WaveformSegment::pause(Duration::from_millis(22)),
                    WaveformSegment::new(
                        WaveformTone::new(880.0, Waveform::Sine, 0.12),
                        Duration::from_millis(65),
                    ),
                    WaveformSegment::pause(Duration::from_millis(22)),
                    WaveformSegment::new(
                        WaveformTone::new(1100.0, Waveform::Sine, 0.14),
                        Duration::from_millis(90),
                    ),
                ],
                None,
                Duration::from_millis(8),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::ParticipantLeft => WaveformSource::new(
                vec![
                    WaveformSegment::new(
                        WaveformTone::new(1100.0, Waveform::Sine, 0.14),
                        Duration::from_millis(65),
                    ),
                    WaveformSegment::pause(Duration::from_millis(22)),
                    WaveformSegment::new(
                        WaveformTone::new(880.0, Waveform::Sine, 0.12),
                        Duration::from_millis(65),
                    ),
                    WaveformSegment::pause(Duration::from_millis(22)),
                    WaveformSegment::new(
                        WaveformTone::new(660.0, Waveform::Sine, 0.10),
                        Duration::from_millis(90),
                    ),
                ],
                None,
                Duration::from_millis(8),
                sample_rate,
                output_channels,
                volume,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::path::PathBuf;

    fn write_wav(dir: &Path, name: &str, channels: u16, sample_rate: u32, secs: f32) -> PathBuf {
        let path = dir.join(name);
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let frames = (sample_rate as f32 * secs) as usize;
        for i in 0..frames {
            let sample = ((i as f32 * 0.05).sin() * 0.9 * i16::MAX as f32) as i16;
            for _ in 0..channels {
                writer.write_sample(sample).unwrap();
            }
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn valid_clip_is_normalized_to_the_chime_peak() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_wav(dir.path(), "ring.wav", 2, 44_100, 1.5);

        let clip = load_ring_clip(&path).unwrap();
        assert!((clip.peak() - 0.2).abs() < 1e-3);
        assert!((clip.duration().as_secs_f32() - 1.5).abs() < 0.01);

        let source = SourceType::Ring.into_ring_source(Some(&clip), None, 48_000, 2, 0.5);
        assert!(source.is_ok());
    }

    #[test]
    fn oversized_clip_is_rejected_from_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_wav(dir.path(), "long.wav", 1, 8_000, 31.0);

        let err = load_ring_clip(&path).unwrap_err();
        assert!(matches!(err, RingSoundError::TooLong { .. }), "{err:?}");
        assert!(err.to_string().contains("maximum is 30 s"), "{err}");
    }

    #[test]
    fn truncated_and_silent_clips_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let short = write_wav(dir.path(), "short.wav", 1, 48_000, 0.05);
        let err = load_ring_clip(&short).unwrap_err();
        assert!(matches!(err, RingSoundError::TooShort { .. }), "{err:?}");
        assert!(err.to_string().contains("shorter than 100 ms"), "{err}");

        let silent = dir.path().join("silent.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&silent, spec).unwrap();
        for _ in 0..24_000 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        let err = load_ring_clip(&silent).unwrap_err();
        assert!(matches!(err, RingSoundError::Silent { .. }), "{err:?}");
        assert!(err.to_string().contains("only silence"), "{err}");
    }

    #[test]
    fn non_wav_file_is_rejected_with_the_path_in_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        std::fs::write(&path, b"ID3 definitely not a wav").unwrap();

        let err = load_ring_clip(&path).unwrap_err();
        assert!(matches!(err, RingSoundError::Decode { .. }), "{err:?}");
        assert!(err.to_string().contains("song.wav"), "{err}");
    }

    #[test]
    fn zero_sample_rate_header_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero-rate.wav");
        // hound refuses to write such a header, so the file is assembled by hand.
        let data = [0x00, 0x40].repeat(4_800);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        std::fs::write(&path, bytes).unwrap();

        let err = load_ring_clip(&path).unwrap_err();
        assert!(matches!(err, RingSoundError::Decode { .. }), "{err:?}");
        assert!(err.to_string().contains("sample rate of 0"), "{err}");
    }

    #[test]
    fn missing_clip_falls_back_to_the_built_in_chime() {
        assert!(
            SourceType::PriorityRing
                .into_ring_source(None, None, 48_000, 2, 0.5)
                .is_ok()
        );
    }

    fn rms_per_second(source: &mut dyn AudioSource, seconds: usize) -> Vec<f32> {
        source.start();
        (0..seconds)
            .map(|_| {
                let mut out = vec![0.0f32; 48_000];
                source.mix_into(&mut out);
                (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt()
            })
            .collect()
    }

    #[test]
    fn looped_custom_ring_repeats_after_the_pause() {
        let clip = WavClip::from_samples(vec![0.2; 48_000], 48_000);
        let mut source = SourceType::Ring
            .into_ring_source(Some(&clip), Some(Duration::from_secs(1)), 48_000, 1, 1.0)
            .unwrap();

        let rms = rms_per_second(source.as_mut(), 5);
        assert!(rms[0] > 0.1 && rms[2] > 0.1 && rms[4] > 0.1, "{rms:?}");
        assert!(rms[1] == 0.0 && rms[3] == 0.0, "{rms:?}");
    }

    #[test]
    fn oneshot_ring_and_built_in_chime_play_once_despite_a_loop_pause() {
        let clip = WavClip::from_samples(vec![0.2; 48_000], 48_000);
        let mut oneshot = SourceType::RingOneshot
            .into_ring_source(Some(&clip), None, 48_000, 1, 1.0)
            .unwrap();
        let mut built_in = SourceType::Ring
            .into_ring_source(None, Some(Duration::ZERO), 48_000, 1, 1.0)
            .unwrap();

        for source in [oneshot.as_mut(), built_in.as_mut()] {
            let rms = rms_per_second(source, 4);
            assert!(rms[0] > 0.0, "{rms:?}");
            assert!(rms[2..].iter().all(|r| *r == 0.0), "{rms:?}");
        }
    }
}
