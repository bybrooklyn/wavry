use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(target_os = "macos")]
use cpal::{BufferSize, SampleFormat, SampleRate, Stream, StreamConfig, SupportedBufferSize};
#[cfg(target_os = "macos")]
use dispatch2::{DispatchObject, DispatchRetained};
#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use objc2::{define_class, AnyThread, DefinedClass, Message};
#[cfg(target_os = "macos")]
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, AudioBuffer, AudioBufferList, AudioFormatFlags,
};
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFAllocator, CFRetained};
#[cfg(target_os = "macos")]
use objc2_core_media::{
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, CMAudioFormatDescription,
    CMBlockBuffer, CMSampleBuffer, CMTime,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::{
    SCContentFilter, SCRunningApplication, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamOutput, SCStreamOutputType,
};
use std::collections::VecDeque;
use std::ptr::NonNull;
#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::Instant;
#[cfg(target_os = "macos")]
use tokio::sync::oneshot;

use crate::audio::{
    opus_frame_duration_us, AUDIO_MAX_BUFFER_SAMPLES, OPUS_CHANNELS, OPUS_SAMPLE_RATE,
};
#[cfg(feature = "opus-support")]
use crate::audio::{OPUS_BITRATE_BPS, OPUS_FRAME_SAMPLES, OPUS_MAX_PACKET_BYTES};
use crate::EncodedFrame;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacAudioRoute {
    SystemMix,
    Microphone,
    Application(String),
}

#[cfg(target_os = "macos")]
#[cfg(feature = "opus-support")]
use opus::{Application, Channels, Encoder as OpusEncoder};

#[cfg(target_os = "macos")]
struct AudioContext {
    // `tx` and `frame_duration_us` are only consumed inside the
    // `#[cfg(feature = "opus-support")]` encoding path; they are intentionally
    // kept in the struct unconditionally so `AudioHandler::new` has a
    // consistent shape regardless of features.
    #[cfg_attr(not(feature = "opus-support"), allow(dead_code))]
    tx: mpsc::Sender<EncodedFrame>,
    start_time: Instant,
    #[cfg(feature = "opus-support")]
    encoder: OpusEncoder,
    pcm: VecDeque<i16>,
    next_timestamp_us: Option<u64>,
    #[cfg_attr(not(feature = "opus-support"), allow(dead_code))]
    frame_duration_us: u64,
    channels: usize,
}

#[cfg(target_os = "macos")]
impl AudioContext {
    fn ingest_chunk(&mut self, chunk: PcmChunk) {
        if self.next_timestamp_us.is_none() || self.pcm.is_empty() {
            self.next_timestamp_us = Some(chunk.timestamp_us);
        }

        self.pcm.extend(chunk.samples);

        if self.pcm.len() > AUDIO_MAX_BUFFER_SAMPLES {
            let drop = self.pcm.len() - AUDIO_MAX_BUFFER_SAMPLES;
            let aligned_drop = drop - (drop % self.channels.max(1));
            for _ in 0..aligned_drop {
                self.pcm.pop_front();
            }
            if let Some(ts) = self.next_timestamp_us.as_mut() {
                let frames_dropped = aligned_drop / self.channels.max(1);
                let advance = (frames_dropped as u64) * 1_000_000 / (OPUS_SAMPLE_RATE as u64);
                *ts = ts.saturating_add(advance);
            }
        }

        #[cfg(feature = "opus-support")]
        {
            let frame_len = OPUS_FRAME_SAMPLES * self.channels;
            while self.pcm.len() >= frame_len {
                let frame: Vec<i16> = self.pcm.drain(..frame_len).collect();
                let mut out = vec![0u8; OPUS_MAX_PACKET_BYTES];
                let encoded = match self.encoder.encode(&frame, &mut out) {
                    Ok(size) => size,
                    Err(err) => {
                        log::warn!("Opus encode error: {}", err);
                        break;
                    }
                };
                out.truncate(encoded);

                let timestamp_us = self
                    .next_timestamp_us
                    .unwrap_or_else(|| self.start_time.elapsed().as_micros() as u64);
                self.next_timestamp_us = Some(timestamp_us.saturating_add(self.frame_duration_us));

                let packet = EncodedFrame {
                    timestamp_us,
                    keyframe: true,
                    data: out,
                };
                let _ = self.tx.try_send(packet);
            }
        }
        #[cfg(not(feature = "opus-support"))]
        {
            // Just clear the buffer if no encoder is available
            self.pcm.clear();
        }
    }
}

#[cfg(target_os = "macos")]
struct PcmChunk {
    timestamp_us: u64,
    samples: Vec<i16>,
}

#[cfg(target_os = "macos")]
#[cfg(feature = "opus-support")]
fn create_opus_encoder() -> Result<OpusEncoder> {
    let mut encoder = OpusEncoder::new(OPUS_SAMPLE_RATE, Channels::Stereo, Application::Audio)
        .map_err(|e| anyhow!("Opus encoder init failed: {}", e))?;
    encoder
        .set_bitrate(opus::Bitrate::Bits(OPUS_BITRATE_BPS))
        .map_err(|e| anyhow!("Opus bitrate set failed: {}", e))?;
    encoder.set_complexity(5).ok();
    encoder.set_inband_fec(false).ok();
    encoder.set_dtx(false).ok();
    Ok(encoder)
}

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "WavryAudioHandler"]
    #[ivars = std::sync::Mutex<AudioContext>]
    struct AudioHandler;

    unsafe impl NSObjectProtocol for AudioHandler {}

    unsafe impl SCStreamOutput for AudioHandler {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output_sample_buffer(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Audio {
                return;
            }

            let ctx = self.ivars();
            let mut guard = match ctx.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(chunk) = build_pcm_chunk(sample_buffer, &guard.start_time) {
                guard.ingest_chunk(chunk);
            }
        }
    }
);

