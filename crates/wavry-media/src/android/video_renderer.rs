use crate::{DecodeConfig, Renderer};
use anyhow::Result;
use std::ffi::c_void;

pub struct AndroidVideoRenderer {
    _config: DecodeConfig,
    _native_window: *mut c_void,
}

impl AndroidVideoRenderer {
    pub fn new(config: DecodeConfig, native_window: *mut c_void) -> Result<Self> {
        log::info!(
            "Initializing Android Video Renderer for {:?} with window {:?}",
            config.codec,
            native_window
        );
        // In a real implementation, we would:
        // 1. Initialize MediaCodec for the given codec
        // 2. Set the native_window as the output surface
        Ok(Self {
            _config: config,
            _native_window: native_window,
        })
    }
}

impl Renderer for AndroidVideoRenderer {
    fn render(&mut self, _payload: &[u8], _timestamp_us: u64) -> Result<()> {
        // Feed payload to MediaCodec
        Ok(())
    }
}

unsafe impl Send for AndroidVideoRenderer {}
