//! macOS Video Renderer using VideoToolbox VTDecompressionSession
//! 
//! Decodes H.264/HEVC video and displays it via AVSampleBufferDisplayLayer.

#![allow(dead_code, unused_imports, unused_variables, deprecated)]
use anyhow::{anyhow, Result};
use objc2_video_toolbox::{VTDecompressionSession, VTDecompressionOutputCallbackRecord, VTDecompressionSessionCreate, VTDecodeInfoFlags};
use objc2_core_media::{CMSampleBuffer, CMVideoFormatDescription, CMTime, CMTimeFlags, CMBlockBuffer};
use objc2_core_video::{CVImageBuffer, CVBuffer};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use std::ffi::c_void;
use std::ptr::NonNull;
use core::ffi::c_int;
use log::{debug, warn, info, error};

type OSStatus = i32;

// H.264 NAL Unit Types
const NAL_SLICE: u8 = 1;
const NAL_DPA: u8 = 2;
const NAL_DPB: u8 = 3;
const NAL_DPC: u8 = 4;
const NAL_IDR: u8 = 5;
const NAL_SEI: u8 = 6;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;
const NAL_AUD: u8 = 9;

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        allocator: *const c_void,
        parameter_set_count: usize,
        parameter_set_pointers: *const *const u8,
        parameter_set_sizes: *const usize,
        nal_unit_header_length: c_int,
        format_description_out: *mut *mut CMVideoFormatDescription,
    ) -> OSStatus;
    
    fn CMBlockBufferCreateWithMemoryBlock(
        allocator: *const c_void,
        memory_block: *mut c_void,
        block_length: usize,
        block_allocator: *const c_void,
        custom_block_source: *const c_void,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut *mut CMBlockBuffer,
    ) -> OSStatus;
    
    fn CMSampleBufferCreateReady(
        allocator: *const c_void,
        data_buffer: *mut CMBlockBuffer,
        format_description: *mut CMVideoFormatDescription,
        num_samples: i32,
        num_sample_timing_entries: i32,
        sample_timing_array: *const c_void,
        num_sample_size_entries: i32,
        sample_size_array: *const usize,
        sample_buffer_out: *mut *mut CMSampleBuffer,
    ) -> OSStatus;
    
    fn CMSampleBufferCreate(
        allocator: *const c_void,
        data_buffer: *mut CMBlockBuffer,
        data_ready: bool,
        make_data_ready_callback: *const c_void,
        make_data_ready_refcon: *mut c_void,
        format_description: *mut CMVideoFormatDescription,
        num_samples: i32,
        num_sample_timing_entries: i32,
        sample_timing_array: *const c_void,
        num_sample_size_entries: i32,
        sample_size_array: *const usize,
        sample_buffer_out: *mut *mut CMSampleBuffer,
    ) -> OSStatus;

    fn CMSampleBufferCreateForImageBuffer(
        allocator: *const c_void,
        image_buffer: *mut CVBuffer,
        data_ready: bool,
        make_data_ready_callback: *const c_void,
        make_data_ready_refcon: *mut c_void,
        format_description: *mut CMVideoFormatDescription,
        sample_timing_entry: *const c_void, // CMSampleTimingInfo
        sample_buffer_out: *mut *mut CMSampleBuffer,
    ) -> OSStatus;
    
    fn CFRelease(cf: *const c_void);
    fn VTDecompressionSessionInvalidate(session: *mut VTDecompressionSession);
    fn VTDecompressionSessionDecodeFrame(
        session: *mut VTDecompressionSession,
        sample_buffer: *mut CMSampleBuffer,
        decode_flags: u32,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
}

// Decode flags
const K_VT_DECODE_FRAME_ENABLE_ASYNC_DECOMPRESSION: u32 = 1 << 0;
const K_VT_DECODE_FRAME_DO_NOT_OUTPUT_FRAME: u32 = 1 << 1;

/// Struct to pass context to the decompression callback
struct DecoderContext {
    layer: Retained<AnyObject>, 
}

pub struct MacVideoRenderer {
    session: *mut VTDecompressionSession,
    format_desc: *mut CMVideoFormatDescription,
    context: *mut DecoderContext,
    
