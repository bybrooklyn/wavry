#![allow(
    dead_code,
    unused_variables,
    deprecated,
    clippy::arc_with_non_send_sync
)]
use crate::{Codec, EncodeConfig, EncodedFrame};
use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, oneshot};

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use dispatch2::{DispatchObject, DispatchRetained};
#[cfg(target_os = "macos")]
use libloading::Library;
#[cfg(target_os = "macos")]
use objc2::{define_class, msg_send, rc::Retained, AnyThread, DeclaredClass, Message};
#[cfg(target_os = "macos")]
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoCodecType};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
    SCStreamOutputType,
};
#[cfg(target_os = "macos")]
use objc2_video_toolbox::{VTCompressionSession, VTEncodeInfoFlags};
#[cfg(target_os = "macos")]
use std::ffi::c_void;
#[cfg(target_os = "macos")]
use std::ptr::NonNull;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

// Codec types
#[cfg(target_os = "macos")]
const K_CMVIDEO_CODEC_TYPE_H264: CMVideoCodecType = 0x61766331; // 'avc1'
#[cfg(target_os = "macos")]
const K_CMVIDEO_CODEC_TYPE_HEVC: CMVideoCodecType = 0x68766331; // 'hvc1'
#[cfg(target_os = "macos")]
const K_CMVIDEO_CODEC_TYPE_AV1: CMVideoCodecType = 0x61763031; // 'av01'

// 10-bit and HDR Constants
#[cfg(target_os = "macos")]
const K_CVPIXEL_FORMAT_TYPE_420Y_PB_CR10_BI_PLANAR_VIDEO_RANGE: u32 = 0x78343230; // 'x420'
#[cfg(target_os = "macos")]
const K_CVPIXEL_FORMAT_TYPE_420Y_PB_CR10_BI_PLANAR_FULL_RANGE: u32 = 0x66343230; // 'f420'

// Property keys for VTCompressionSession
#[cfg(target_os = "macos")]
#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    static kVTCompressionPropertyKey_RealTime: *const c_void;
    static kVTCompressionPropertyKey_ProfileLevel: *const c_void;
    static kVTCompressionPropertyKey_AllowFrameReordering: *const c_void;
    static kVTCompressionPropertyKey_AverageBitRate: *const c_void;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: *const c_void;
    static kVTCompressionPropertyKey_ExpectedFrameRate: *const c_void;
    static kVTCompressionPropertyKey_DataRateLimits: *const c_void;
    static kVTCompressionPropertyKey_MaximizePowerEfficiency: *const c_void;
    static kVTCompressionPropertyKey_H264EntropyMode: *const c_void;
    static kVTCompressionPropertyKey_ColorPrimaries: *const c_void;
    static kVTCompressionPropertyKey_TransferFunction: *const c_void;
    static kVTCompressionPropertyKey_YCbCrMatrix: *const c_void;

    // Profiles
    static kVTProfileLevel_HEVC_Main10_AutoLevel: *const c_void;

    fn VTSessionSetProperty(session: *mut c_void, key: *const c_void, value: *const c_void) -> i32;
    fn VTCompressionSessionPrepareToEncodeFrames(session: *mut c_void) -> i32;
    fn VTCompressionSessionEncodeFrame(
        session: *mut c_void,
        image_buffer: *mut c_void,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: *const c_void,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> i32;
    fn VTCompressionSessionInvalidate(session: *mut c_void);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGDisplayCopyColorSpace(display: u32) -> *const c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVColorSpaceGetPrimaries(color_space: *const c_void) -> *const c_void;
    fn CVColorSpaceGetTransferFunction(color_space: *const c_void) -> *const c_void;
    fn CVColorSpaceGetYCbCrMatrix(color_space: *const c_void) -> *const c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sbuf: *const CMSampleBuffer) -> *mut c_void;
    fn CMBlockBufferGetDataPointer(
        the_buffer: *mut c_void,
        offset: usize,
        length_at_offset_out: *mut usize,
        total_length_out: *mut usize,
        data_pointer_out: *mut *mut u8,
    ) -> i32;
    fn CMSampleBufferGetImageBuffer(sbuf: *const CMSampleBuffer) -> *mut c_void;
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: *const CMSampleBuffer) -> CMTime;
    fn CMSampleBufferGetDuration(sbuf: *const CMSampleBuffer) -> CMTime;
    fn CMTimeGetSeconds(time: CMTime) -> f64;
    fn CMSampleBufferGetSampleAttachmentsArray(
        sbuf: *const CMSampleBuffer,
        create: bool,
    ) -> *const c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: *const c_void;
    static kCFBooleanFalse: *const c_void;
    fn CFNumberCreate(
        allocator: *const c_void,
        the_type: i64,
        value_ptr: *const c_void,
    ) -> *const c_void;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        c_string: *const u8,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFBooleanGetValue(boolean: *const c_void) -> bool;

    // Dictionary keys for sample buffer attachments
    static kCMSampleAttachmentKey_NotSync: *const c_void;
    static kCMSampleAttachmentKey_DependsOnOthers: *const c_void;
}

