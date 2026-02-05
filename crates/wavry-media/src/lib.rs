// #![forbid(unsafe_code)]

use anyhow::Result;
#[cfg(unix)]
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
    #[cfg(unix)]
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

pub trait Renderer: Send {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()>;
}

// Input Types abstraction (simplified for now)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton { Left, Right, Middle }

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    MouseMove { x: f32, y: f32 }, // Normalized 0..1
    MouseDown { button: MouseButton },
    MouseUp { button: MouseButton },
    KeyDown { key_code: u32 }, // Platform specific for now (Mac: CGKeyCode)
    KeyUp { key_code: u32 },
    Scroll { dx: f32, dy: f32 },
}

pub trait InputInjector: Send {
    fn inject(&mut self, event: InputEvent) -> Result<()>;
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
pub use linux::{PipewireEncoder, GstVideoRenderer, GstAudioRenderer, PipewireAudioCapturer};

mod dummy;
pub use dummy::{DummyEncoder, DummyRenderer};

#[cfg(target_os = "macos")]
mod mac_screen_encoder;
#[cfg(target_os = "macos")]
mod mac_video_renderer;

#[cfg(target_os = "macos")]
pub use mac_screen_encoder::MacScreenEncoder;
#[cfg(target_os = "macos")]
pub use mac_video_renderer::MacVideoRenderer;

#[cfg(target_os = "macos")]
mod mac_input_injector;
#[cfg(target_os = "macos")]
pub use mac_input_injector::MacInputInjector;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{WindowsEncoder, WindowsRenderer, WindowsAudioRenderer, WindowsAudioCapturer};