    // Buffer for storing SPS/PPS until we have both
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    
    // Frame counter for debugging
    frames_decoded: u64,
}

unsafe impl Send for MacVideoRenderer {}
unsafe impl Sync for MacVideoRenderer {}

/// Decompression output callback - receives decoded frames
unsafe extern "C-unwind" fn decompression_callback(
    decompression_output_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: OSStatus,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVBuffer, 
    presentation_time_stamp: CMTime,
    _presentation_duration: CMTime,
) {
    if status != 0 {
        warn!("Decompression callback error: {}", status);
        return;
    }
    
    if image_buffer.is_null() {
        return;
    }

    let ctx_ptr = decompression_output_ref_con as *mut DecoderContext;
    if ctx_ptr.is_null() {
        return;
    }
    let ctx = &*ctx_ptr;

    // Enqueue the decoded image buffer to AVSampleBufferDisplayLayer
    // The layer expects CMSampleBuffer, but for video display we can use a simpler path:
    // Just enqueue the CVPixelBuffer directly using the layer's enqueuePixelBuffer method
    // if available, or create a CMSampleBuffer wrapper.
    
    // For AVSampleBufferDisplayLayer, we need to wrap in CMSampleBuffer
    // This is a simplified approach - directly enqueueing the pixel buffer
    // In practice, we'd create a proper CMSampleBuffer with timing info
    
    // Create timing info for the sample
    #[repr(C)]
    struct CMSampleTimingInfo {
        duration: CMTime,
        presentation_time_stamp: CMTime,
        decode_time_stamp: CMTime,
    }
    
    // We don't have duration info easily available here without tracking previous frames,
    // but for display it matters less. We can set invalid duration.
    let timing = CMSampleTimingInfo {
        duration: CMTime { value: 0, timescale: 0, flags: CMTimeFlags(0), epoch: 0 }, // kCMTimeInvalid
        presentation_time_stamp,
        decode_time_stamp: CMTime { value: 0, timescale: 0, flags: CMTimeFlags(0), epoch: 0 }, // kCMTimeInvalid
    };
    
    let mut sample_buffer: *mut CMSampleBuffer = std::ptr::null_mut();
    
    // Create CMSampleBuffer from CVPixelBuffer (image_buffer)
    let status = unsafe {
        CMSampleBufferCreateForImageBuffer(
            std::ptr::null(), // allocator
            image_buffer,
            true,             // data ready
            std::ptr::null(), // callback
            std::ptr::null_mut(), // refcon
            std::ptr::null_mut(), // format description (inferred)
            &timing as *const _ as *const c_void,
            &mut sample_buffer
        )
    };
    
    if status == 0 && !sample_buffer.is_null() {
        // Enqueue to layer
        unsafe {
            // Check if layer responds to enqueueSampleBuffer:
            // For now assume it is AVSampleBufferDisplayLayer
            let _: () = msg_send![&ctx.layer, enqueueSampleBuffer: sample_buffer];
            CFRelease(sample_buffer as *const c_void);
        }
    } else {
        warn!("Failed to create CMSampleBuffer for display: {}", status);
    }
    
    let pts_val = presentation_time_stamp.value;
    // debug!("Decoded frame at PTS: {}", pts_val);
}

impl MacVideoRenderer {
    pub fn new(layer_ptr: *mut c_void) -> Result<Self> {
        if layer_ptr.is_null() {
            return Err(anyhow!("Layer pointer is null"));
        }
        
        let layer = unsafe { Retained::retain(layer_ptr as *mut AnyObject) }
            .ok_or(anyhow!("Failed to retain layer"))?;
            
        let context = Box::new(DecoderContext { layer });
        let context_ptr = Box::into_raw(context);

        info!("MacVideoRenderer created");

        Ok(Self {
            session: std::ptr::null_mut(),
            format_desc: std::ptr::null_mut(),
            context: context_ptr,
            sps: None,
            pps: None,
            frames_decoded: 0,
        })
    }
    