// CFNumber types
#[cfg(target_os = "macos")]
const K_CFNUMBER_INT32_TYPE: i64 = 3;
#[cfg(target_os = "macos")]
const K_CFNUMBER_FLOAT64_TYPE: i64 = 13;

// Shared context for encoding
#[cfg(target_os = "macos")]
struct EncoderContext {
    tx: mpsc::Sender<EncodedFrame>,
    start_time: std::time::Instant,
    frame_count: AtomicU64,
}

// Define Ivars for SCStreamOutput handler
#[cfg(target_os = "macos")]
struct OutputHandlerIvars {
    session_ptr: *mut c_void,
    start_time: std::time::Instant,
}

#[cfg(target_os = "macos")]
unsafe impl Send for OutputHandlerIvars {}
#[cfg(target_os = "macos")]
unsafe impl Sync for OutputHandlerIvars {}

// Define OutputHandler
#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "WavryOutputHandler"]
    #[ivars = OutputHandlerIvars]
    struct OutputHandler;

    unsafe impl NSObjectProtocol for OutputHandler {}

    unsafe impl SCStreamOutput for OutputHandler {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output_sample_buffer(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Screen {
                return;
            }

            let ivars = self.ivars();
            let session_ptr = ivars.session_ptr;
            if session_ptr.is_null() {
                return;
            }

            // Extract CVPixelBuffer from CMSampleBuffer
            let pixel_buffer = unsafe { CMSampleBufferGetImageBuffer(sample_buffer as *const _) };
            if pixel_buffer.is_null() {
                return;
            }

            // Get presentation timestamp
            let pts = unsafe { CMSampleBufferGetPresentationTimeStamp(sample_buffer as *const _) };
            let duration = unsafe { CMSampleBufferGetDuration(sample_buffer as *const _) };

            // Encode the frame
            let mut info_flags: u32 = 0;
            let status = unsafe {
                VTCompressionSessionEncodeFrame(
                    session_ptr,
                    pixel_buffer,
                    pts,
                    duration,
                    std::ptr::null(), // frame properties (nil = use session defaults)
                    std::ptr::null_mut(), // source frame ref con
                    &mut info_flags,
                )
            };

            if status != 0 {
                log::warn!("VTCompressionSessionEncodeFrame failed: {}", status);
            }
        }
    }
);

