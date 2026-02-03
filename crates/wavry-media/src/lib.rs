#![forbid(unsafe_code)]

use anyhow::Result;
use std::os::fd::OwnedFd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Hevc,
    H264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    Rgba8,
    Nv12,
}

#[derive(Debug)]
pub enum FrameData {
    Cpu { bytes: Vec<u8>, stride: u32 },
    Dmabuf {
        fd: OwnedFd,
        stride: u32,
        offset: u32,
        modifier: u64,
        size: u32,
    },
}

#[derive(Debug)]
pub struct RawFrame {
    pub width: u16,
    pub height: u16,
    pub format: FrameFormat,
    pub timestamp_us: u64,
    pub data: FrameData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeConfig {
    pub codec: Codec,
    pub resolution: Resolution,
    pub fps: u16,
    pub bitrate_kbps: u32,
    pub keyframe_interval_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeConfig {
    pub codec: Codec,
    pub resolution: Resolution,
}

pub trait Encoder: Send {
    fn encode(&mut self, frame: RawFrame) -> Result<EncodedFrame>;
}

pub trait Decoder: Send {
    fn decode(&mut self, payload: &[u8], timestamp_us: u64) -> Result<RawFrame>;
}

pub trait CapabilityProbe: Send + Sync {
    fn supported_encoders(&self) -> Result<Vec<Codec>>;
    fn supported_decoders(&self) -> Result<Vec<Codec>>;
}

pub struct NullProbe;

impl CapabilityProbe for NullProbe {
    fn supported_encoders(&self) -> Result<Vec<Codec>> {
        Ok(vec![])
    }

    fn supported_decoders(&self) -> Result<Vec<Codec>> {
        Ok(vec![])
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{PipewireEncoder, GstVideoRenderer};