    /// Parse AVCC/length-prefixed NAL units from the payload
    /// VideoToolbox encoder outputs AVCC format (4-byte length prefix + NAL)
    fn parse_avcc_nalus(data: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut nalus = Vec::new();
        let mut offset = 0;
        
        while offset + 4 <= data.len() {
            // Read 4-byte big-endian length
            let length = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            
            offset += 4;
            
            if offset + length > data.len() {
                warn!("Invalid AVCC length: {} at offset {}, remaining {}", length, offset, data.len() - offset);
                break;
            }
            
            let nal_data = &data[offset..offset + length];
            if !nal_data.is_empty() {
                let nal_type = nal_data[0] & 0x1F;
                nalus.push((nal_type, nal_data.to_vec()));
            }
            
            offset += length;
        }
        
        nalus
    }
    
    /// Create format description from SPS and PPS
    fn create_format_description(&mut self) -> Result<()> {
        let sps = self.sps.as_ref().ok_or(anyhow!("No SPS available"))?;
        let pps = self.pps.as_ref().ok_or(anyhow!("No PPS available"))?;
        
        // Free old format description
        if !self.format_desc.is_null() {
            unsafe { CFRelease(self.format_desc as *const c_void) };
            self.format_desc = std::ptr::null_mut();
        }
        
        // Create parameter set arrays
        let param_set_ptrs: [*const u8; 2] = [sps.as_ptr(), pps.as_ptr()];
        let param_set_sizes: [usize; 2] = [sps.len(), pps.len()];
        
        let mut format_desc: *mut CMVideoFormatDescription = std::ptr::null_mut();
        
        let status = unsafe {
            CMVideoFormatDescriptionCreateFromH264ParameterSets(
                std::ptr::null(),           // allocator
                2,                          // parameter set count
                param_set_ptrs.as_ptr(),    // parameter set pointers
                param_set_sizes.as_ptr(),   // parameter set sizes
                4,                          // NAL unit header length (AVCC uses 4)
                &mut format_desc,           // output
            )
        };
        
        if status != 0 {
            return Err(anyhow!("Failed to create format description: OSStatus {}", status));
        }
        
        self.format_desc = format_desc;
        info!("Created H.264 format description from SPS ({} bytes) and PPS ({} bytes)", 
              sps.len(), pps.len());
        
        Ok(())
    }
    
    /// Create decompression session with current format description
    fn create_session(&mut self) -> Result<()> {
        if self.format_desc.is_null() {
            return Err(anyhow!("No format description available"));
        }
        
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
        
        let status = unsafe {
            VTDecompressionSessionCreate(
                None,                                   // allocator
                &*(self.format_desc as *const _),       // format description
                None,                                   // decoder specification
                None,                                   // destination image buffer attributes
                &record as *const _,                    // output callback record
                NonNull::new(&mut session).unwrap(),    // session out
            )
        };

        if status != 0 {
            return Err(anyhow!("Failed to create decompression session: OSStatus {}", status));
        }
        
        self.session = session;
        info!("Created VTDecompressionSession");
        
        Ok(())
    }
    
    /// Decode a video frame (NAL unit in AVCC format)
    fn decode_frame(&mut self, data: &[u8], timestamp_us: u64) -> Result<()> {
        if self.session.is_null() {
            return Err(anyhow!("No decompression session"));
        }
        
        // Create CMBlockBuffer from the data
        // We need to copy the data because VideoToolbox may access it asynchronously
        let mut block_buffer: *mut CMBlockBuffer = std::ptr::null_mut();
        let data_copy = data.to_vec();
        let data_len = data_copy.len();
        let data_ptr = Box::into_raw(data_copy.into_boxed_slice()) as *mut c_void;
        
        let status = unsafe {
            CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(),       // allocator
                data_ptr,               // memory block
                data_len,               // block length
                std::ptr::null(),       // block allocator (NULL = default)
                std::ptr::null(),       // custom block source
                0,                      // offset to data
                data_len,               // data length
                0,                      // flags
                &mut block_buffer,      // output
            )
        };
        
        if status != 0 || block_buffer.is_null() {
            // Clean up the data we allocated
            unsafe { 
                // Reconstruct Vec to drop it properly
                let _ = Vec::from_raw_parts(data_ptr as *mut u8, data_len, data_len); 
            }
            return Err(anyhow!("Failed to create CMBlockBuffer: OSStatus {}", status));
        }
        
        // Create timing info
        let pts = CMTime {
            value: timestamp_us as i64,
            timescale: 1_000_000, // microseconds
            flags: CMTimeFlags(1), // kCMTimeFlags_Valid
            epoch: 0,
        };
        