// Output callback for compressed frames
#[cfg(target_os = "macos")]
pub unsafe extern "C-unwind" fn compression_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer,
) {
    if status != 0 {
        log::warn!("Compression callback error: {}", status);
        return;
    }
    if sample_buffer.is_null() {
        return;
    }

    // Cast refCon to our context
    let ctx = unsafe { &*(output_callback_ref_con as *const EncoderContext) };
    let sbuf = sample_buffer as *const CMSampleBuffer;

    // Extract compressed data from CMSampleBuffer
    let block_buffer = unsafe { CMSampleBufferGetDataBuffer(sbuf) };
    if block_buffer.is_null() {
        return;
    }

    let mut length: usize = 0;
    let mut data_ptr: *mut u8 = std::ptr::null_mut();
    let err = unsafe {
        CMBlockBufferGetDataPointer(
            block_buffer,
            0,
            std::ptr::null_mut(),
            &mut length,
            &mut data_ptr,
        )
    };
    if err != 0 || data_ptr.is_null() {
        return;
    }

    // Copy the encoded data
    let data = unsafe { std::slice::from_raw_parts(data_ptr, length) }.to_vec();

    // Get timestamp in microseconds
    let pts = unsafe { CMSampleBufferGetPresentationTimeStamp(sbuf) };
    let timestamp_seconds = unsafe { CMTimeGetSeconds(pts) };
    let timestamp_us = if timestamp_seconds.is_finite() {
        (timestamp_seconds * 1_000_000.0) as u64
    } else {
        ctx.start_time.elapsed().as_micros() as u64
    };

    // Determine if this is a keyframe by checking sample attachments
    let keyframe = unsafe {
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sbuf, false);
        if attachments.is_null() || CFArrayGetCount(attachments) == 0 {
            // No attachments = assume keyframe (first frame)
            true
        } else {
            let dict = CFArrayGetValueAtIndex(attachments, 0);
            if dict.is_null() {
                true
            } else {
                // kCMSampleAttachmentKey_NotSync = true means NOT a keyframe
                let not_sync = CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync);
                if not_sync.is_null() {
                    true // Not present = keyframe
                } else {
                    !CFBooleanGetValue(not_sync) // Inverse: NotSync=false means keyframe
                }
            }
        }
    };

    let frame = EncodedFrame {
        timestamp_us,
        keyframe,
        data,
    };

    // Send frame (non-blocking)
    let _ = ctx.tx.try_send(frame);
    ctx.frame_count.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
type ShareableContentSender = std::sync::Arc<
    std::sync::Mutex<Option<oneshot::Sender<Result<SendRetained<SCShareableContent>>>>>,
>;

#[cfg(target_os = "macos")]
type StreamSetupResult = (
    SendRetained<SCStream>,
    SendPtr<dispatch2::DispatchRetained<dispatch2::DispatchQueue>>,
    oneshot::Receiver<Result<()>>,
);

