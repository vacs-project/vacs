//! WAV writer for replay clip files.
//!
//! Writes 16-bit PCM mono using [`hound`], downmixing interleaved input to mono and
//! converting f32 samples to i16 with clipping. Sample rate is taken from the source
//! (no resampling); inputs that are not already 48 kHz are written at their native rate.

use crate::replay::ReplayError;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use vacs_audio::dsp::downmix_interleaved_to_mono;

const TARGET_BITS_PER_SAMPLE: u16 = 16;
const TARGET_CHANNELS: u16 = 1;

/// Streams f32 frames into a 16-bit PCM mono WAV file.
pub struct ClipWriter {
    inner: WavWriter<BufWriter<File>>,
    input_channels: u16,
    sample_count: u64,
    sample_rate: u32,
    mono_scratch: Vec<f32>,
}

impl ClipWriter {
    /// Open a new WAV file at `path`. The file is created (or truncated) and the WAV
    /// header is reserved for finalization.
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> Result<Self, ReplayError> {
        if channels == 0 {
            return Err(ReplayError::Wav(
                "channels must be greater than zero".to_owned(),
            ));
        }
        if sample_rate == 0 {
            return Err(ReplayError::Wav(
                "sample_rate must be greater than zero".to_owned(),
            ));
        }

        let spec = WavSpec {
            channels: TARGET_CHANNELS,
            sample_rate,
            bits_per_sample: TARGET_BITS_PER_SAMPLE,
            sample_format: SampleFormat::Int,
        };
        let inner = WavWriter::create(path, spec).map_err(map_hound)?;

        Ok(Self {
            inner,
            input_channels: channels,
            sample_count: 0,
            sample_rate,
            mono_scratch: Vec::new(),
        })
    }

    /// Append interleaved f32 samples. The buffer length must be a multiple of the
    /// configured input channel count; trailing samples that do not form a full frame
    /// are dropped.
    pub fn write_frame(&mut self, samples: &[f32]) -> Result<(), ReplayError> {
        let channels = usize::from(self.input_channels);
        let usable = samples.len() - (samples.len() % channels);
        if usable == 0 {
            return Ok(());
        }

        downmix_interleaved_to_mono(&samples[..usable], channels, &mut self.mono_scratch);

        for &mono in &self.mono_scratch {
            let pcm = f32_to_i16(mono);
            self.inner.write_sample(pcm).map_err(map_hound)?;
            self.sample_count += 1;
        }

        Ok(())
    }

    /// Finalize the WAV header and return the clip duration in milliseconds.
    pub fn finalize(self) -> Result<u64, ReplayError> {
        let sample_count = self.sample_count;
        let sample_rate = u64::from(self.sample_rate);

        self.inner.finalize().map_err(map_hound)?;

        if sample_rate == 0 {
            return Ok(0);
        }

        Ok(sample_count.saturating_mul(1_000) / sample_rate)
    }
}

fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * f32::from(i16::MAX)) as i16
}

fn map_hound(err: hound::Error) -> ReplayError {
    match err {
        hound::Error::IoError(io) => ReplayError::Io(io),
        other => ReplayError::Wav(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use tempfile::tempdir;

    #[test]
    fn writes_mono_input_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mono.wav");
        let mut writer = ClipWriter::create(&path, 48_000, 1).unwrap();
        writer.write_frame(&[0.0, 0.5, -0.5, 1.0, -1.0]).unwrap();
        let duration = writer.finalize().unwrap();

        let reader = WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 16);
        let samples: Vec<i16> = reader.into_samples().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 5);
        assert_eq!(samples[0], 0);
        assert_eq!(samples[3], i16::MAX);
        assert_eq!(samples[4], -i16::MAX);
        // 5 samples / 48000 Hz = 0 ms (rounded down).
        assert_eq!(duration, 0);
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        let mut writer = ClipWriter::create(&path, 48_000, 2).unwrap();
        // Two stereo frames: (1.0, -1.0) and (0.5, 0.5).
        writer.write_frame(&[1.0, -1.0, 0.5, 0.5]).unwrap();
        writer.finalize().unwrap();

        let reader = WavReader::open(&path).unwrap();
        let samples: Vec<i16> = reader.into_samples().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 2);
        // (1.0 + -1.0) / 2 = 0.0
        assert_eq!(samples[0], 0);
        // (0.5 + 0.5) / 2 = 0.5
        assert!((samples[1] - (i16::MAX as f32 * 0.5) as i16).abs() <= 1);
    }

    #[test]
    fn clips_out_of_range_samples() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        let mut writer = ClipWriter::create(&path, 48_000, 1).unwrap();
        writer.write_frame(&[2.0, -2.0, 1.5]).unwrap();
        writer.finalize().unwrap();
        let reader = WavReader::open(&path).unwrap();
        let samples: Vec<i16> = reader.into_samples().map(Result::unwrap).collect();
        assert_eq!(samples, vec![i16::MAX, -i16::MAX, i16::MAX]);
    }

    #[test]
    fn drops_partial_trailing_frame() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.wav");
        let mut writer = ClipWriter::create(&path, 48_000, 2).unwrap();
        // 3 floats with channels=2: only one full stereo frame consumed.
        writer.write_frame(&[1.0, -1.0, 0.5]).unwrap();
        writer.finalize().unwrap();
        let reader = WavReader::open(&path).unwrap();
        let samples: Vec<i16> = reader.into_samples().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn duration_ms_matches_sample_count() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("duration.wav");
        let mut writer = ClipWriter::create(&path, 48_000, 1).unwrap();
        writer.write_frame(&vec![0.0_f32; 48_000]).unwrap();
        let duration = writer.finalize().unwrap();
        assert_eq!(duration, 1_000);
    }

    #[test]
    fn rejects_invalid_format() {
        let dir = tempdir().unwrap();
        assert!(ClipWriter::create(&dir.path().join("zero.wav"), 0, 1).is_err());
        assert!(ClipWriter::create(&dir.path().join("zero.wav"), 48_000, 0).is_err());
    }
}