        // Sample size
        let sample_size = data_len;
        
        // Create CMSampleBuffer
        let mut sample_buffer: *mut CMSampleBuffer = std::ptr::null_mut();
        
        let status = unsafe {
            CMSampleBufferCreate(
                std::ptr::null(),           // allocator
                block_buffer,               // data buffer
                true,                       // data is ready
                std::ptr::null(),           // make data ready callback
                std::ptr::null_mut(),       // make data ready refcon
                self.format_desc,           // format description
                1,                          // num samples
                0,                          // num sample timing entries (0 = no timing)
                std::ptr::null(),           // sample timing array
                1,                          // num sample size entries
                &sample_size,               // sample size array
                &mut sample_buffer,         // output
            )
        };
        
        // Release block buffer (sample buffer now owns it)
        unsafe { CFRelease(block_buffer as *const c_void); }
        
        if status != 0 || sample_buffer.is_null() {
            return Err(anyhow!("Failed to create CMSampleBuffer: OSStatus {}", status));
        }
        
        // Decode the frame
        let mut info_flags: u32 = 0;
        let decode_status = unsafe {
            VTDecompressionSessionDecodeFrame(
                self.session,
                sample_buffer,
                K_VT_DECODE_FRAME_ENABLE_ASYNC_DECOMPRESSION,
                std::ptr::null_mut(),   // source frame ref con
                &mut info_flags,
            )
        };
        
        // Release sample buffer
        unsafe { CFRelease(sample_buffer as *const c_void); }
        
        if decode_status != 0 {
            return Err(anyhow!("Failed to decode frame: OSStatus {}", decode_status));
        }
        
        self.frames_decoded += 1;
        
        Ok(())
    }
    
    pub fn frames_decoded(&self) -> u64 {
        self.frames_decoded
    }
}

impl crate::Renderer for MacVideoRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        if payload.is_empty() {
            return Ok(());
        }
        
        // Parse AVCC NAL units from the payload
        let nalus = Self::parse_avcc_nalus(payload);
        
        if nalus.is_empty() {
            // Try treating the entire payload as a single NAL (Annex B without length prefix)
            let nal_type = payload[0] & 0x1F;
            return self.handle_nal_unit(nal_type, payload, timestamp_us);
        }
        
        // Process each NAL unit
        for (nal_type, nal_data) in nalus {
            self.handle_nal_unit(nal_type, &nal_data, timestamp_us)?;
        }
        
        Ok(())
    }
}

impl MacVideoRenderer {
    fn handle_nal_unit(&mut self, nal_type: u8, nal_data: &[u8], timestamp_us: u64) -> Result<()> {
        match nal_type {
            NAL_SPS => {
                debug!("Received SPS ({} bytes)", nal_data.len());
                self.sps = Some(nal_data.to_vec());
                
                // If we have both SPS and PPS, create the decoder
                if self.pps.is_some() {
                    self.create_format_description()?;
                    self.create_session()?;
                }
            }
            NAL_PPS => {
                debug!("Received PPS ({} bytes)", nal_data.len());
                self.pps = Some(nal_data.to_vec());
                
                // If we have both SPS and PPS, create the decoder
                if self.sps.is_some() {
                    self.create_format_description()?;
                    self.create_session()?;
                }
            }
            NAL_IDR | NAL_SLICE => {
                // Video frame - decode it
                if self.session.is_null() {
                    debug!("Skipping frame - no decoder session yet");
                    return Ok(());
                }
                
                // Reconstruct AVCC format: 4-byte length prefix + NAL data
                let mut avcc_data = Vec::with_capacity(4 + nal_data.len());
                avcc_data.extend_from_slice(&(nal_data.len() as u32).to_be_bytes());
                avcc_data.extend_from_slice(nal_data);
                
                self.decode_frame(&avcc_data, timestamp_us)?;
            }
            NAL_SEI | NAL_AUD => {
                // Ignore SEI and access unit delimiter
            }
            _ => {
                debug!("Ignoring NAL type {}", nal_type);
            }
        }
        
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
        info!("MacVideoRenderer dropped (decoded {} frames)", self.frames_decoded);
    }
}