#[cfg(target_os = "macos")]
fn request_shareable_content(tx: ShareableContentSender) {
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            if let Ok(mut tx_guard) = tx.lock() {
                if let Some(tx) = tx_guard.take() {
                    if !error.is_null() {
                        let _ = tx.send(Err(anyhow::anyhow!("ScreenCaptureKit error")));
                    } else if !content.is_null() {
                        let content_ref = unsafe { &*content };
                        let ret = content_ref.retain();
                        let _ = tx.send(Ok(SendRetained(ret)));
                    } else {
                        let _ = tx.send(Err(anyhow::anyhow!(
                            "No content provided by ScreenCaptureKit"
                        )));
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

    // Call synchronous helper to keep RcBlock off the async stack
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
fn cm_codec_type(codec: Codec) -> CMVideoCodecType {
    match codec {
        Codec::Av1 => K_CMVIDEO_CODEC_TYPE_AV1,
        Codec::Hevc => K_CMVIDEO_CODEC_TYPE_HEVC,
        Codec::H264 => K_CMVIDEO_CODEC_TYPE_H264,
    }
}

#[cfg(target_os = "macos")]
fn hardware_encode_supported(codec: Codec) -> bool {
    type VTHardwareEncodeFn = unsafe extern "C" fn(CMVideoCodecType) -> u8;

    let Ok(lib) =
        (unsafe { Library::new("/System/Library/Frameworks/VideoToolbox.framework/VideoToolbox") })
    else {
        return false;
    };
    let Ok(func) = (unsafe { lib.get::<VTHardwareEncodeFn>(b"VTIsHardwareEncodeSupported\0") })
    else {
        return false;
    };

    unsafe { func(cm_codec_type(codec)) != 0 }
}

#[cfg(target_os = "macos")]
fn create_compression_session(
    config: EncodeConfig,
    tx: mpsc::Sender<EncodedFrame>,
) -> Result<(*mut c_void, Box<EncoderContext>)> {
    // Create context
    let ctx = Box::new(EncoderContext {
        tx,
        start_time: std::time::Instant::now(),
        frame_count: AtomicU64::new(0),
    });
    let ctx_ptr = Box::into_raw(ctx);

    // Create compression session
    let mut session_ptr: *mut VTCompressionSession = std::ptr::null_mut();

    let status = unsafe {
        VTCompressionSession::create(
            None, // allocator
            config.resolution.width as i32,
            config.resolution.height as i32,
            cm_codec_type(config.codec),
            None, // encoderSpecification
            None, // sourceImageBufferAttributes
            None, // compressedDataAllocator
            Some(compression_callback),
            ctx_ptr as *mut c_void,
            NonNull::new(&mut session_ptr).unwrap(),
        )
    };

    if status != 0 || session_ptr.is_null() {
        // Clean up context
        unsafe { drop(Box::from_raw(ctx_ptr)) };
        return Err(anyhow!(
            "Failed to create VTCompressionSession: status {}",
            status
        ));
    }

    // Configure session properties
    unsafe {
        let session = session_ptr as *mut c_void;

        // Real-time encoding
        VTSessionSetProperty(session, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);

        // Disable B-frames for low latency
        VTSessionSetProperty(
            session,
            kVTCompressionPropertyKey_AllowFrameReordering,
            kCFBooleanFalse,
        );

        // Set bitrate (in bits per second)
        let bitrate = (config.bitrate_kbps * 1000) as i32;
        let bitrate_num = CFNumberCreate(
            std::ptr::null(),
            K_CFNUMBER_INT32_TYPE,
            &bitrate as *const _ as *const c_void,
        );
        if !bitrate_num.is_null() {
            VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_AverageBitRate,
                bitrate_num,
            );
            CFRelease(bitrate_num);
        }

        // Set expected frame rate
        let fps_f64 = config.fps as f64;
        let fps_num = CFNumberCreate(
            std::ptr::null(),
            K_CFNUMBER_FLOAT64_TYPE,
            &fps_f64 as *const _ as *const c_void,
        );
        if !fps_num.is_null() {
            VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_ExpectedFrameRate,
                fps_num,
            );
            CFRelease(fps_num);
        }

        // Set keyframe interval (in frames)
        let keyframe_frames = ((config.keyframe_interval_ms as f64 / 1000.0) * config.fps as f64) as i32;
        let keyframe_num = CFNumberCreate(
            std::ptr::null(),
            K_CFNUMBER_INT32_TYPE,
            &keyframe_frames as *const _ as *const c_void,
        );
        if !keyframe_num.is_null() {
            VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                keyframe_num,
            );
            CFRelease(keyframe_num);
        }

        // 10-bit and HDR support
        if config.enable_10bit && config.codec == Codec::Hevc {
            VTSessionSetProperty(
                session,
                kVTCompressionPropertyKey_ProfileLevel,
                kVTProfileLevel_HEVC_Main10_AutoLevel,
            );
            // AV1 10-bit is often implicit in AutoLevel or requires specific profile keys
            // which vary by macOS version.
        }

        // Prioritize performance over battery
        VTSessionSetProperty(
            session,
            kVTCompressionPropertyKey_MaximizePowerEfficiency,
            kCFBooleanFalse,
        );

        // H.264-only entropy setting for better quality/efficiency at the same bitrate.
        if config.codec == Codec::H264 {
            let cabac = b"CABAC\0";
            let cabac_str = CFStringCreateWithCString(std::ptr::null(), cabac.as_ptr(), 0x08000100); // kCFStringEncodingUTF8
            if !cabac_str.is_null() {
                VTSessionSetProperty(
                    session,
                    kVTCompressionPropertyKey_H264EntropyMode,
                    cabac_str,
                );
                CFRelease(cabac_str);
            }
        }

        // Prepare for encoding
        VTCompressionSessionPrepareToEncodeFrames(session);
    }

    // Reconstruct context Box for returning ownership
    let ctx = unsafe { Box::from_raw(ctx_ptr) };

    Ok((session_ptr as *mut c_void, ctx))
}

#[cfg(target_os = "macos")]
impl OutputHandler {
    fn new(session_ptr: *mut c_void) -> Retained<Self> {
        let ivars = OutputHandlerIvars {
            session_ptr,
            start_time: std::time::Instant::now(),
        };
        let this = Self::alloc().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct MacScreenEncoder {
    #[cfg(target_os = "macos")]
    stream: Option<Retained<SCStream>>,

    #[cfg(target_os = "macos")]
    output_handler: Option<Retained<OutputHandler>>,

    #[cfg(target_os = "macos")]
    session_ptr: *mut c_void,

    #[cfg(target_os = "macos")]
    encoder_context: Option<Box<EncoderContext>>,

    #[cfg(target_os = "macos")]
    _queue: DispatchRetained<dispatch2::DispatchQueue>,

    rx: mpsc::Receiver<EncodedFrame>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MacScreenEncoder {}
#[cfg(target_os = "macos")]
unsafe impl Sync for MacScreenEncoder {}

#[cfg(target_os = "macos")]
// Safe wrappers for sending across threads (needed for async await state machine)
struct SendPtr<T>(pub T);
unsafe impl<T> Send for SendPtr<T> {}

struct SendRetained<T: Message>(pub Retained<T>);
unsafe impl<T: Message> Send for SendRetained<T> {}

impl Drop for MacScreenEncoder {
    fn drop(&mut self) {
        // Stop stream
        unsafe {
            if let Some(stream) = &self.stream {
                let (tx, rx) = std::sync::mpsc::channel();
                let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

                let completion = RcBlock::new(move |error: *mut NSError| {
                    if let Ok(mut g) = tx.lock() {
                        if let Some(tx) = g.take() {
                            let _ = tx.send(());
                        }
                    }
                });

                stream.stopCaptureWithCompletionHandler(Some(&completion));
                let _ = rx.recv_timeout(std::time::Duration::from_millis(1000));
            }
        }

        // Invalidate compression session
        if !self.session_ptr.is_null() {
            unsafe {
                VTCompressionSessionInvalidate(self.session_ptr);
                CFRelease(self.session_ptr);
            }
        }

        // Drop encoder context (cleans up channel sender)
        self.encoder_context.take();
    }
}

impl MacScreenEncoder {
    #[cfg(not(target_os = "macos"))]
    pub async fn new(_config: EncodeConfig) -> Result<Self> {
        anyhow::bail!("MacScreenEncoder only supported on macOS")
    }

    #[cfg(target_os = "macos")]
    fn setup_stream(
        width: i32,
        height: i32,
        config: EncodeConfig,
        content: Retained<SCShareableContent>,
        output_handler: &OutputHandler,
        session_ptr: *mut c_void,
    ) -> Result<StreamSetupResult> {
        // Create dispatch queue
        let queue = dispatch2::DispatchQueue::new("com.wavry.screen-encoder", None);
        let queue_ptr = queue.as_raw().as_ptr();
        let queue_obj = unsafe { &*(queue_ptr as *const objc2::runtime::AnyObject) };

        let displays = unsafe { content.displays() };
        if displays.count() == 0 {
            return Err(anyhow!("No displays found"));
        }

        let display = if let Some(id) = config.display_id {
            let mut found = None;
            for i in 0..displays.count() {
                let d = displays.objectAtIndex(i);
                if unsafe { d.displayID() } == id {
                    found = Some(d);
                    break;
                }
            }
            found.ok_or_else(|| anyhow!("Display {} not found", id))?
        } else {
            displays.objectAtIndex(0)
        };

        let display_id = unsafe { display.displayID() };

        // Handle color space and HDR
        if !session_ptr.is_null() {
            unsafe {
                let color_space = CGDisplayCopyColorSpace(display_id);
                if !color_space.is_null() {
                    let primaries = CVColorSpaceGetPrimaries(color_space);
                    let transfer = CVColorSpaceGetTransferFunction(color_space);
                    let matrix = CVColorSpaceGetYCbCrMatrix(color_space);

                    if !primaries.is_null() {
                        VTSessionSetProperty(
                            session_ptr,
                            kVTCompressionPropertyKey_ColorPrimaries,
                            primaries,
                        );
                    }
                    if !transfer.is_null() {
                        VTSessionSetProperty(
                            session_ptr,
                            kVTCompressionPropertyKey_TransferFunction,
                            transfer,
                        );
                    }
                    if !matrix.is_null() {
                        VTSessionSetProperty(
                            session_ptr,
                            kVTCompressionPropertyKey_YCbCrMatrix,
                            matrix,
                        );
                    }
                    CFRelease(color_space);
                }
            }
        }

        // Create content filter
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            )
        };

        // Configure stream
        let stream_config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            stream_config.setWidth(width as usize);
            stream_config.setHeight(height as usize);

            // Use 10-bit pixel format if requested
            if config.enable_10bit {
                stream_config.setPixelFormat(K_CVPIXEL_FORMAT_TYPE_420Y_PB_CR10_BI_PLANAR_VIDEO_RANGE);
            } else {
                stream_config.setPixelFormat(0x42475241); // 'BGRA'
            }

            stream_config.setShowsCursor(true);
            stream_config.setMinimumFrameInterval(CMTime {
                value: 1,
                timescale: config.fps as i32,
                flags: objc2_core_media::CMTimeFlags(1),
                epoch: 0,
            });
        }

        // Create stream
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
                type: SCStreamOutputType::Screen,
                sampleHandlerQueue: queue_obj,
                error: &mut err
            ]
        };

        if !success || !err.is_null() {
            return Err(anyhow!("Failed to add stream output"));
        }

        // Start capture
        let (tx_start, rx_start) = oneshot::channel();
        let tx_start = std::sync::Arc::new(std::sync::Mutex::new(Some(tx_start)));

        {
            let completion_handler = RcBlock::new(move |error: *mut NSError| {
                if let Ok(mut g) = tx_start.lock() {
                    if let Some(tx) = g.take() {
                        if !error.is_null() {
                            let _ = tx.send(Err(anyhow!("Start capture failed")));
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

        Ok((SendRetained(stream), SendPtr(queue), rx_start))
    }

    #[cfg(target_os = "macos")]
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        let (tx, rx) = mpsc::channel(32);

        // 1. Get content (Async)
        let content = get_shareable_content().await?.0;

        // 2. Create compression session
        let (session_ptr, encoder_context) = create_compression_session(config, tx)?;
        let send_session_ptr = SendPtr(session_ptr);

        let output_handler = OutputHandler::new(send_session_ptr.0);
        let send_output_handler = SendRetained(output_handler);

        // 3. Setup Stream
        let (send_stream, send_queue, rx_start) = Self::setup_stream(
            config.resolution.width as i32,
            config.resolution.height as i32,
            config,
            content,
            &send_output_handler.0,
            send_session_ptr.0,
        )?;

        // 4. Await start
        rx_start
            .await
            .map_err(|e| anyhow!("Start capture canceled: {}", e))??;

        let stream = send_stream.0;
        let queue = send_queue.0;
        let output_handler = send_output_handler.0;
        let session_ptr = send_session_ptr.0;

        log::info!(
            "MacScreenEncoder started: {:?}, {}x{} @ {}fps, {}kbps, 10bit: {}",
            config.codec,
            config.resolution.width,
            config.resolution.height,
            config.fps,
            config.bitrate_kbps,
            config.enable_10bit
        );

        Ok(Self {
            stream: Some(stream),
            output_handler: Some(output_handler),
            session_ptr,
            encoder_context: Some(encoder_context),
            _queue: queue,
            rx,
        })
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        self.rx
            .blocking_recv()
            .ok_or_else(|| anyhow!("encoder stream closed"))
    }

    pub async fn next_frame_async(&mut self) -> Result<EncodedFrame> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow!("encoder stream closed"))
    }

    #[cfg(target_os = "macos")]
    pub fn frame_count(&self) -> u64 {
        self.encoder_context
            .as_ref()
            .map(|ctx| ctx.frame_count.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn frame_count(&self) -> u64 {
        0
    }

    /// Update the encoder bitrate at runtime.
    /// VideoToolbox supports dynamic bitrate changes without recreating the session.
    #[cfg(target_os = "macos")]
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        if self.session_ptr.is_null() {
            return Err(anyhow!("Compression session is null"));
        }

        unsafe {
            let bitrate = (bitrate_kbps * 1000) as i32;
            let bitrate_num = CFNumberCreate(
                std::ptr::null(),
                K_CFNUMBER_INT32_TYPE,
                &bitrate as *const _ as *const c_void,
            );
            if bitrate_num.is_null() {
                return Err(anyhow!("Failed to create CFNumber for bitrate"));
            }

            let status = VTSessionSetProperty(
                self.session_ptr,
                kVTCompressionPropertyKey_AverageBitRate,
                bitrate_num,
            );
            CFRelease(bitrate_num);

            if status != 0 {
                return Err(anyhow!(
                    "VTSessionSetProperty failed with status: {}",
                    status
                ));
            }
        }

        log::debug!("Encoder bitrate updated to {} kbps", bitrate_kbps);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn set_bitrate(&mut self, _bitrate_kbps: u32) -> Result<()> {
        Err(anyhow!("set_bitrate only supported on macOS"))
    }
}

pub struct MacProbe;

impl crate::CapabilityProbe for MacProbe {
    fn supported_encoders(&self) -> Result<Vec<crate::Codec>> {
        Ok(self
            .encoder_capabilities()?
            .into_iter()
            .map(|cap| cap.codec)
            .collect())
    }

    fn supported_decoders(&self) -> Result<Vec<crate::Codec>> {
        Ok(vec![crate::Codec::Hevc, crate::Codec::H264])
    }

    fn encoder_capabilities(&self) -> Result<Vec<crate::VideoCodecCapability>> {
        let mut caps = Vec::new();

        for codec in [crate::Codec::Av1, crate::Codec::Hevc, crate::Codec::H264] {
            let hardware_accelerated = hardware_encode_supported(codec);
            let available = hardware_accelerated || codec != crate::Codec::Av1;
            if !available {
                continue;
            }
            let supports_hdr10 =
                hardware_accelerated && matches!(codec, crate::Codec::Av1 | crate::Codec::Hevc);
            caps.push(crate::VideoCodecCapability {
                codec,
                hardware_accelerated,
                supports_10bit: supports_hdr10,
                supports_hdr10,
            });
        }

        if caps.is_empty() {
            caps.push(crate::VideoCodecCapability::sdr(crate::Codec::H264, false));
        }

        Ok(caps)
    }

    fn enumerate_displays(&self) -> Result<Vec<crate::DisplayInfo>> {
        #[cfg(target_os = "macos")]
        unsafe {
            let mut displays_ids = [0u32; 16];
            let mut count = 0;
            extern "C" {
                fn CGGetActiveDisplayList(
                    max_displays: u32,
                    active_displays: *mut u32,
                    display_count: *mut u32,
                ) -> i32;
                fn CGDisplayPixelsWide(display: u32) -> usize;
                fn CGDisplayPixelsHigh(display: u32) -> usize;
            }

            if CGGetActiveDisplayList(16, displays_ids.as_mut_ptr(), &mut count) != 0 {
                return Err(anyhow!("CGGetActiveDisplayList failed"));
            }

            let mut info = Vec::new();
            for id in displays_ids.iter().take(count as usize).copied() {
                info.push(crate::DisplayInfo {
                    id,
                    name: format!("Display {}", id),
                    resolution: crate::Resolution {
                        width: CGDisplayPixelsWide(id) as u16,
                        height: CGDisplayPixelsHigh(id) as u16,
                    },
                });
            }
            Ok(info)
        }
        #[cfg(not(target_os = "macos"))]
        Ok(vec![])
    }
}
