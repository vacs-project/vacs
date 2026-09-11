use crate::dsp::downmix_interleaved_to_mono;
use crate::sources::AudioSource;
use anyhow::{Context, Result};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler, WindowFunction};
use std::path::Path;
use std::time::Duration;

pub struct WavSource {
    samples: Vec<f32>, // mono f32, resampled to output sample_rate

    sample_rate: u32,
    output_channels: usize,
    volume: f32,

    active: bool,
    pos: usize,

    update_interval: usize,                     // in ms, defaults to 100ms
    on_update: Option<Box<dyn Fn(f32) + Send>>, // progress from 0.0 to 1.0
}

impl WavSource {
    pub fn from_file(
        path: impl AsRef<Path>,
        sample_rate: u32,
        output_channels: usize,
        volume: f32,
        update_interval: Option<usize>,
        on_update: Option<Box<dyn Fn(f32) + Send>>,
    ) -> Result<Self> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let file_channels = spec.channels as usize;

        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let max_val = (1u32 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|x| x as f32 / max_val))
                    .collect::<Result<_, _>>()?
            }
        };

        let mut samples = if file_channels == 1 {
            interleaved
        } else {
            let mut mono = Vec::new();
            downmix_interleaved_to_mono(&interleaved, file_channels, &mut mono);
            mono
        };

        if spec.sample_rate != sample_rate {
            samples = resample(&samples, spec.sample_rate as usize, sample_rate as usize)?;
        }

        Ok(Self {
            samples,
            pos: 0,
            sample_rate,
            output_channels: output_channels.max(1),
            volume: volume.clamp(0.0, 1.0),
            active: false,
            update_interval: update_interval.unwrap_or(500),
            on_update,
        })
    }
}

impl AudioSource for WavSource {
    fn mix_into(&mut self, output: &mut [f32]) {
        if !self.active || self.volume == 0.0 {
            return;
        }

        if self.pos >= self.samples.len() {
            if let Some(on_update) = &self.on_update {
                on_update(1.0);
            }
            self.active = false;
            return;
        }

        for frame in output.chunks_mut(self.output_channels) {
            let sample = self.samples[self.pos] * self.volume;
            self.pos += 1;
            for s in frame.iter_mut() {
                *s += sample;
            }

            if self.pos >= self.samples.len() {
                if let Some(on_update) = &self.on_update {
                    on_update(1.0);
                }
                self.active = false;
                break;
            }

            if let Some(on_update) = &self.on_update
                && self.pos.is_multiple_of(self.update_interval)
            {
                let elapsed = self.pos as f32 / self.samples.len() as f32;
                on_update(elapsed);
            }
        }
    }

    fn start(&mut self) {
        self.active = true;
    }

    fn stop(&mut self) {
        self.active = false;
    }

    fn restart(&mut self) {
        self.pos = 0;
        self.active = true;
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    fn skip(&mut self, duration: Duration) {
        let frames = (duration.as_secs_f32() * self.sample_rate as f32).round() as usize;
        self.pos = (self.pos + frames).min(self.samples.len().saturating_sub(1)); // "- 1" to allow mix_into to finish
    }

    fn rewind(&mut self, duration: Duration) {
        let frames = (duration.as_secs_f32() * self.sample_rate as f32).round() as usize;
        self.pos = self.pos - frames.min(self.pos);
    }
}

fn resample(samples: &[f32], in_rate: usize, out_rate: usize) -> anyhow::Result<Vec<f32>> {
    let mut resampler = Fft::<f32>::new_custom(
        in_rate,
        out_rate,
        1024,
        2,
        1,
        WindowFunction::BlackmanHarris2,
        FixedSync::Input,
    )
    .context("Failed to construct WAV resampler")?;

    let input_frames = samples.len();
    // `process_all_needed_output_len` returns an upper bound that includes the
    // anti-aliasing filter's ringout, so the buffer has to be allocated at that size.
    let output_frames = resampler.process_all_needed_output_len(input_frames);
    let mut out = vec![0.0f32; output_frames];

    let input_adapter = InterleavedSlice::new(samples, 1, input_frames)
        .context("Failed to create resampler input adapter")?;
    let mut output_adapter = InterleavedSlice::new_mut(&mut out, 1, output_frames)
        .context("Failed to create resampler output adapter")?;

    resampler
        .process_all_into_buffer(&input_adapter, &mut output_adapter, input_frames, None)
        .context("Failed to resample WAV audio")?;

    // Trim the ringout back to the clip's actual duration. The resampler already
    // compensates its own delay, so the audio starts at frame 0 and only the tail is
    // excess. Leaving it in would append silence to every resampled clip and inflate
    // the denominator behind `AudioSource` progress reporting and seeking.
    let duration_frames = (input_frames as u64 * out_rate as u64).div_ceil(in_rate as u64) as usize;
    out.truncate(duration_frames);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{WavSource, resample};
    use crate::sources::AudioSource;
    use std::sync::{Arc, Mutex};

    #[test]
    fn finished_source_stays_silent_when_started_again() {
        let updates = Arc::new(Mutex::new(Vec::new()));
        let sink = updates.clone();
        let mut source = WavSource {
            samples: vec![0.5; 8],
            pos: 0,
            sample_rate: 48_000,
            output_channels: 1,
            volume: 1.0,
            active: true,
            update_interval: 500,
            on_update: Some(Box::new(move |p| sink.lock().unwrap().push(p))),
        };

        let mut out = vec![0.0f32; 16];
        source.mix_into(&mut out);
        assert!(!source.active);
        assert_eq!(updates.lock().unwrap().as_slice(), &[1.0]);

        source.start();
        let mut out = vec![0.0f32; 16];
        source.mix_into(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(!source.active);
        assert_eq!(updates.lock().unwrap().as_slice(), &[1.0, 1.0]);
    }

    /// Generate a mono sine of `freq` Hz at `rate` for `secs` seconds.
    fn sine(freq: f32, rate: usize, secs: f32) -> Vec<f32> {
        let frames = (rate as f32 * secs) as usize;
        (0..frames)
            .map(|n| (std::f32::consts::TAU * freq * n as f32 / rate as f32).sin())
            .collect()
    }

    /// Estimate the dominant frequency of a clean sine via zero crossings,
    /// ignoring the head and tail where the resampler's filter ramps up/down.
    fn dominant_freq(samples: &[f32], rate: usize) -> f32 {
        let skip = samples.len() / 10;
        let body = &samples[skip..samples.len() - skip];
        let crossings = body
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f32 * rate as f32 / body.len() as f32
    }

    fn peak(samples: &[f32]) -> f32 {
        let skip = samples.len() / 10;
        samples[skip..samples.len() - skip]
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()))
    }

