#![allow(dead_code, unused_variables)]
use anyhow::{anyhow, Result};
use crate::{EncodeConfig, EncodedFrame};
use tokio::sync::{mpsc, oneshot};

#[cfg(target_os = "macos")]
use objc2::{
    define_class, msg_send,
    rc::Retained, DeclaredClass, AnyThread, Message,
};
#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSObject, NSObjectProtocol, NSError, NSArray};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::{
    SCContentFilter, SCStream, SCStreamConfiguration, SCStreamOutput, SCStreamOutputType, SCShareableContent,
};
use std::ptr::NonNull;
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoCodecType};
#[cfg(target_os = "macos")]
use objc2_video_toolbox::{
    VTCompressionSession, VTEncodeInfoFlags,
};
#[cfg(target_os = "macos")]
use std::ffi::c_void;

const K_CMVIDEO_CODEC_TYPE_H264: CMVideoCodecType = 1635148593; // 'avc1'

#[cfg(target_os = "macos")]
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sbuf: &CMSampleBuffer) -> *mut c_void; // Returns CMBlockBuffer
    fn CMBlockBufferGetDataPointer(
        the_buffer: *mut c_void,
        offset: usize,
        length_at_offset_out: *mut usize,
        total_length_out: *mut usize,
        data_pointer_out: *mut *mut u8
    ) -> i32; // OSStatus
    fn CMSampleBufferGetImageBuffer(sbuf: &CMSampleBuffer) -> *mut c_void; // Returns CVImageBufferRef
    fn CMSampleBufferGetPresentationTimeStamp(sbuf: &CMSampleBuffer) -> CMTime;
    fn CMSampleBufferGetDuration(sbuf: &CMSampleBuffer) -> CMTime;
}

// Define Ivars
#[cfg(target_os = "macos")]
struct OutputHandlerIvars {
    // tx: mpsc::Sender<EncodedFrame>, // Removed: tx is passed to session callback context
    session: Retained<VTCompressionSession>,
}

// Define OutputHandler
#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "WavryOutputHandler"]
    #[ivars = OutputHandlerIvars]
    struct OutputHandler;

    
    /*
    impl OutputHandler {
    }
    */

    unsafe impl NSObjectProtocol for OutputHandler {}

    unsafe impl SCStreamOutput for OutputHandler {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output_sample_buffer(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType
        ) {
            if kind == SCStreamOutputType::Screen {
                // Feed to compression session
                let _ivars = self.ivars();
                // We need to pass presentation timestamp. Use Invalid/Indefinite if unknown but VT requires it.
                // CMSampleBuffer has it.
                // VTCompressionSession::encode_frame(session, sampleBuffer, presentationTime, duration, frameProperties, sourceFrameRefCon, infoFlagsOut)
                
                // Using method call for objc2 0.6.3 (if mapped) or manually calling function?
                // The error earlier suggested `VTCompressionSession::encode_frame`.
                // Let's assume it maps to `VTCompressionSessionEncodeFrame`.
                // Signature: (&self, image_buffer: &CVImageBuffer, presentation_time_stamp: CMTime, duration: CMTime, frame_properties: Option<&CFDictionary>, source_frame_ref_con: *mut c_void, info_flags_out: *mut VTEncodeInfoFlags) -> OSStatus
                
                // WAIT! VTCompressionSessionEncodeFrame takes IMAGE BUFFER (CVPixelBuffer), not CMSampleBuffer!
                // CaptureKit provides CMSampleBuffer.
                // We must extract CVPixelBuffer (ImageBuffer) from CMSampleBuffer.
                // CMSampleBufferGetImageBuffer(sbuf) -> CVImageBufferRef.
                
                // I need another extern function: CMSampleBufferGetImageBuffer.
            }
        }
    }
);

