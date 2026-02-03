use anyhow::{anyhow, Result};
use crate::{EncodeConfig, EncodedFrame};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use objc2::{
    define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, ProtocolObject},
    ClassType, DeclaredClass, AnyThread,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
#[cfg(target_os = "macos")]
use objc2_screen_capture_kit::{
    SCContentFilter, SCStream, SCStreamConfiguration, SCStreamOutput, SCStreamOutputType,
    SCShareableContent,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoCodecType};
#[cfg(target_os = "macos")]
use objc2_video_toolbox::{
    VTCompressionSession, VTCompressionOutputCallback, VTEncodeInfoFlags,
};
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFAllocator, CFDictionary};
#[cfg(target_os = "macos")]
use std::ffi::c_void;

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
                let ivars = self.ivars();
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

// ...


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

    rx: mpsc::Receiver<EncodedFrame>,
}

impl MacScreenEncoder {
    #[cfg(not(target_os = "macos"))]
    pub async fn new(_config: EncodeConfig) -> Result<Self> {
        anyhow::bail!("MacScreenEncoder only supported on macOS")
    }

    #[cfg(target_os = "macos")]
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        let (tx, rx) = mpsc::channel(16);

        // Placeholder for real logic
        
        Ok(Self {
            stream: None,
            output_handler: None, // OutputHandler::new(tx),
            rx,
        })
    }

    pub async fn next_frame(&mut self) -> Result<EncodedFrame> {
        self.rx.recv().await.ok_or_else(|| anyhow!("encoder stream closed"))
    }
}