    #[test]
    fn resample_output_length_matches_clip_duration() {
        // The resampler compensates its own delay, so output length must correspond
        // exactly to the clip's duration. Any excess is filter ringout that would show
        // up as appended silence and skew progress reporting.
        for (in_rate, out_rate) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (22_050, 48_000),
        ] {
            let input = sine(1_000.0, in_rate, 1.0);
            let expected = (input.len() as u64 * out_rate as u64).div_ceil(in_rate as u64) as usize;
            let out = resample(&input, in_rate, out_rate).unwrap();

            assert_eq!(
                out.len(),
                expected,
                "{in_rate}->{out_rate} produced {} frames, expected {expected}",
                out.len()
            );
        }
    }

    #[test]
    fn resample_does_not_append_silence() {
        // Guards the trim: a clip that is loud right up to its final frame must stay
        // loud right up to its final frame after resampling.
        let input = sine(1_000.0, 44_100, 1.0);
        let out = resample(&input, 44_100, 48_000).unwrap();

        let tail_peak = out[out.len() - 480..]
            .iter()
            .fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(
            tail_peak > 0.9,
            "last 10 ms of the clip decayed to {tail_peak}, ringout was not trimmed"
        );
    }

    #[test]
    fn resample_is_time_aligned() {
        // The resampler must compensate its own delay: a burst starting 1000 frames in
        // must land at the corresponding output frame, not later.
        let (in_rate, out_rate) = (44_100usize, 48_000usize);
        let mut input = vec![0.0f32; 1_000];
        input.extend(sine(1_000.0, in_rate, 0.05));
        input.extend(vec![0.0f32; 1_000]);

        let out = resample(&input, in_rate, out_rate).unwrap();

        let expected_start = 1_000 * out_rate / in_rate;
        let actual_start = out.iter().position(|s| s.abs() > 0.01).unwrap();
        assert!(
            actual_start.abs_diff(expected_start) < 32,
            "burst landed at frame {actual_start}, expected ~{expected_start}"
        );
    }

    #[test]
    fn resample_preserves_tone_and_amplitude() {
        // Sweep the device rates we realistically see, in both directions.
        for (in_rate, out_rate) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (48_000, 96_000),
            (32_000, 48_000),
            (22_050, 48_000),
        ] {
            let input = sine(1_000.0, in_rate, 1.0);
            let out = resample(&input, in_rate, out_rate).unwrap();

            assert!(
                out.iter().all(|s| s.is_finite()),
                "{in_rate}->{out_rate} produced non-finite samples"
            );

            let freq = dominant_freq(&out, out_rate);
            assert!(
                (freq - 1_000.0).abs() < 20.0,
                "{in_rate}->{out_rate} shifted the tone to {freq} Hz"
            );

            let amp = peak(&out);
            assert!(
                (amp - 1.0).abs() < 0.05,
                "{in_rate}->{out_rate} changed amplitude to {amp}"
            );
        }
    }

    #[test]
    fn resample_handles_short_and_empty_input() {
        // Shorter than one chunk, and empty — neither should panic.
        assert!(resample(&sine(1_000.0, 44_100, 0.005), 44_100, 48_000).is_ok());
        assert!(resample(&[], 44_100, 48_000).is_ok());
    }
}
