// Windows stubs for wavry-media
// These are placeholder implementations that return errors or no-ops
// until real Windows implementations are added.

use anyhow::{anyhow, Result};
use crate::{DecodeConfig, EncodeConfig, EncodedFrame, Renderer};

/// Windows screen encoder stub
pub struct WindowsEncoder;

impl WindowsEncoder {
    pub async fn new(_config: EncodeConfig) -> Result<Self> {
        Err(anyhow!("Windows screen capture not yet implemented"))
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        Err(anyhow!("Windows screen capture not yet implemented"))
    }

    pub fn set_bitrate(&mut self, _bitrate_kbps: u32) -> Result<()> {
        Ok(())
    }
}

/// Windows video renderer stub
pub struct WindowsRenderer;

impl WindowsRenderer {
    pub fn new(_config: DecodeConfig) -> Result<Self> {
        Ok(Self)
    }
}

impl Renderer for WindowsRenderer {
    fn render(&mut self, _payload: &[u8], _timestamp_us: u64) -> Result<()> {
        // No-op on Windows for now
        Ok(())
    }
}

/// Windows audio renderer stub
pub struct WindowsAudioRenderer;

impl WindowsAudioRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn play(&mut self, _data: &[u8]) -> Result<()> {
        Ok(())
    }
}

/// Windows audio capturer stub
pub struct WindowsAudioCapturer;

impl WindowsAudioCapturer {
    pub async fn new() -> Result<Self> {
        Err(anyhow!("Windows audio capture not yet implemented"))
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        Err(anyhow!("Windows audio capture not yet implemented"))
    }
}
