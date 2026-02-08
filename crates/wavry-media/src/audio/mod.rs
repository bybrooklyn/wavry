#![allow(dead_code)]

pub(crate) const OPUS_SAMPLE_RATE: u32 = 48_000;
pub(crate) const OPUS_CHANNELS: usize = 2;
pub(crate) const OPUS_FRAME_MS: u32 = 5;
pub(crate) const OPUS_FRAME_SAMPLES: usize =
    (OPUS_SAMPLE_RATE as usize / 1000) * (OPUS_FRAME_MS as usize);
pub(crate) const OPUS_MAX_FRAME_SAMPLES: usize = 5_760;
pub(crate) const OPUS_MAX_PACKET_BYTES: usize = 4_000;
pub(crate) const OPUS_BITRATE_BPS: i32 = 128_000;
pub(crate) const AUDIO_MAX_BUFFER_FRAMES: usize = 4;
pub(crate) const AUDIO_MAX_BUFFER_SAMPLES: usize =
    OPUS_FRAME_SAMPLES * OPUS_CHANNELS * AUDIO_MAX_BUFFER_FRAMES;

pub(crate) fn opus_frame_duration_us() -> u64 {
    (OPUS_FRAME_SAMPLES as u64) * 1_000_000 / (OPUS_SAMPLE_RATE as u64)
}

pub mod renderer;
