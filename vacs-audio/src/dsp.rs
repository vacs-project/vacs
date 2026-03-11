use crate::TARGET_SAMPLE_RATE;
use biquad::{Biquad, Coefficients, DirectForm2Transposed, Q_BUTTERWORTH_F32, ToHertz, Type};

pub fn downmix_interleaved_to_mono(interleaved: &[f32], channels: usize, mono: &mut Vec<f32>) {
    debug_assert!(channels > 0);
    debug_assert_eq!(interleaved.len() % channels, 0);

    let frames = interleaved.len() / channels;
    mono.clear();
    mono.reserve(frames);
    for frame in interleaved.chunks(channels) {
        mono.push(downmix_frame_to_mono(frame));
    }
}

#[inline]
fn downmix_frame_to_mono(frame: &[f32]) -> f32 {
    match frame.len() {
        0 => 0.0f32,
        1 => frame[0],
        2 => {
            let (l, r) = (frame[0], frame[1]);
            if (l - r).abs() < 1e-4 {
                l
            } else {
                (l + r) * 0.5f32
            }
        }
        n => frame.iter().take(n).copied().sum::<f32>() / (n as f32),
    }
}

/// DC blocker pole r in y[n] = x[n] - x[n-1] + r * y[n-1]
/// Range: 0.990..=0.999; higher = lower cutoff (~closer to pure DC removal).
const DC_BLOCK_R: f32 = 0.995f32;

/// High-pass cutoff in Hz for speech rumble/plosive reduction.
/// Typical range: 80..=140 Hz (100 Hz is a good default).
const HPF_CUTOFF_HZ: f32 = 100.0f32;
/// HPF Q (Butterworth-like ~0.707 is flat in passband).
/// Range: 0.5..=1.0 (0.707 ≈ Butterworth).
const HPF_Q: f32 = Q_BUTTERWORTH_F32;

/// Noise gate open threshold in dBFS (RMS over frame).
/// Range: -60..=-30 dB. Less negative (e.g., -40) = gate opens more easily.
const GATE_OPEN_DB: f32 = -45.0f32;

/// Noise gate close threshold in dBFS (must be < open to create hysteresis).
/// Range: (GATE_OPEN_DB-10)..=(GATE_OPEN_DB-2).
const GATE_CLOSE_DB: f32 = -50.0f32;

/// Noise gate attack time (seconds). Faster = quicker unmute on speech start.
/// Range: 0.002..=0.020 (2-20 ms).
const GATE_ATTACK_S: f32 = 0.008f32; // 8 ms

/// Noise gate release time (seconds). Longer = smoother tails, fewer chops.
/// Range: 0.050..=0.200 (50-200 ms).
const GATE_RELEASE_S: f32 = 0.090f32; // 90 ms

/// Soft limiter ceiling in dBFS. Set just below 0 dBFS to avoid clipping.
/// Range: -6.0..=-0.1. More negative = gentler, more headroom.
const LIMITER_THR_DBFS: f32 = -1.0f32;

/// One-pole DC blocker (very low-cut high-pass).
/// Removes DC bias and sub-Hz drift without coloring audible band.
struct DcBlock {
    x1: f32, // x[n-1]
    y1: f32, // y[n-1]
    r: f32,  // pole, ~0.995..0.999
}

impl Default for DcBlock {
    fn default() -> Self {
        Self {
            x1: 0.0f32,
            y1: 0.0f32,
            r: DC_BLOCK_R,
        }
    }
}

impl DcBlock {
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // y[n] = x[n] - x[n-1] + r * y[n-1]
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// RMS-based downward expander with hysteresis and smooth attack/release.
/// Suppresses background when below threshold, avoids chattering near threshold.
struct NoiseGate {
    open_lin: f32,
    close_lin: f32,
    att_s: f32,
    rel_s: f32,
    fs: f32,
    gain: f32,
    target: f32,
}

impl Default for NoiseGate {
    fn default() -> Self {
        let lin = |db| 10.0f32.powf(db / 20.0f32);
        Self {
            open_lin: lin(GATE_OPEN_DB),
            close_lin: lin(GATE_CLOSE_DB),
            att_s: GATE_ATTACK_S,
            rel_s: GATE_RELEASE_S,
            fs: TARGET_SAMPLE_RATE as f32,
            gain: 0.0f32,
            target: 0.0f32,
        }
    }
}

impl NoiseGate {
    #[inline]
    fn coeff(&self, faster: bool) -> f32 {
        // One-pole smoothing coefficient; always (0,1)
        let tau = if faster { self.att_s } else { self.rel_s };
        let denom = (tau * self.fs).max(1e-6); // avoid div-by-zero
        1.0 - (-1.0 / denom).exp()
    }

    /// Process one full 10 ms frame (RMS measured over the frame).
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        // quick RMS
        let mut sum = 0.0f32;
        for &s in frame.iter() {
            sum += s * s;
        }
        let rms = (sum / frame.len() as f32).sqrt();

        if rms >= self.open_lin {
            self.target = 1.0f32;
        } else if rms <= self.close_lin {
            self.target = 0.0f32;
        }

        let a = self.coeff(true);
        let r = self.coeff(false);

        for s in frame.iter_mut() {
            let c = if self.target > self.gain { a } else { r };
            self.gain += c * (self.target - self.gain);
            *s *= self.gain;
        }
    }
}

/// Simple peak soft-knee limiter near 0 dBFS.
/// Transparent under normal speech; gently tames unexpected peaks.
struct SoftLimiter {
    thr: f32, // linear amplitude
}

impl Default for SoftLimiter {
    fn default() -> Self {
        Self {
            thr: 10.0f32.powf(LIMITER_THR_DBFS / 20.0f32),
        }
    }
}

impl SoftLimiter {
    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        for s in frame.iter_mut() {
            let a = s.abs();
            if a > self.thr {
                let sign = s.signum();
                let over = (a - self.thr) / (1.0f32 - self.thr + 1e-9f32);
                let soft = self.thr + over / (1.0f32 + over); // soft knee curve
                *s = sign * soft.min(0.9999f32);
            }
        }
    }
}

/// Capture-side chain for 48 kHz mono, 20 ms frames.
/// Apply on each full frame **before** Opus encoding.
pub struct MicProcessor {
    dc_block: DcBlock,
    hpf: DirectForm2Transposed<f32>,
    noise_gate: NoiseGate,
    soft_limiter: SoftLimiter,
}

impl Default for MicProcessor {
    fn default() -> Self {
        let coeffs = Coefficients::from_params(
            Type::HighPass,
            TARGET_SAMPLE_RATE.hz(),
            HPF_CUTOFF_HZ.hz(),
            HPF_Q,
        )
        .expect("Failed to create HPF coefficients");
        Self {
            dc_block: DcBlock::default(),
            hpf: DirectForm2Transposed::new(coeffs),
            noise_gate: NoiseGate::default(),
            soft_limiter: SoftLimiter::default(),
        }
    }
}

impl MicProcessor {
    /// Process one 20 ms (960-sample) frame at [`TARGET_SAMPLE_RATE`].
    /// Assumes frame is **mono f32** at the target rate.
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        // Per-sample IIR (stateful) stages first.
        for s in frame.iter_mut() {
            *s = self.dc_block.process(*s);
            *s = self.hpf.run(*s);
        }
        // Then frame-level dynamics.
        self.noise_gate.process_frame(frame);
        self.soft_limiter.process_frame(frame);
    }
}