// Output callback
#[cfg(target_os = "macos")]
// Output callback
#[cfg(target_os = "macos")]
pub unsafe extern "C-unwind" fn compression_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32, 
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer, 
) {
    if status != 0 {
        return;
    }
    if sample_buffer.is_null() { return; }
    
    // Safety: Cast refCon to Sender
    let tx = unsafe { &*(output_callback_ref_con as *const mpsc::Sender<EncodedFrame>) };

    // Safety: Cast sample_buffer to &CMSampleBuffer
    let sbuf: &CMSampleBuffer = unsafe { &*sample_buffer };

    // Extract data
    let block_buffer = unsafe { CMSampleBufferGetDataBuffer(sbuf) };
    if block_buffer.is_null() { return; }

    let mut length: usize = 0;
    let mut data_ptr: *mut u8 = std::ptr::null_mut();
    // Safety: FFI call
    let err = unsafe { CMBlockBufferGetDataPointer(block_buffer, 0, std::ptr::null_mut(), &mut length, &mut data_ptr) };
    if err != 0 { return; }

    // Copy data
    let data = unsafe { std::slice::from_raw_parts(data_ptr, length) }.to_vec();
    
    // Timestamp (Placeholder)
    let timestamp_us = 0;
    
    // Keyframe (Placeholder)
    let keyframe = false; 

    let frame = EncodedFrame {
        timestamp_us,
        keyframe,
        data,
    };
    
    let _ = tx.blocking_send(frame);
}

#[cfg(target_os = "macos")]
async fn get_shareable_content() -> Result<Retained<SCShareableContent>> {
    let (tx, rx) = oneshot::channel::<Result<Retained<SCShareableContent>>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    
    // Block signature: (SCShareableContent *content, NSError *error) -> void
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        if let Some(mut tx_guard) = tx.lock().ok() {
            if let Some(tx) = tx_guard.take() {
                if !error.is_null() {
                     let _ = tx.send(Err(anyhow::anyhow!("SCK Error")));
                } else if !content.is_null() {
                     let content_ref = unsafe { &*content };
                     let ret = content_ref.retain();
                     let _ = tx.send(Ok(ret));
                } else {
                     let _ = tx.send(Err(anyhow::anyhow!("No content provided by SCK")));
                }
            }
        }
    });

    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };

    rx.await.map_err(|e| anyhow::anyhow!("SCK CanceledOrFailed: {}", e))?
}

#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
fn create_compression_session(
    width: i32,
    height: i32,
    tx: mpsc::Sender<EncodedFrame>,
) -> Result<(Retained<VTCompressionSession>, *mut c_void)> {
    let tx_ptr = Box::into_raw(Box::new(tx)) as *mut c_void;
    
    // Create out pointer
    let mut session_ptr: *mut VTCompressionSession = std::ptr::null_mut();
    
    let status = unsafe { VTCompressionSession::create(
        None, // allocator
        width,
        height,
        K_CMVIDEO_CODEC_TYPE_H264, 
        None, // encoderSpecification
        None, // sourceImageBufferAttributes
        None, // compressedDataAllocator
        Some(compression_callback), // outputCallback
        tx_ptr, // outputCallbackRefCon
        NonNull::new(&mut session_ptr).unwrap() // out
    ) };
    
    if status == 0 {
        // Correctly created. Wrap in Retained.
        // `create` follows Create Rule (+1 ref). `Retained::from_raw` logic:
        // If from_raw assumes +1, it's good.
        // Actually RefCounted types in objc2 use from_raw for ownership transfer?
        // Let's assume Retained::retain is safer if we are unsure, but `create` returns ownership.
        // So unsafe { Retained::from_raw(session_ptr) } is semantically correct for CFCreate.
        let session = unsafe { Retained::from_raw(session_ptr) }
            .ok_or_else(|| anyhow!("Created session is null"))?;
        Ok((session, tx_ptr))
    } else {
        // Drop tx
        unsafe { drop(Box::from_raw(tx_ptr as *mut mpsc::Sender<EncodedFrame>)) };
        Err(anyhow!("Failed to create compression session: status {}", status))
    }
}


