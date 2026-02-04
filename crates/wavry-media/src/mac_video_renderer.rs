use anyhow::{anyhow, Result};
use objc2_video_toolbox::{VTDecompressionSession, VTDecompressionOutputCallbackRecord, VTDecompressionSessionCreate, VTDecodeInfoFlags};
use objc2_core_media::{CMSampleBuffer, CMVideoFormatDescription, CMTime};
use objc2_core_video::{CVImageBuffer, CVBuffer};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject};
use objc2::{msg_send};
use std::ffi::{c_void};
use std::ptr::{NonNull, null, null_mut};
use core::ffi::c_int;
use std::ffi::c_long;

type OSStatus = i32;

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferCreateForImageBuffer(
        allocator: *const c_void, // CFAllocatorRef
        image_buffer: *mut CVImageBuffer,
        charged_item: *mut c_void,
        sample_buffer_out: *mut *mut CMSampleBuffer,
    ) -> OSStatus;

    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        allocator: *const c_void,
        parameter_set_count: usize,
        parameter_set_pointers: *const *const u8,
        parameter_set_sizes: *const usize,
        nal_unit_header_length: c_int,
        format_description_out: *mut *mut CMVideoFormatDescription,
    ) -> OSStatus;
    
    // CoreFoundation
    fn CFRelease(cf: *const c_void);
    // VTSessionInvalidate
    fn VTDecompressionSessionInvalidate(session: *mut VTDecompressionSession);
}

// Struct to pass context to the callback
struct Context {
    layer: Retained<AnyObject>, 
}

pub struct MacVideoRenderer {
    session: *mut VTDecompressionSession,
    format_desc: *mut CMVideoFormatDescription,
    context: *mut Context, // Raw pointer to Box<Context>
}

unsafe impl Send for MacVideoRenderer {}

unsafe extern "C-unwind" fn decompression_callback(
    decompression_output_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: OSStatus,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVBuffer, 
    _presentation_time_stamp: CMTime,
    _presentation_duration: CMTime,
) {
    if status != 0 || image_buffer.is_null() {
        return;
    }

    let ctx_ptr = decompression_output_ref_con as *mut Context;
    if ctx_ptr.is_null() {
        return;
    }
    let ctx = &*ctx_ptr;

    let mut sample_buffer: *mut CMSampleBuffer = std::ptr::null_mut();
    // note: CVBuffer is compatible with CVImageBuffer for this call usually, 
    // but the binding might expect specific type. 
    // CVImageBuffer inherits from CVBuffer. pointer cast is safe.
    let create_status = CMSampleBufferCreateForImageBuffer(
        std::ptr::null(),
        image_buffer as *mut CVImageBuffer,
        std::ptr::null_mut(),
        &mut sample_buffer,
    );

    if create_status == 0 && !sample_buffer.is_null() {
        // Enqueue to AVSampleBufferDisplayLayer
        let _: () = msg_send![&ctx.layer, enqueueSampleBuffer: sample_buffer];
        // Release the sample buffer we just created
        CFRelease(sample_buffer as *const c_void);
    }
}

impl MacVideoRenderer {
    pub fn new(layer_ptr: *mut c_void) -> Result<Self> {
        if layer_ptr.is_null() {
            return Err(anyhow!("Layer pointer is null"));
        }
        
        let layer = unsafe { Retained::retain(layer_ptr as *mut AnyObject) }
            .ok_or(anyhow!("Failed to retain layer"))?;
            
        let context = Box::new(Context { layer });
        let context_ptr = Box::into_raw(context);

        Ok(Self {
            session: std::ptr::null_mut(),
            format_desc: std::ptr::null_mut(),
            context: context_ptr,
        })
    }
    
    fn create_session(&mut self, format_desc: *mut CMVideoFormatDescription) -> Result<()> {
         // Invalidate old session
         if !self.session.is_null() {
             unsafe { 
                VTDecompressionSessionInvalidate(self.session); 
                CFRelease(self.session as *const c_void);
             }
             self.session = std::ptr::null_mut();
         }

         let record = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(decompression_callback),
            decompressionOutputRefCon: self.context as *mut c_void,
        };

        let mut session: *mut VTDecompressionSession = std::ptr::null_mut();
        
        // Unsafe block for FFI
        let status = unsafe {
            VTDecompressionSessionCreate(
                None, // allocator (Option<&CFAllocator>)
                &*(format_desc as *const _), // reference to format desc
                None, // decoder specification (Option<&CFDictionary>)
                None, // image buffer attributes
                &record as *const _, // Raw pointer to record
                NonNull::new(&mut session).unwrap(), 
            )
        };

        if status != 0 {
            return Err(anyhow!("Failed to create decompression session: {}", status));
        }
        
        self.session = session;
        self.format_desc = format_desc; // Assuming caller transfers ownership or we retain.
        // For CMCreateFromH264... the caller usually owns ("Create Rule").
        // We will take ownership.
        
        Ok(())
    }
}

impl crate::Renderer for MacVideoRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        // Simple NALU parsing: Check for SPS/PPS (Type 7 and 8)
        // H.264 NALU header: lower 5 bits of first byte.
        // Assuming Annex B (start codes) or length-prefixed? 
        // Wavry uses Length-Prefixed usually for sending, or raw NALUs.
        // Let's assume we get one NALU per payload for simplicity, or a contiguous block.
        // If the payload starts with SPS, we re-initialize.
        
        if payload.len() > 0 {
            let nal_type = payload[0] & 0x1F;
            if nal_type == 7 { // SPS
                // We assume PPS follows immediately or in next packet. To simplify, we should buffer.
                // But for this scaffold, we'll assume we get SPS+PPS+IDR in one go or separate.
                // Let's assume the payload IS the SPS.
                // Real implementation needs robust parsing.
                return Ok(());
            }
        }
        
        // TODO: Decode Frame
        // VTDecompressionSessionDecodeFrame(self.session, sample_buffer, flags, frame_ref_con, info_flags_out)
        
        Ok(())
    }
}

impl Drop for MacVideoRenderer {
    fn drop(&mut self) {
        unsafe {
            if !self.session.is_null() {
                VTDecompressionSessionInvalidate(self.session);
                CFRelease(self.session as *const c_void);
            }
            if !self.format_desc.is_null() {
                CFRelease(self.format_desc as *const c_void);
            }
            if !self.context.is_null() {
                let _ = Box::from_raw(self.context);
            }
        }
    }
}