#[cfg(target_os = "macos")]
impl AudioHandler {
    #[cfg(feature = "opus-support")]
    fn new(tx: mpsc::Sender<EncodedFrame>, encoder: OpusEncoder) -> Retained<Self> {
        let ivars = std::sync::Mutex::new(AudioContext {
            tx,
            start_time: Instant::now(),
            encoder,
            pcm: VecDeque::with_capacity(AUDIO_MAX_BUFFER_SAMPLES),
            next_timestamp_us: None,
            frame_duration_us: opus_frame_duration_us(),
            channels: OPUS_CHANNELS,
        });
        let this = Self::alloc().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    #[cfg(not(feature = "opus-support"))]
    fn new(tx: mpsc::Sender<EncodedFrame>) -> Retained<Self> {
        let ivars = std::sync::Mutex::new(AudioContext {
            tx,
            start_time: Instant::now(),
            pcm: VecDeque::with_capacity(AUDIO_MAX_BUFFER_SAMPLES),
            next_timestamp_us: None,
            frame_duration_us: opus_frame_duration_us(),
            channels: OPUS_CHANNELS,
        });
        let this = Self::alloc().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(target_os = "macos")]
type ShareableContentSender = std::sync::Arc<
    std::sync::Mutex<Option<oneshot::Sender<Result<SendRetained<SCShareableContent>>>>>,
>;

#[cfg(target_os = "macos")]
type StreamSetupResult = (
    SendRetained<SCStream>,
    DispatchRetained<dispatch2::DispatchQueue>,
    oneshot::Receiver<Result<()>>,
);

#[cfg(target_os = "macos")]
struct SendRetained<T: objc2::Message>(pub Retained<T>);
#[cfg(target_os = "macos")]
unsafe impl<T: objc2::Message> Send for SendRetained<T> {}

#[cfg(target_os = "macos")]
fn request_shareable_content(tx: ShareableContentSender) {
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            if let Ok(mut tx_guard) = tx.lock() {
                if let Some(tx) = tx_guard.take() {
                    if !error.is_null() {
                        let _ = tx.send(Err(anyhow!("ScreenCaptureKit error")));
                    } else if !content.is_null() {
                        let content_ref = unsafe { &*content };
                        let ret = content_ref.retain();
                        let _ = tx.send(Ok(SendRetained(ret)));
                    } else {
                        let _ = tx.send(Err(anyhow!("No content provided by ScreenCaptureKit")));
                    }
                }
            }
        },
    );

    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
}

#[cfg(target_os = "macos")]
fn start_get_shareable_content(
) -> Result<oneshot::Receiver<Result<SendRetained<SCShareableContent>>>> {
    let (tx, rx) = oneshot::channel::<Result<SendRetained<SCShareableContent>>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    request_shareable_content(tx);
    Ok(rx)
}

#[cfg(target_os = "macos")]
async fn get_shareable_content() -> Result<SendRetained<SCShareableContent>> {
    let rx = start_get_shareable_content()?;
    rx.await
        .map_err(|_| anyhow!("Shareable content channel closed"))?
}

