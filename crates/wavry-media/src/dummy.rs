use anyhow::Result;
use crate::{DecodeConfig, EncodeConfig, EncodedFrame};
use std::time::{Duration, Instant};

pub struct DummyEncoder {
    start: Instant,
    seq: u64,
    fps: u16,
}

impl DummyEncoder {
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        Ok(Self {
            start: Instant::now(),
            seq: 0,
            fps: config.fps,
        })
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        // Simulate frame timing
        let frame_interval = Duration::from_secs_f64(1.0 / self.fps as f64);
        let target_time = self.start + frame_interval * self.seq as u32;
        
        if let Some(wait) = target_time.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }

        let timestamp_us = self.start.elapsed().as_micros() as u64;
        self.seq += 1;

        Ok(EncodedFrame {
            timestamp_us,
            keyframe: self.seq.is_multiple_of(60),
            data: vec![0x99; 1000], // Dummy payload
        })
    }
}

pub struct DummyRenderer;

impl DummyRenderer {
    pub fn new(_config: DecodeConfig) -> Result<Self> {
        Ok(Self)
    }

    pub fn push(&mut self, _data: &[u8], _timestamp_us: u64) -> Result<()> {
        // Just log (or do nothing)
        // tracing::debug!("Rendered frame: {} bytes @ {} us", data.len(), timestamp_us);
        Ok(())
    }
}
