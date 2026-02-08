use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use block2::RcBlock;
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
use objc2_foundation::{NSError, NSObject, NSObjectProtocol};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType,
};
use std::collections::VecDeque;
use std::ptr::NonNull;
#[cfg(target_os = "macos")]
use std::time::Instant;
#[cfg(target_os = "macos")]
use tokio::sync::oneshot;

use crate::audio::{
    opus_frame_duration_us, AUDIO_MAX_BUFFER_SAMPLES, OPUS_BITRATE_BPS, OPUS_CHANNELS,
    OPUS_FRAME_SAMPLES, OPUS_MAX_PACKET_BYTES, OPUS_SAMPLE_RATE,
};
use crate::EncodedFrame;

#[cfg(target_os = "macos")]
use opus::{Application, Channels, Encoder as OpusEncoder};

#[cfg(target_os = "macos")]
struct AudioContext {
    tx: mpsc::Sender<EncodedFrame>,
    start_time: Instant,
    encoder: OpusEncoder,
    pcm: VecDeque<i16>,
    next_timestamp_us: Option<u64>,
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
}

#[cfg(target_os = "macos")]
struct PcmChunk {
    timestamp_us: u64,
    samples: Vec<i16>,
}

#[cfg(target_os = "macos")]
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
) -> Result<StreamSetupResult> {
    let queue = dispatch2::DispatchQueue::new("com.wavry.audio-capture", None);
    let queue_ptr = queue.as_raw().as_ptr();
    let queue_obj = unsafe { &*(queue_ptr as *const AnyObject) };

    let displays = unsafe { content.displays() };
    if displays.count() == 0 {
        return Err(anyhow!("No displays found"));
    }
    let display = displays.objectAtIndex(0);

    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &objc2_foundation::NSArray::new(),
        )
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

pub struct MacAudioCapturer {
    #[cfg(target_os = "macos")]
    _stream: Option<Retained<SCStream>>,
    #[cfg(target_os = "macos")]
    _output_handler: Option<Retained<AudioHandler>>,
    #[cfg(target_os = "macos")]
    _queue: DispatchRetained<dispatch2::DispatchQueue>,
    rx: mpsc::Receiver<EncodedFrame>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MacAudioCapturer {}

impl MacAudioCapturer {
    #[cfg(target_os = "macos")]
    pub async fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(32);
        let content = get_shareable_content().await?.0;
        let encoder = create_opus_encoder()?;
        let output_handler = AudioHandler::new(tx, encoder);
        let (stream, queue, rx_start) = setup_stream(content, &output_handler)?;

        rx_start
            .await
            .map_err(|e| anyhow!("Start audio capture canceled: {}", e))??;

        Ok(Self {
            _stream: Some(stream.0),
            _output_handler: Some(output_handler),
            _queue: queue,
            rx,
        })
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