#[cfg(target_os = "macos")]
fn setup_stream(
    content: Retained<SCShareableContent>,
    output_handler: &AudioHandler,
    route: &MacAudioRoute,
) -> Result<StreamSetupResult> {
    let queue = dispatch2::DispatchQueue::new("com.wavry.audio-capture", None);
    let queue_ptr = queue.as_raw().as_ptr();
    let queue_obj = unsafe { &*(queue_ptr as *const AnyObject) };

    let displays = unsafe { content.displays() };
    if displays.count() == 0 {
        return Err(anyhow!("No displays found"));
    }
    let display = displays.objectAtIndex(0);

    let empty_windows = NSArray::new();
    let filter = match route {
        MacAudioRoute::SystemMix => unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &empty_windows,
            )
        },
        MacAudioRoute::Application(app_filter) => {
            let selected =
                find_running_app_by_name_or_bundle(&content, app_filter).ok_or_else(|| {
                    anyhow!(
                        "application audio source '{}' not found in shareable application list",
                        app_filter
                    )
                })?;
            let selected_name = unsafe { selected.applicationName() }.to_string();
            let selected_bundle = unsafe { selected.bundleIdentifier() }.to_string();
            log::info!(
                "capturing app-specific macOS audio route: {} ({})",
                selected_name,
                selected_bundle
            );
            let selected_array = NSArray::from_slice(&[&*selected]);
            unsafe {
                SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &selected_array,
                    &empty_windows,
                )
            }
        }
        MacAudioRoute::Microphone => {
            return Err(anyhow!(
                "microphone route must be initialized via microphone capture path"
            ));
        }
    };

    let stream_config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        stream_config.setCapturesAudio(true);
        stream_config.setSampleRate(OPUS_SAMPLE_RATE as isize);
        stream_config.setChannelCount(OPUS_CHANNELS as isize);
        stream_config.setExcludesCurrentProcessAudio(true);
    }

    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &stream_config,
            None,
        )
    };

    let mut err: *mut NSError = std::ptr::null_mut();
    let success: bool = unsafe {
        msg_send![
            &stream,
            addStreamOutput: output_handler,
            type: SCStreamOutputType::Audio,
            sampleHandlerQueue: queue_obj,
            error: &mut err
        ]
    };

    if !success || !err.is_null() {
        return Err(anyhow!("Failed to add audio stream output"));
    }

    let (tx_start, rx_start) = oneshot::channel();
    let tx_start = std::sync::Arc::new(std::sync::Mutex::new(Some(tx_start)));

    {
        let completion_handler = RcBlock::new(move |error: *mut NSError| {
            if let Ok(mut g) = tx_start.lock() {
                if let Some(tx) = g.take() {
                    if !error.is_null() {
                        let _ = tx.send(Err(anyhow!("Start audio capture failed")));
                    } else {
                        let _ = tx.send(Ok(()));
                    }
                }
            }
        });

        unsafe {
            stream.startCaptureWithCompletionHandler(Some(&completion_handler));
        };
    }

    Ok((SendRetained(stream), queue, rx_start))
}