#[cfg(target_os = "macos")]
impl OutputHandler {
    fn new(session: Retained<VTCompressionSession>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputHandlerIvars { session });
        // this is PartialInit<Self>. We need validation.
        // We call [super init] to finalize initialization.
        // msg_send![super(this), init]
        unsafe { msg_send![super(this), init] }
    }
}

pub struct MacScreenEncoder {
    #[cfg(target_os = "macos")]
    stream: Option<Retained<SCStream>>,
    
    #[cfg(target_os = "macos")]
    output_handler: Option<Retained<OutputHandler>>, 
    
    #[cfg(target_os = "macos")]
    session: Option<Retained<VTCompressionSession>>,
    
    #[cfg(target_os = "macos")]
    callback_context: *mut c_void,

    rx: mpsc::Receiver<EncodedFrame>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MacScreenEncoder {}

#[cfg(target_os = "macos")]
impl Drop for MacScreenEncoder {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            unsafe { session.invalidate() };
        }
        if !self.callback_context.is_null() {
            unsafe { drop(Box::from_raw(self.callback_context as *mut mpsc::Sender<EncodedFrame>)) };
        }
    }
}

impl MacScreenEncoder {
    #[cfg(not(target_os = "macos"))]
    pub async fn new(_config: EncodeConfig) -> Result<Self> {
        anyhow::bail!("MacScreenEncoder only supported on macOS")
    }

    #[cfg(target_os = "macos")]
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        let (tx, rx) = mpsc::channel(16);

        let width = config.resolution.width as i32;
        let height = config.resolution.height as i32;

        let (session, callback_context) = create_compression_session(width, height, tx)?;
        
        let output_handler = OutputHandler::new(session.clone());

#[cfg(target_os = "macos")]
use dispatch2::DispatchObject;

        let content = get_shareable_content().await?;
        let displays = unsafe { content.displays() };
        let count = displays.count();
        if count == 0 {
             return Err(anyhow!("No displays found"));
        }
        let display = displays.objectAtIndex(0);

        let filter = unsafe { SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(), 
            &display, 
            &NSArray::new()
        ) };

        let stream_config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            stream_config.setWidth(width as usize);
            stream_config.setHeight(height as usize);
            stream_config.setPixelFormat(1111970369); 
            stream_config.setShowsCursor(true);
        }

        let stream = unsafe { SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(), 
            &filter, 
            &stream_config, 
            None
        ) };
        
        let queue = dispatch2::Queue::new("com.wavry.screen-encoder", None);
        let queue_ptr = queue.as_raw().as_ptr(); 
        // Cast to AnyObject for msg_send
        let queue_obj = unsafe { &*(queue_ptr as *const objc2::runtime::AnyObject) };
        
        // Manual msg_send to avoid OS_dispatch_queue trait import issues
        // - (BOOL)addStreamOutput:(id<SCStreamOutput>)output type:(SCStreamOutputType)type sampleHandlerQueue:(dispatch_queue_t)sampleHandlerQueue error:(NSError **)error;
        let mut err: *mut NSError = std::ptr::null_mut();
        let success: bool = unsafe {
             msg_send![
                 &stream,
                 addStreamOutput: &*output_handler, 
                 type: SCStreamOutputType::Screen, 
                 sampleHandlerQueue: queue_obj,
                 error: &mut err
             ]
        };
        
        if !success || !err.is_null() {
             return Err(anyhow!("Failed to add stream output (objc error)"));
        }
        
        let (tx_start, rx_start) = oneshot::channel();
        let tx_start = std::sync::Arc::new(std::sync::Mutex::new(Some(tx_start)));

        let completion_handler = RcBlock::new(move |error: *mut NSError| {
             if let Some(mut g) = tx_start.lock().ok() {
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
        
        rx_start.await.map_err(|e| anyhow!("Start capture canceled: {}", e))??;

        Ok(Self {
            stream: Some(stream),
            output_handler: Some(output_handler),
            session: Some(session),
            callback_context,
            rx,
        })
    }

    pub async fn next_frame(&mut self) -> Result<EncodedFrame> {
        self.rx.recv().await.ok_or_else(|| anyhow!("encoder stream closed"))
    }
}
