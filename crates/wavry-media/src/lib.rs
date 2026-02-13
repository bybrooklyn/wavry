// #![forbid(unsafe_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::fd::OwnedFd;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Codec {
    Av1,
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
    Cpu {
        bytes: Vec<u8>,
        stride: u32,
    },
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    pub display_id: Option<u32>,
    pub enable_10bit: bool,
    pub enable_hdr: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeConfig {
    pub codec: Codec,
    pub resolution: Resolution,
    pub enable_10bit: bool,
    pub enable_hdr: bool,
}

pub trait Encoder: Send {
    fn encode(&mut self, frame: RawFrame) -> Result<EncodedFrame>;
}

pub trait Decoder: Send {
    fn decode(&mut self, payload: &[u8], timestamp_us: u64) -> Result<RawFrame>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoCodecCapability {
    pub codec: Codec,
    pub hardware_accelerated: bool,
    pub supports_10bit: bool,
    pub supports_hdr10: bool,
}

impl VideoCodecCapability {
    pub const fn sdr(codec: Codec, hardware_accelerated: bool) -> Self {
        Self {
            codec,
            hardware_accelerated,
            supports_10bit: false,
            supports_hdr10: false,
        }
    }
}

pub trait CapabilityProbe: Send + Sync {
    fn supported_encoders(&self) -> Result<Vec<Codec>>;
    fn supported_decoders(&self) -> Result<Vec<Codec>>;
    fn enumerate_displays(&self) -> Result<Vec<DisplayInfo>>;

    fn encoder_capabilities(&self) -> Result<Vec<VideoCodecCapability>> {
        Ok(self
            .supported_encoders()?
            .into_iter()
            .map(|codec| VideoCodecCapability::sdr(codec, false))
            .collect())
    }

    fn supported_hardware_encoders(&self) -> Result<Vec<Codec>> {
        Ok(self
            .encoder_capabilities()?
            .into_iter()
            .filter(|cap| cap.hardware_accelerated)
            .map(|cap| cap.codec)
            .collect())
    }
}

pub trait Renderer: Send {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()>;
}

// Input Types abstraction (simplified for now)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GamepadAxis {
    pub axis: u32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GamepadButton {
    pub button: u32,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    MouseMove {
        x: f32,
        y: f32,
    }, // Normalized 0..1
    MouseDown {
        button: MouseButton,
    },
    MouseUp {
        button: MouseButton,
    },
    KeyDown {
        key_code: u32,
    },
    KeyUp {
        key_code: u32,
    },
    Scroll {
        dx: f32,
        dy: f32,
    },
    Gamepad {
        gamepad_id: u32,
        axes: Vec<GamepadAxis>,
        buttons: Vec<GamepadButton>,
    },
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

    fn enumerate_displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(vec![])
    }
}

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
pub use android::{AndroidAudioRenderer, AndroidProbe, AndroidVideoRenderer};

pub mod buffer_pool;
pub use buffer_pool::{
    FrameBuffer, FrameBufferPool, FrameBufferPoolConfig, ReorderBuffer, ReorderError,
};

pub mod encoder_pool;
pub use encoder_pool::{
    EncoderConfig, EncoderPool, EncoderPoolConfig, EncoderPoolStats, MemoryPressure, PooledEncoder,
    ReferenceFrame, ReferenceFrameManager, StagingBuffer, StagingBufferPool,
};

pub mod recorder;
pub use recorder::{Quality, RecorderConfig, VideoRecorder};

#[cfg(target_os = "linux")]
mod linux;

mod audio;

#[cfg(target_os = "linux")]
pub use linux::{
    linux_runtime_diagnostics, GstAudioRenderer, GstVideoRenderer, LinuxProbe,
    LinuxRuntimeDiagnostics, PipewireAudioCapturer, PipewireEncoder,
};

mod dummy;
pub use dummy::{DummyEncoder, DummyRenderer};

#[cfg(target_os = "macos")]
mod mac_screen_encoder;
#[cfg(target_os = "macos")]
mod mac_video_renderer;

#[cfg(target_os = "macos")]
pub use mac_screen_encoder::{MacProbe, MacScreenEncoder};
#[cfg(target_os = "macos")]
pub use mac_video_renderer::MacVideoRenderer;

#[cfg(target_os = "macos")]
mod mac_input_injector;
#[cfg(target_os = "macos")]
pub use mac_input_injector::MacInputInjector;

#[cfg(target_os = "macos")]
mod mac_audio_capturer;
#[cfg(target_os = "macos")]
pub use mac_audio_capturer::{MacAudioCapturer, MacAudioRoute};

#[cfg(target_os = "macos")]
mod mac_audio_renderer;
#[cfg(target_os = "macos")]
pub use mac_audio_renderer::MacAudioRenderer;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::{
    WindowsAudioCapturer, WindowsAudioRenderer, WindowsEncoder, WindowsInputInjector, WindowsProbe,
    WindowsRenderer,
};