#[cfg(target_os = "macos")]
fn find_running_app_by_name_or_bundle(
    content: &SCShareableContent,
    query: &str,
) -> Option<Retained<SCRunningApplication>> {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let apps = unsafe { content.applications() };
    for index in 0..apps.count() {
        let app = apps.objectAtIndex(index);
        let name = unsafe { app.applicationName() }.to_string();
        let bundle = unsafe { app.bundleIdentifier() }.to_string();
        let name_match = name.to_ascii_lowercase().contains(&normalized);
        let bundle_match = bundle.to_ascii_lowercase().contains(&normalized);
        if name_match || bundle_match {
            return Some(app.retain());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn cm_time_to_us(time: CMTime) -> u64 {
    if time.timescale == 0 {
        return 0;
    }
    let value = time.value as f64;
    let scale = time.timescale as f64;
    let seconds = value / scale;
    (seconds * 1_000_000.0) as u64
}

#[cfg(target_os = "macos")]
fn build_pcm_chunk(sample_buffer: &CMSampleBuffer, start_time: &Instant) -> Option<PcmChunk> {
    let pts = unsafe { sample_buffer.presentation_time_stamp() };
    let mut timestamp_us = cm_time_to_us(pts);
    if timestamp_us == 0 {
        timestamp_us = start_time.elapsed().as_micros() as u64;
    }

    let format_desc = unsafe { sample_buffer.format_description() }?;
    let audio_desc_ptr =
        CFRetained::as_ptr(&format_desc).as_ptr() as *const CMAudioFormatDescription;
    let asbd_ptr = unsafe {
        objc2_core_media::CMAudioFormatDescriptionGetStreamBasicDescription(&*audio_desc_ptr)
    };
    let asbd = unsafe { asbd_ptr.as_ref() }?;
    if asbd.mFormatID != kAudioFormatLinearPCM {
        return None;
    }

    let bytes_per_sample = (asbd.mBitsPerChannel / 8) as usize;
    if bytes_per_sample == 0 {
        return None;
    }

    let mut buffer_list_size: usize = 0;
    let mut block_buffer: *mut CMBlockBuffer = std::ptr::null_mut();
    let flags = kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment;

    let status = unsafe {
        sample_buffer.audio_buffer_list_with_retained_block_buffer(
            &mut buffer_list_size,
            std::ptr::null_mut(),
            0,
            None::<&CFAllocator>,
            None::<&CFAllocator>,
            flags,
            &mut block_buffer,
        )
    };
    if status != 0 || buffer_list_size == 0 {
        return None;
    }

    let mut storage = vec![0u8; buffer_list_size];
    let buffer_list_ptr = storage.as_mut_ptr() as *mut AudioBufferList;
    let status = unsafe {
        sample_buffer.audio_buffer_list_with_retained_block_buffer(
            &mut buffer_list_size,
            buffer_list_ptr,
            buffer_list_size,
            None::<&CFAllocator>,
            None::<&CFAllocator>,
            flags,
            &mut block_buffer,
        )
    };
    if status != 0 {
        return None;
    }

    let _block = unsafe { NonNull::new(block_buffer).map(|ptr| CFRetained::from_raw(ptr)) };

    let buffer_list = unsafe { &*buffer_list_ptr };
    let buffers = unsafe {
        std::slice::from_raw_parts(
            buffer_list.mBuffers.as_ptr(),
            buffer_list.mNumberBuffers as usize,
        )
    };

    let channels = asbd.mChannelsPerFrame as usize;
    if channels == 0 || buffers.is_empty() {
        return None;
    }
    if channels != OPUS_CHANNELS {
        return None;
    }

    let is_float = (asbd.mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    let is_signed = (asbd.mFormatFlags & kAudioFormatFlagIsSignedInteger) != 0;
    let non_interleaved = (asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;

    let mut out: Vec<i16> = Vec::new();
    if non_interleaved {
        if buffers.len() < channels {
            return None;
        }
        let frames = buffers[0].mDataByteSize as usize / bytes_per_sample;
        out.reserve(frames * channels);
        for frame in 0..frames {
            for buffer in buffers.iter().take(channels) {
                if let Some(sample) = read_sample_i16(
                    buffer,
                    frame,
                    bytes_per_sample,
                    is_float,
                    is_signed,
                    asbd.mFormatFlags,
                ) {
                    out.push(sample);
                }
            }
        }
    } else {
        let frames = buffers[0].mDataByteSize as usize / bytes_per_sample;
        out.reserve(frames);
        let sample_count = frames;
        for idx in 0..sample_count {
            if let Some(sample) = read_sample_i16(
                &buffers[0],
                idx,
                bytes_per_sample,
                is_float,
                is_signed,
                asbd.mFormatFlags,
            ) {
                out.push(sample);
            }
        }
    }

    Some(PcmChunk {
        timestamp_us,
        samples: out,
    })
}

#[cfg(target_os = "macos")]
fn read_sample_i16(
    buffer: &AudioBuffer,
    index: usize,
    bytes_per_sample: usize,
    is_float: bool,
    is_signed: bool,
    _flags: AudioFormatFlags,
) -> Option<i16> {
    if buffer.mData.is_null() {
        return None;
    }
    let base = buffer.mData as *const u8;
    let offset = index.checked_mul(bytes_per_sample)?;
    let ptr = unsafe { base.add(offset) };

    let sample = if is_float && bytes_per_sample == 4 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, 4) };
        let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
    } else if is_signed && bytes_per_sample == 2 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, 2) };
        i16::from_le_bytes([bytes[0], bytes[1]])
    } else if is_signed && bytes_per_sample == 4 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, 4) };
        let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        (value as f64 / i32::MAX as f64 * i16::MAX as f64) as i16
    } else {
        return None;
    };

    Some(sample)
}

