pub mod device;
mod dsp;
pub mod error;
pub(crate) mod mixer;
pub mod sources;
pub mod stream;

pub type EncodedAudioFrame = bytes::Bytes;

pub const TARGET_SAMPLE_RATE: u32 = 48_000;
pub const FRAME_DURATION_MS: u64 = 20;
const FRAME_SIZE: usize = TARGET_SAMPLE_RATE as usize * FRAME_DURATION_MS as usize / 1000;
