use crate::{Codec, DecodeConfig, Renderer};
use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::ptr::NonNull;

#[cfg(target_os = "android")]
use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
    MediaFormat,
};
#[cfg(target_os = "android")]
use ndk::native_window::NativeWindow;
#[cfg(target_os = "android")]
use ndk_sys::ANativeWindow;

pub struct AndroidVideoRenderer {
    #[cfg(target_os = "android")]
    codec: MediaCodec,
    #[cfg(target_os = "android")]
    _native_window: NonNull<ANativeWindow>,
    #[cfg(not(target_os = "android"))]
    _dummy: (),
}

impl AndroidVideoRenderer {
    pub fn new(config: DecodeConfig, native_window: *mut c_void) -> Result<Self> {
        #[cfg(target_os = "android")]
        {
            if native_window.is_null() {
                return Err(anyhow!("Native window is null"));
            }
            let nw = NonNull::new(native_window as *mut ANativeWindow).unwrap();

            let mime = match config.codec {
                Codec::H264 => "video/avc",
                Codec::Hevc => "video/hevc",
                Codec::Av1 => "video/av01",
            };

            log::info!(
                "Initializing Android MediaCodec ({}) for {}x{}",
                mime,
                config.resolution.width,
                config.resolution.height
            );

            let format = MediaFormat::new();
            format.set_str("mime", mime);
            format.set_i32("width", config.resolution.width as i32);
            format.set_i32("height", config.resolution.height as i32);

            // Optional: Set some extra parameters for low latency
            format.set_i32("low-latency", 1);
            format.set_i32("vendor.rtc-ext-dec-low-latency.enable", 1);

            let codec = MediaCodec::from_decoder_type(mime)
                .ok_or_else(|| anyhow!("Failed to create MediaCodec for {}", mime))?;

            let nw_ref = unsafe { NativeWindow::from_ptr(nw) };

            codec
                .configure(&format, Some(&nw_ref), MediaCodecDirection::Decoder)
                .map_err(|e| anyhow!("MediaCodec configure failed: {:?}", e))?;

            codec
                .start()
                .map_err(|e| anyhow!("MediaCodec start failed: {:?}", e))?;

            Ok(Self {
                codec,
                _native_window: nw,
            })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = config;
            let _ = native_window;
            Err(anyhow!("AndroidVideoRenderer only supported on Android"))
        }
    }
}

impl Renderer for AndroidVideoRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            // 1. Queue input buffer
            match self
                .codec
                .dequeue_input_buffer(std::time::Duration::from_millis(5))
            {
                Ok(DequeuedInputBufferResult::Buffer(mut buffer)) => {
                    let buffer_data = buffer.buffer_mut();
                    let len = payload.len().min(buffer_data.len());
                    // SAFETY: buffer_data is a slice of MaybeUninit<u8>, payload is [u8]
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            payload.as_ptr(),
                            buffer_data.as_mut_ptr() as *mut u8,
                            len,
                        );
                    }

                    self.codec
                        .queue_input_buffer(buffer, 0, len, timestamp_us, 0)
                        .map_err(|e| anyhow!("Failed to queue input buffer: {:?}", e))?;
                }
                Ok(DequeuedInputBufferResult::TryAgainLater) => {
                    log::debug!("No input buffer available for decoding");
                }
                Err(e) => {
                    return Err(anyhow!("Error dequeuing input buffer: {:?}", e));
                }
            }

            // 2. Dequeue output buffer and release to surface
            // We do this in the same call for simplicity, but in a high-perf scenario
            // we might want a separate thread or more frequent polling.
            loop {
                match self
                    .codec
                    .dequeue_output_buffer(std::time::Duration::from_micros(0))
                {
                    Ok(DequeuedOutputBufferInfoResult::Buffer(buffer)) => {
                        self.codec
                            .release_output_buffer(buffer, true)
                            .map_err(|e| anyhow!("Failed to release output buffer: {:?}", e))?;
                    }
                    Ok(DequeuedOutputBufferInfoResult::TryAgainLater) => break,
                    Ok(DequeuedOutputBufferInfoResult::OutputFormatChanged) => {
                        log::info!("MediaCodec output format changed");
                    }
                    Ok(DequeuedOutputBufferInfoResult::OutputBuffersChanged) => {
                        log::info!("MediaCodec output buffers changed");
                    }
                    Err(e) => {
                        log::warn!("Error dequeuing output buffer: {:?}", e);
                        break;
                    }
                }
            }

            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = payload;
            let _ = timestamp_us;
            Ok(())
        }
    }
}

unsafe impl Send for AndroidVideoRenderer {}