#[cfg(target_os = "macos")]
fn build_audio_context(tx: mpsc::Sender<EncodedFrame>) -> Result<AudioContext> {
    #[cfg(feature = "opus-support")]
    {
        let encoder = create_opus_encoder()?;
        Ok(AudioContext {
            tx,
            start_time: Instant::now(),
            encoder,
            pcm: VecDeque::with_capacity(AUDIO_MAX_BUFFER_SAMPLES),
            next_timestamp_us: None,
            frame_duration_us: opus_frame_duration_us(),
            channels: OPUS_CHANNELS,
        })
    }

    #[cfg(not(feature = "opus-support"))]
    {
        Ok(AudioContext {
            tx,
            start_time: Instant::now(),
            pcm: VecDeque::with_capacity(AUDIO_MAX_BUFFER_SAMPLES),
            next_timestamp_us: None,
            frame_duration_us: opus_frame_duration_us(),
            channels: OPUS_CHANNELS,
        })
    }
}

#[cfg(target_os = "macos")]
fn select_input_config(device: &cpal::Device) -> Result<(StreamConfig, SampleFormat)> {
    let mut preferred: Option<(StreamConfig, SampleFormat, u16)> = None;
    let configs = device
        .supported_input_configs()
        .map_err(|e| anyhow!("audio input config query failed: {}", e))?;

    for cfg in configs {
        let channels = cfg.channels();
        if channels == 0 {
            continue;
        }
        let min = cfg.min_sample_rate().0;
        let max = cfg.max_sample_rate().0;
        if min <= OPUS_SAMPLE_RATE && max >= OPUS_SAMPLE_RATE {
            let mut config = cfg.with_sample_rate(SampleRate(OPUS_SAMPLE_RATE)).config();
            if let SupportedBufferSize::Range { min, max } = cfg.buffer_size() {
                let desired = OPUS_FRAME_SAMPLES as u32;
                config.buffer_size = BufferSize::Fixed(desired.clamp(*min, *max));
            }
            if channels == OPUS_CHANNELS as u16 {
                return Ok((config, cfg.sample_format()));
            }
            preferred = Some((config, cfg.sample_format(), channels));
        }
    }

    if let Some((config, format, channels)) = preferred {
        log::info!(
            "microphone route using {}-channel input (will map to {} channels)",
            channels,
            OPUS_CHANNELS
        );
        return Ok((config, format));
    }

    let fallback = device
        .default_input_config()
        .map_err(|e| anyhow!("default microphone config error: {}", e))?;
    Ok((fallback.config(), fallback.sample_format()))
}

#[cfg(target_os = "macos")]
fn ingest_mic_callback_frames(
    context: &Arc<Mutex<AudioContext>>,
    frames: usize,
    mut sample_for: impl FnMut(usize, usize) -> i16,
) {
    if frames == 0 {
        return;
    }

    let mut interleaved = Vec::with_capacity(frames * OPUS_CHANNELS);
    for frame in 0..frames {
        let left = sample_for(frame, 0);
        let right = if OPUS_CHANNELS > 1 {
            sample_for(frame, 1)
        } else {
            left
        };
        interleaved.push(left);
        if OPUS_CHANNELS > 1 {
            interleaved.push(right);
        }
    }

    let mut guard = match context.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let timestamp_us = guard.start_time.elapsed().as_micros() as u64;
    guard.ingest_chunk(PcmChunk {
        timestamp_us,
        samples: interleaved,
    });
}

#[cfg(target_os = "macos")]
fn f32_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(target_os = "macos")]
fn u16_to_i16(value: u16) -> i16 {
    let normalized = (value as f32 / u16::MAX as f32) * 2.0 - 1.0;
    f32_to_i16(normalized)
}

#[cfg(target_os = "macos")]
fn build_input_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    input_channels: usize,
    context: Arc<Mutex<AudioContext>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_input_stream(
        config,
        move |data: &[f32], _| {
            let frames = data.len() / input_channels.max(1);
            ingest_mic_callback_frames(&context, frames, |frame, channel| {
                let src_channel = channel.min(input_channels.saturating_sub(1));
                let idx = frame * input_channels + src_channel;
                f32_to_i16(data[idx])
            });
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

#[cfg(target_os = "macos")]
fn build_input_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    input_channels: usize,
    context: Arc<Mutex<AudioContext>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_input_stream(
        config,
        move |data: &[i16], _| {
            let frames = data.len() / input_channels.max(1);
            ingest_mic_callback_frames(&context, frames, |frame, channel| {
                let src_channel = channel.min(input_channels.saturating_sub(1));
                let idx = frame * input_channels + src_channel;
                data[idx]
            });
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

#[cfg(target_os = "macos")]
fn build_input_stream_u16(
    device: &cpal::Device,
    config: &StreamConfig,
    input_channels: usize,
    context: Arc<Mutex<AudioContext>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_input_stream(
        config,
        move |data: &[u16], _| {
            let frames = data.len() / input_channels.max(1);
            ingest_mic_callback_frames(&context, frames, |frame, channel| {
                let src_channel = channel.min(input_channels.saturating_sub(1));
                let idx = frame * input_channels + src_channel;
                u16_to_i16(data[idx])
            });
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

#[cfg(target_os = "macos")]
fn start_microphone_capture(tx: mpsc::Sender<EncodedFrame>) -> Result<Stream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("No microphone/input audio device available"))?;
    let (config, sample_format) = select_input_config(&device)?;
    let input_channels = config.channels as usize;
    if input_channels == 0 {
        return Err(anyhow!("invalid microphone channel count"));
    }

    let context = Arc::new(Mutex::new(build_audio_context(tx)?));
    let err_fn = |err| {
        log::warn!("microphone capture stream error: {}", err);
    };
    let stream = match sample_format {
        SampleFormat::F32 => {
            build_input_stream_f32(&device, &config, input_channels, context.clone(), err_fn)?
        }
        SampleFormat::I16 => {
            build_input_stream_i16(&device, &config, input_channels, context.clone(), err_fn)?
        }
        SampleFormat::U16 => {
            build_input_stream_u16(&device, &config, input_channels, context.clone(), err_fn)?
        }
        _ => return Err(anyhow!("Unsupported microphone sample format")),
    };
    stream
        .play()
        .map_err(|e| anyhow!("failed to start microphone capture stream: {}", e))?;
    Ok(stream)
}

pub struct MacAudioCapturer {
    #[cfg(target_os = "macos")]
    _stream: Option<Retained<SCStream>>,
    #[cfg(target_os = "macos")]
    _output_handler: Option<Retained<AudioHandler>>,
    #[cfg(target_os = "macos")]
    _queue: Option<DispatchRetained<dispatch2::DispatchQueue>>,
    #[cfg(target_os = "macos")]
    _cpal_stream: Option<Stream>,
    rx: mpsc::Receiver<EncodedFrame>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MacAudioCapturer {}

impl MacAudioCapturer {
    #[cfg(target_os = "macos")]
    pub async fn new() -> Result<Self> {
        Self::new_with_route(MacAudioRoute::SystemMix).await
    }

    #[cfg(target_os = "macos")]
    pub async fn new_with_route(route: MacAudioRoute) -> Result<Self> {
        let (tx, rx) = mpsc::channel(32);
        match route {
            MacAudioRoute::Microphone => {
                log::info!("starting macOS microphone capture route");
                let stream = start_microphone_capture(tx)?;
                Ok(Self {
                    _stream: None,
                    _output_handler: None,
                    _queue: None,
                    _cpal_stream: Some(stream),
                    rx,
                })
            }
            MacAudioRoute::SystemMix | MacAudioRoute::Application(_) => {
                let content = get_shareable_content().await?.0;

                #[cfg(feature = "opus-support")]
                let output_handler = {
                    let encoder = create_opus_encoder()?;
                    AudioHandler::new(tx, encoder)
                };
                #[cfg(not(feature = "opus-support"))]
                let output_handler = AudioHandler::new(tx);

                let (stream, queue, rx_start) = setup_stream(content, &output_handler, &route)?;

                rx_start
                    .await
                    .map_err(|e| anyhow!("Start audio capture canceled: {}", e))??;

                Ok(Self {
                    _stream: Some(stream.0),
                    _output_handler: Some(output_handler),
                    _queue: Some(queue),
                    _cpal_stream: None,
                    rx,
                })
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub async fn new() -> Result<Self> {
        Err(anyhow!("Not supported on this OS"))
    }

    pub fn next_packet(&mut self) -> Result<EncodedFrame> {
        self.rx
            .blocking_recv()
            .ok_or_else(|| anyhow!("audio capture stream ended"))
    }

    pub async fn next_packet_async(&mut self) -> Result<EncodedFrame> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow!("audio capture stream ended"))
    }
}
