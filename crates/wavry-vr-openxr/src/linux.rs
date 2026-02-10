use std::env;
use std::ffi::CString;
use std::ptr;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use ash::vk::Handle;
use ash::{vk, Entry as VkEntry};
use glow::HasContext;
use openxr as xr;
use x11::{glx, xlib};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use wavry_vr::types::{StreamConfig, VideoCodec, VrTiming};
use wavry_vr::{VrError, VrResult};

use crate::common::{eye_layout, to_pose, HandTrackingState, InputActions};
use crate::SharedState;

const VIEW_COUNT: usize = 2;
const VULKAN_FENCE_SKIP_LIMIT: u32 = 2;
const VULKAN_FENCE_WAIT_TIMEOUT_NS: u64 = 1_000_000;

struct GlxContext {
    display: *mut xlib::Display,
    fb_config: glx::GLXFBConfig,
    visualid: u64,
    drawable: glx::GLXDrawable,
    context: glx::GLXContext,
}

impl GlxContext {
    unsafe fn new() -> VrResult<Self> {
        let display = xlib::XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err(VrError::Adapter("XOpenDisplay failed".to_string()));
        }

        let screen = xlib::XDefaultScreen(display);
        let attrs = [
            glx::GLX_X_RENDERABLE,
            1,
            glx::GLX_DRAWABLE_TYPE,
            glx::GLX_WINDOW_BIT,
            glx::GLX_RENDER_TYPE,
            glx::GLX_RGBA_BIT,
            glx::GLX_X_VISUAL_TYPE,
            glx::GLX_TRUE_COLOR,
            glx::GLX_RED_SIZE,
            8,
            glx::GLX_GREEN_SIZE,
            8,
            glx::GLX_BLUE_SIZE,
            8,
            glx::GLX_ALPHA_SIZE,
            8,
            glx::GLX_DEPTH_SIZE,
            24,
            glx::GLX_STENCIL_SIZE,
            8,
            glx::GLX_DOUBLEBUFFER,
            1,
            0,
        ];

        let mut fbcount = 0;
        let fb_configs = glx::glXChooseFBConfig(display, screen, attrs.as_ptr(), &mut fbcount);
        if fb_configs.is_null() || fbcount == 0 {
            xlib::XCloseDisplay(display);
            return Err(VrError::Adapter("glXChooseFBConfig failed".to_string()));
        }
        let fb_config = *fb_configs;

        let visual_info = glx::glXGetVisualFromFBConfig(display, fb_config);
        if visual_info.is_null() {
            xlib::XFree(fb_configs as *mut _);
            xlib::XCloseDisplay(display);
            return Err(VrError::Adapter(
                "glXGetVisualFromFBConfig failed".to_string(),
            ));
        }
        let visualid = (*visual_info).visualid;

        let root = xlib::XDefaultRootWindow(display);
        let colormap = xlib::XCreateColormap(display, root, (*visual_info).visual, xlib::AllocNone);

        let mut swa: xlib::XSetWindowAttributes = std::mem::zeroed();
        swa.colormap = colormap;
        swa.event_mask = 0;
        let window = xlib::XCreateWindow(
            display,
            root,
            0,
            0,
            16,
            16,
            0,
            (*visual_info).depth,
            xlib::InputOutput as u32,
            (*visual_info).visual,
            xlib::CWColormap,
            &mut swa,
        );
        let title = CString::new("wavry-vr").unwrap();
        xlib::XStoreName(display, window, title.as_ptr());
        xlib::XMapWindow(display, window);

        let context =
            glx::glXCreateNewContext(display, fb_config, glx::GLX_RGBA_TYPE, ptr::null_mut(), 1);
        if context.is_null() {
            xlib::XFree(visual_info as *mut _);
            xlib::XFree(fb_configs as *mut _);
            xlib::XCloseDisplay(display);
            return Err(VrError::Adapter("glXCreateNewContext failed".to_string()));
        }

        if glx::glXMakeCurrent(display, window, context) == 0 {
            glx::glXDestroyContext(display, context);
            xlib::XFree(visual_info as *mut _);
            xlib::XFree(fb_configs as *mut _);
            xlib::XCloseDisplay(display);
            return Err(VrError::Adapter("glXMakeCurrent failed".to_string()));
        }

        xlib::XFree(visual_info as *mut _);
        xlib::XFree(fb_configs as *mut _);

        Ok(Self {
            display,
            fb_config,
            visualid,
            drawable: window,
            context,
        })
    }
}

impl Drop for GlxContext {
    fn drop(&mut self) {
        unsafe {
            glx::glXMakeCurrent(self.display, 0, ptr::null_mut());
            glx::glXDestroyContext(self.display, self.context);
            xlib::XDestroyWindow(self.display, self.drawable);
            xlib::XCloseDisplay(self.display);
        }
    }
}

struct GstDecoder {
    _pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    appsink: gst_app::AppSink,
    width: u32,
    height: u32,
}

struct DecodedFrame {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

impl GstDecoder {
    fn new(config: StreamConfig) -> VrResult<Self> {
        gst::init().map_err(|e| VrError::Adapter(format!("gstreamer init: {e}")))?;

        let parser = match config.codec {
            VideoCodec::Av1 => "av1parse",
            VideoCodec::Hevc => "h265parse",
            VideoCodec::H264 => "h264parse",
        };

        let pipeline_str = format!(
            "appsrc name=src is-live=true format=time do-timestamp=true ! {parser} ! decodebin ! videoconvert ! video/x-raw,format=RGBA,width={w},height={h} ! appsink name=sink max-buffers=1 drop=true sync=false",
            w = config.width,
            h = config.height,
        );

        let pipeline = gst::parse::launch(&pipeline_str)
            .map_err(|e| VrError::Adapter(format!("gst parse: {e}")))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| VrError::Adapter("gst pipeline downcast failed".to_string()))?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| VrError::Adapter("gst appsrc missing".to_string()))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| VrError::Adapter("gst appsrc type mismatch".to_string()))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| VrError::Adapter("gst appsink missing".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| VrError::Adapter("gst appsink type mismatch".to_string()))?;

        let caps_str = match config.codec {
            VideoCodec::Av1 => "video/x-av1,stream-format=(string)obu-stream,alignment=(string)tu",
            VideoCodec::Hevc => {
                "video/x-h265,stream-format=(string)byte-stream,alignment=(string)au"
            }
            VideoCodec::H264 => {
                "video/x-h264,stream-format=(string)byte-stream,alignment=(string)au"
            }
        };
        let caps = gst::Caps::from_str(caps_str)
            .map_err(|e| VrError::Adapter(format!("gst caps: {e}")))?;
        appsrc.set_caps(Some(&caps));

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| VrError::Adapter(format!("gst state: {e:?}")))?;

        Ok(Self {
            _pipeline: pipeline,
            appsrc,
            appsink,
            width: config.width as u32,
            height: config.height as u32,
        })
    }

    fn decode(&self, payload: &[u8], timestamp_us: u64) -> VrResult<Option<DecodedFrame>> {
        let mut buffer = gst::Buffer::with_size(payload.len())
            .map_err(|e| VrError::Adapter(format!("gst buffer: {e}")))?;
        {
            let buffer = buffer
                .get_mut()
                .ok_or_else(|| VrError::Adapter("gst buffer mut failed".to_string()))?;
            buffer
                .copy_from_slice(0, payload)
                .map_err(|_| VrError::Adapter("gst buffer copy failed".to_string()))?;
            buffer.set_pts(gst::ClockTime::from_nseconds(timestamp_us * 1_000));
        }
        self.appsrc
            .push_buffer(buffer)
            .map_err(|e| VrError::Adapter(format!("gst push: {e}")))?;

        let sample = self
            .appsink
            .try_pull_sample(gst::ClockTime::from_mseconds(2));
        let sample = match sample {
            Some(sample) => sample,
            None => return Ok(None),
        };

        let buffer = sample
            .buffer()
            .ok_or_else(|| VrError::Adapter("gst missing buffer".to_string()))?;
        let map = buffer
            .map_readable()
            .map_err(|e| VrError::Adapter(format!("gst map: {e}")))?;
        Ok(Some(DecodedFrame {
            data: map.as_slice().to_vec(),
            width: self.width,
            height: self.height,
        }))
    }
}

struct VulkanContext {
    instance: ash::Instance,
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
    queue: vk::Queue,
    queue_family_index: u32,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_size: vk::DeviceSize,
    upload_fence: vk::Fence,
    fence_in_use: bool,
    fence_skip_count: u32,
}

impl VulkanContext {
    fn new(xr_instance: &xr::Instance, system: xr::SystemId) -> VrResult<Self> {
        let entry = unsafe { VkEntry::load() }
            .map_err(|e| VrError::Adapter(format!("Vulkan entry load failed: {e}")))?;

        let reqs = xr_instance
            .graphics_requirements::<xr::Vulkan>(system)
            .map_err(|e| VrError::Adapter(format!("OpenXR Vulkan requirements: {e:?}")))?;
        let api_version = vk::make_api_version(
            0,
            reqs.min_api_version_supported.major() as u32,
            reqs.min_api_version_supported.minor() as u32,
            reqs.min_api_version_supported.patch(),
        );

        let instance_exts = xr_instance
            .vulkan_legacy_instance_extensions(system)
            .map_err(|e| VrError::Adapter(format!("OpenXR Vulkan instance extensions: {e:?}")))?;
        let instance_exts = parse_extension_list(&instance_exts);
        let instance_ext_ptrs: Vec<*const i8> = instance_exts.iter().map(|s| s.as_ptr()).collect();

        let app_name = CString::new("Wavry").unwrap();
        let engine_name = CString::new("Wavry").unwrap();
        let app_info = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .engine_name(&engine_name)
            .api_version(api_version);

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_extension_names(&instance_ext_ptrs);

        let instance = unsafe {
            entry
                .create_instance(&create_info, None)
                .map_err(|e| VrError::Adapter(format!("Vulkan instance create failed: {e}")))?
        };

        let physical_device = unsafe {
            xr_instance.vulkan_graphics_device(system, instance.handle().as_raw() as *const _)
        }
        .map_err(|e| VrError::Adapter(format!("OpenXR Vulkan graphics device: {e:?}")))?;
        let physical_device = vk::PhysicalDevice::from_raw(physical_device as u64);

        let queue_family_index = find_graphics_queue_family(&instance, physical_device)?;

        let device_exts = xr_instance
            .vulkan_legacy_device_extensions(system)
            .map_err(|e| VrError::Adapter(format!("OpenXR Vulkan device extensions: {e:?}")))?;
        let device_exts = parse_extension_list(&device_exts);
        let device_ext_ptrs: Vec<*const i8> = device_exts.iter().map(|s| s.as_ptr()).collect();

        let priorities = [1.0f32];
        let queue_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities);

        let device_create = vk::DeviceCreateInfo::builder()
            .queue_create_infos(std::slice::from_ref(&queue_info))
            .enabled_extension_names(&device_ext_ptrs);

        let device = unsafe {
            instance
                .create_device(physical_device, &device_create, None)
                .map_err(|e| VrError::Adapter(format!("Vulkan device create failed: {e}")))?
        };

        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let command_pool_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe {
            device
                .create_command_pool(&command_pool_info, None)
                .map_err(|e| VrError::Adapter(format!("Vulkan command pool create failed: {e}")))?
        };

        let command_buffer_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe {
            device
                .allocate_command_buffers(&command_buffer_info)
                .map_err(|e| VrError::Adapter(format!("Vulkan command buffer alloc failed: {e}")))?
        }[0];

        let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
        let upload_fence = unsafe {
            device
                .create_fence(&fence_info, None)
                .map_err(|e| VrError::Adapter(format!("Vulkan fence create failed: {e}")))?
        };

        Ok(Self {
            instance,
            device,
            physical_device,
            queue,
            queue_family_index,
            command_pool,
            command_buffer,
            staging_buffer: vk::Buffer::null(),
            staging_memory: vk::DeviceMemory::null(),
            staging_size: 0,
            upload_fence,
            fence_in_use: false,
            fence_skip_count: 0,
        })
    }

    fn ensure_staging(&mut self, size: vk::DeviceSize) -> VrResult<()> {
        if self.staging_size >= size && self.staging_buffer != vk::Buffer::null() {
            return Ok(());
        }

        unsafe {
            if self.staging_buffer != vk::Buffer::null() {
                self.device.destroy_buffer(self.staging_buffer, None);
                self.device.free_memory(self.staging_memory, None);
                self.staging_buffer = vk::Buffer::null();
                self.staging_memory = vk::DeviceMemory::null();
                self.staging_size = 0;
            }
        }

        let buffer_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let staging_buffer = unsafe {
            self.device.create_buffer(&buffer_info, None).map_err(|e| {
                VrError::Adapter(format!("Vulkan staging buffer create failed: {e}"))
            })?
        };
        let req = unsafe { self.device.get_buffer_memory_requirements(staging_buffer) };
        let memory_type_index = find_memory_type(
            &self.instance,
            self.physical_device,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?
        .ok_or_else(|| VrError::Adapter("No suitable Vulkan memory type".to_string()))?;

        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(req.size)
            .memory_type_index(memory_type_index);
        let staging_memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| VrError::Adapter(format!("Vulkan memory alloc failed: {e}")))?
        };
        unsafe {
            self.device
                .bind_buffer_memory(staging_buffer, staging_memory, 0)
                .map_err(|e| VrError::Adapter(format!("Vulkan bind memory failed: {e}")))?;
        }

        self.staging_buffer = staging_buffer;
        self.staging_memory = staging_memory;
        self.staging_size = size;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_rgba_region(
        &mut self,
        image: vk::Image,
        old_layout: vk::ImageLayout,
        src_width: u32,
        src_height: u32,
        src_offset_x: u32,
        copy_width: u32,
        copy_height: u32,
        data: &[u8],
    ) -> VrResult<vk::ImageLayout> {
        let size = (copy_width as vk::DeviceSize) * (copy_height as vk::DeviceSize) * 4;
        if size == 0 {
            return Ok(old_layout);
        }

        if self.fence_in_use {
            let ready = unsafe { self.device.get_fence_status(self.upload_fence) }
                .map_err(|e| VrError::Adapter(format!("Vulkan fence status failed: {e}")))?;
            if !ready {
                self.fence_skip_count = self.fence_skip_count.saturating_add(1);
                if self.fence_skip_count < VULKAN_FENCE_SKIP_LIMIT {
                    return Ok(old_layout);
                }
                let wait_result = unsafe {
                    self.device.wait_for_fences(
                        &[self.upload_fence],
                        true,
                        VULKAN_FENCE_WAIT_TIMEOUT_NS,
                    )
                };
                match wait_result {
                    Ok(()) => {
                        unsafe {
                            self.device
                                .reset_fences(&[self.upload_fence])
                                .map_err(|e| {
                                    VrError::Adapter(format!("Vulkan fence reset failed: {e}"))
                                })?;
                        }
                        self.fence_in_use = false;
                        self.fence_skip_count = 0;
                    }
                    Err(vk::Result::TIMEOUT) => {
                        return Ok(old_layout);
                    }
                    Err(err) => {
                        return Err(VrError::Adapter(format!("Vulkan fence wait failed: {err}")));
                    }
                }
            }
            unsafe {
                self.device
                    .reset_fences(&[self.upload_fence])
                    .map_err(|e| VrError::Adapter(format!("Vulkan fence reset failed: {e}")))?;
            }
            self.fence_in_use = false;
            self.fence_skip_count = 0;
        } else {
            unsafe {
                self.device
                    .reset_fences(&[self.upload_fence])
                    .map_err(|e| VrError::Adapter(format!("Vulkan fence reset failed: {e}")))?;
            }
            self.fence_skip_count = 0;
        }

        self.ensure_staging(size)?;

        unsafe {
            let ptr = self
                .device
                .map_memory(self.staging_memory, 0, size, vk::MemoryMapFlags::empty())
                .map_err(|e| VrError::Adapter(format!("Vulkan map memory failed: {e}")))?;
            let dst = ptr.cast::<u8>();
            if src_width == copy_width && src_height == copy_height && src_offset_x == 0 {
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, size as usize);
            } else {
                let src_stride = (src_width * 4) as usize;
                let dst_stride = (copy_width * 4) as usize;
                for row in 0..copy_height as usize {
                    let src_index = row * src_stride + (src_offset_x as usize * 4);
                    let dst_index = row * dst_stride;
                    let src_ptr = data.as_ptr().add(src_index);
                    let dst_ptr = dst.add(dst_index);
                    std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, dst_stride);
                }
            }
            self.device.unmap_memory(self.staging_memory);

            self.device
                .reset_command_buffer(self.command_buffer, vk::CommandBufferResetFlags::empty())
                .map_err(|e| {
                    VrError::Adapter(format!("Vulkan reset command buffer failed: {e}"))
                })?;

            let begin_info = vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device
                .begin_command_buffer(self.command_buffer, &begin_info)
                .map_err(|e| {
                    VrError::Adapter(format!("Vulkan begin command buffer failed: {e}"))
                })?;

            let subresource_range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };

            let to_transfer = vk::ImageMemoryBarrier::builder()
                .old_layout(old_layout)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(image)
                .subresource_range(subresource_range);

            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                std::slice::from_ref(&to_transfer),
            );

            let region = vk::BufferImageCopy::builder()
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_extent(vk::Extent3D {
                    width: copy_width,
                    height: copy_height,
                    depth: 1,
                });

            self.device.cmd_copy_buffer_to_image(
                self.command_buffer,
                self.staging_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&region),
            );

            let to_color = vk::ImageMemoryBarrier::builder()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                )
                .image(image)
                .subresource_range(subresource_range);

            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                std::slice::from_ref(&to_color),
            );

            self.device
                .end_command_buffer(self.command_buffer)
                .map_err(|e| VrError::Adapter(format!("Vulkan end command buffer failed: {e}")))?;

            let submit_info = vk::SubmitInfo::builder()
                .command_buffers(std::slice::from_ref(&self.command_buffer));
            self.device
                .queue_submit(
                    self.queue,
                    std::slice::from_ref(&submit_info),
                    self.upload_fence,
                )
                .map_err(|e| VrError::Adapter(format!("Vulkan queue submit failed: {e}")))?;
            self.fence_in_use = true;
            self.fence_skip_count = 0;
        }

        Ok(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            if self.staging_buffer != vk::Buffer::null() {
                self.device.destroy_buffer(self.staging_buffer, None);
            }
            if self.staging_memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.staging_memory, None);
            }
            if self.command_pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.command_pool, None);
            }
            if self.upload_fence != vk::Fence::null() {
                self.device.destroy_fence(self.upload_fence, None);
            }
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

fn parse_extension_list(list: &str) -> Vec<CString> {
    list.split_whitespace()
        .filter(|s| !s.is_empty())
        .map(|s| CString::new(s).unwrap())
        .collect()
}

fn find_graphics_queue_family(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
) -> VrResult<u32> {
    let families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    families
        .iter()
        .enumerate()
        .find(|(_, family)| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .map(|(idx, _)| idx as u32)
        .ok_or_else(|| VrError::Adapter("No Vulkan graphics queue family".to_string()))
}

fn find_memory_type(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> VrResult<Option<u32>> {
    let mem = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    for i in 0..mem.memory_type_count {
        let is_type = (type_bits & (1 << i)) != 0;
        let has_props = mem.memory_types[i as usize]
            .property_flags
            .contains(properties);
        if is_type && has_props {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

pub fn spawn(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
    thread::Builder::new()
        .name("wavry-pcvr-linux".to_string())
        .spawn(move || {
            wavry_vr::set_pcvr_status("PCVR: starting (Linux runtime)".to_string());
            let result = run(state);
            match result {
                Ok(()) => {
                    wavry_vr::set_pcvr_status("PCVR: runtime stopped".to_string());
                }
                Err(err) => {
                    wavry_vr::set_pcvr_status(format!("PCVR: runtime failed: {err}"));
                }
            }
        })
        .map_err(|e| VrError::Adapter(format!("thread spawn: {e}")))
}

fn describe_gl_swapchain_format(format: u32) -> (&'static str, bool) {
    match format {
        glow::RGBA8 => ("GL_RGBA8", false),
        glow::SRGB8_ALPHA8 => ("GL_SRGB8_ALPHA8", true),
        _ => ("UNKNOWN_GL_FORMAT", false),
    }
}

fn choose_gl_swapchain_format(formats: &[u32]) -> (u32, &'static str, bool) {
    let preferred = [glow::RGBA8, glow::SRGB8_ALPHA8];
    if let Some(format) = preferred.iter().copied().find(|fmt| formats.contains(fmt)) {
        let (name, srgb) = describe_gl_swapchain_format(format);
        return (format, name, srgb);
    }
    let fallback = formats.first().copied().unwrap_or(glow::RGBA8);
    let (name, srgb) = describe_gl_swapchain_format(fallback);
    (fallback, name, srgb)
}

fn describe_vk_swapchain_format(format: u32) -> (&'static str, bool) {
    if format == vk::Format::R8G8B8A8_UNORM.as_raw() as u32 {
        ("VK_FORMAT_R8G8B8A8_UNORM", false)
    } else if format == vk::Format::B8G8R8A8_UNORM.as_raw() as u32 {
        ("VK_FORMAT_B8G8R8A8_UNORM", false)
    } else if format == vk::Format::R8G8B8A8_SRGB.as_raw() as u32 {
        ("VK_FORMAT_R8G8B8A8_SRGB", true)
    } else if format == vk::Format::B8G8R8A8_SRGB.as_raw() as u32 {
        ("VK_FORMAT_B8G8R8A8_SRGB", true)
    } else {
        ("UNKNOWN_VK_FORMAT", false)
    }
}

fn choose_vk_swapchain_format(formats: &[u32]) -> (u32, &'static str, bool) {
    let preferred = [
        vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
        vk::Format::B8G8R8A8_UNORM.as_raw() as u32,
        vk::Format::R8G8B8A8_SRGB.as_raw() as u32,
        vk::Format::B8G8R8A8_SRGB.as_raw() as u32,
    ];
    if let Some(format) = preferred
        .iter()
        .copied()
        .find(|&fmt| formats.contains(&fmt))
    {
        let (name, srgb) = describe_vk_swapchain_format(format);
        return (format, name, srgb);
    }
    let fallback = formats
        .first()
        .copied()
        .unwrap_or(vk::Format::R8G8B8A8_UNORM.as_raw() as u32);
    let (name, srgb) = describe_vk_swapchain_format(fallback);
    (fallback, name, srgb)
}

fn log_swapchain_validation_u32(
    instance: &xr::Instance,
    backend: &str,
    available: &[u32],
    selected: u32,
    selected_name: &str,
    selected_srgb: bool,
) {
    let runtime = instance.properties().ok();
    let runtime_name = runtime
        .as_ref()
        .map(|p| p.runtime_name.as_str())
        .unwrap_or("unknown-runtime");
    let gamma_mode = if selected_srgb {
        "sRGB (runtime gamma conversion)"
    } else {
        "linear UNORM (passthrough)"
    };
    eprintln!(
        "OpenXR swapchain validation [{}]: runtime='{}' selected={} (0x{:X}) gamma_mode={} available={:?}",
        backend, runtime_name, selected_name, selected, gamma_mode, available
    );
}

fn run(state: Arc<SharedState>) -> VrResult<()> {
    let use_vulkan = env::var("WAVRY_USE_VULKAN").is_ok();
    if use_vulkan {
        run_vulkan(state)
    } else {
        run_glx(state)
    }
}

fn run_glx(state: Arc<SharedState>) -> VrResult<()> {
    let glx = unsafe { GlxContext::new()? };
    let gl = unsafe {
        glow::Context::from_loader_function(|s| {
            let s = CString::new(s).unwrap();
            std::mem::transmute(glx::glXGetProcAddress(s.as_ptr() as *const u8))
        })
    };
    unsafe {
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
    }

    let entry = unsafe { xr::Entry::load() }
        .map_err(|e| VrError::Adapter(format!("OpenXR load failed: {e:?}")))?;
    let available_exts = entry
        .enumerate_extensions()
        .map_err(|e| VrError::Adapter(format!("OpenXR ext enumerate: {e:?}")))?;
    if !available_exts.khr_opengl_enable {
        return Err(VrError::Unavailable(
            "OpenXR KHR_opengl_enable not available".to_string(),
        ));
    }
    let mut exts = xr::ExtensionSet::default();
    exts.khr_opengl_enable = true;
    if available_exts.ext_hand_tracking {
        exts.ext_hand_tracking = true;
    }

    let app_info = xr::ApplicationInfo {
        application_name: "Wavry",
        application_version: 1,
        engine_name: "Wavry",
        engine_version: 1,
        api_version: xr::Version::new(1, 0, 0),
    };
    let instance = entry
        .create_instance(&app_info, &exts, &[])
        .map_err(|e| VrError::Adapter(format!("OpenXR create_instance: {e:?}")))?;
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|e| VrError::Adapter(format!("OpenXR system: {e:?}")))?;

    let create_info = xr::opengl::SessionCreateInfo::Xlib {
        x_display: glx.display as *mut _,
        visualid: glx.visualid as u32,
        glx_fb_config: glx.fb_config as *mut _,
        glx_drawable: glx.drawable,
        glx_context: glx.context as *mut _,
    };

    let (session, mut frame_waiter, mut frame_stream) = unsafe {
        instance
            .create_session::<xr::OpenGL>(system, &create_info)
            .map_err(|e| VrError::Adapter(format!("OpenXR create_session: {e:?}")))?
    };
    wavry_vr::set_pcvr_status("PCVR: Linux X11 OpenGL runtime active".to_string());
    let mut input_actions = InputActions::new(&instance, &session).ok();
    let hand_tracking = if available_exts.ext_hand_tracking {
        HandTrackingState::new(&session).ok()
    } else {
        None
    };

    let reference_space = session
        .create_reference_space(
            xr::ReferenceSpaceType::LOCAL,
            xr::Posef {
                orientation: xr::Quaternionf {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                },
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            },
        )
        .map_err(|e| VrError::Adapter(format!("OpenXR reference space: {e:?}")))?;

    let mut event_buffer = xr::EventDataBuffer::new();
    let mut session_running = false;
    let mut decoder: Option<GstDecoder> = None;
    let mut swapchains: Option<[xr::Swapchain<xr::OpenGL>; VIEW_COUNT]> = None;
    let mut swapchain_images: Option<[Vec<u32>; VIEW_COUNT]> = None;
    let mut last_decoded: Option<DecodedFrame> = None;
    let mut last_refresh_hz: Option<f32> = None;

    loop {
        while let Some(event) = instance
            .poll_event(&mut event_buffer)
            .map_err(|e| VrError::Adapter(format!("OpenXR poll_event: {e:?}")))?
        {
            if let xr::Event::SessionStateChanged(e) = event {
                match e.state() {
                    xr::SessionState::READY => {
                        session
                            .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                            .map_err(|e| {
                                VrError::Adapter(format!("OpenXR session begin: {e:?}"))
                            })?;
                        session_running = true;
                    }
                    xr::SessionState::STOPPING => {
                        session
                            .end()
                            .map_err(|e| VrError::Adapter(format!("OpenXR session end: {e:?}")))?;
                        session_running = false;
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if state.stop.load(Ordering::Relaxed) {
            if session_running {
                let _ = session.end();
            }
            return Ok(());
        }

        if !session_running {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let frame_state = frame_waiter
            .wait()
            .map_err(|e| VrError::Adapter(format!("OpenXR wait: {e:?}")))?;
        frame_stream
            .begin()
            .map_err(|e| VrError::Adapter(format!("OpenXR begin: {e:?}")))?;

        let (_view_state, views) = session
            .locate_views(
                xr::ViewConfigurationType::PRIMARY_STEREO,
                frame_state.predicted_display_time,
                &reference_space,
            )
            .map_err(|e| VrError::Adapter(format!("OpenXR locate_views: {e:?}")))?;

        if !views.is_empty() {
            let pose = to_pose(views[0].pose);
            let timestamp_us = (frame_state.predicted_display_time.as_nanos() / 1_000) as u64;
            state.callbacks.on_pose_update(pose, timestamp_us);
            if let Some(actions) = input_actions.as_mut() {
                if let Ok(inputs) = actions.poll(&session, timestamp_us) {
                    for input in inputs {
                        state.callbacks.on_gamepad_input(input);
                    }
                }
            }
            if let Some(tracking) = hand_tracking.as_ref() {
                for hand_pose in tracking.poll(&reference_space, frame_state.predicted_display_time)
                {
                    state.callbacks.on_hand_pose_update(hand_pose, timestamp_us);
                }
            }
        }

        let period_ns = frame_state.predicted_display_period.as_nanos();
        if period_ns > 0 {
            let refresh_hz = 1_000_000_000.0 / period_ns as f32;
            let send = last_refresh_hz.is_none_or(|prev| (prev - refresh_hz).abs() > 0.1);
            if send {
                state.callbacks.on_vr_timing(VrTiming {
                    refresh_hz,
                    vsync_offset_us: 0,
                });
                last_refresh_hz = Some(refresh_hz);
            }
        }

        if decoder.is_none() {
            if let Some(cfg) = state.stream_config.lock().ok().and_then(|c| *c) {
                decoder = Some(GstDecoder::new(cfg)?);
            }
        }

        if let Some(frame) = state.take_latest_frame() {
            if let Some(decoder) = decoder.as_ref() {
                if let Some(decoded) = decoder.decode(&frame.data, frame.timestamp_us)? {
                    last_decoded = Some(decoded);
                }
            }
        }

        if swapchains.is_none() {
            if let Some(cfg) = state.stream_config.lock().ok().and_then(|c| *c) {
                let layout = eye_layout(cfg);
                let formats = session
                    .enumerate_swapchain_formats()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain formats: {e:?}")))?;
                let (format, format_name, format_srgb) = choose_gl_swapchain_format(&formats);
                log_swapchain_validation_u32(
                    &instance,
                    "linux-gl",
                    &formats,
                    format,
                    format_name,
                    format_srgb,
                );

                let create_info = xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                    format,
                    sample_count: 1,
                    width: layout.eye_width,
                    height: layout.eye_height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                };
                let sc0 = session
                    .create_swapchain(&create_info)
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain: {e:?}")))?;
                let sc1 = session
                    .create_swapchain(&create_info)
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain: {e:?}")))?;
                let imgs0 = sc0
                    .enumerate_images()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain images: {e:?}")))?;
                let imgs1 = sc1
                    .enumerate_images()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain images: {e:?}")))?;
                for tex in imgs0.iter().chain(imgs1.iter()) {
                    unsafe {
                        gl.bind_texture(
                            glow::TEXTURE_2D,
                            std::mem::transmute::<u32, Option<glow::NativeTexture>>(*tex),
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MIN_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_MAG_FILTER,
                            glow::LINEAR as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_S,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                        gl.tex_parameter_i32(
                            glow::TEXTURE_2D,
                            glow::TEXTURE_WRAP_T,
                            glow::CLAMP_TO_EDGE as i32,
                        );
                    }
                }
                swapchains = Some([sc0, sc1]);
                swapchain_images = Some([imgs0, imgs1]);
            }
        }

        if frame_state.should_render {
            if let (Some(swapchains), Some(swapchain_images)) =
                (swapchains.as_mut(), swapchain_images.as_ref())
            {
                let cfg = state.stream_config.lock().ok().and_then(|c| *c);
                let layout = cfg.map(eye_layout);
                let (width, height, is_sbs) = match layout {
                    Some(layout) => (
                        layout.eye_width as i32,
                        layout.eye_height as i32,
                        layout.is_sbs,
                    ),
                    None => (0, 0, false),
                };

                let mut layer_views: [xr::CompositionLayerProjectionView<xr::OpenGL>; VIEW_COUNT] = [
                    xr::CompositionLayerProjectionView::new(),
                    xr::CompositionLayerProjectionView::new(),
                ];

                for i in 0..VIEW_COUNT {
                    let image_index = swapchains[i]
                        .acquire_image()
                        .map_err(|e| VrError::Adapter(format!("OpenXR acquire: {e:?}")))?;
                    swapchains[i]
                        .wait_image(xr::Duration::from_nanos(5_000_000))
                        .map_err(|e| VrError::Adapter(format!("OpenXR wait_image: {e:?}")))?;

                    if let Some(decoded) = last_decoded.as_ref() {
                        let tex = swapchain_images[i][image_index as usize];
                        let eye_width = width.max(0) as u32;
                        let eye_height = height.max(0) as u32;
                        let sbs_available = is_sbs
                            && decoded.width >= eye_width * 2
                            && decoded.height >= eye_height;
                        let src_offset_x = if sbs_available {
                            eye_width * i as u32
                        } else {
                            0
                        };
                        let copy_width = if sbs_available {
                            eye_width
                        } else {
                            decoded.width.min(eye_width)
                        };
                        let copy_height = decoded.height.min(eye_height);
                        unsafe {
                            gl.bind_texture(
                                glow::TEXTURE_2D,
                                std::mem::transmute::<u32, Option<glow::NativeTexture>>(tex),
                            );
                            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, decoded.width as i32);
                            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, src_offset_x as i32);
                            gl.tex_sub_image_2d(
                                glow::TEXTURE_2D,
                                0,
                                0,
                                0,
                                copy_width as i32,
                                copy_height as i32,
                                glow::RGBA,
                                glow::UNSIGNED_BYTE,
                                glow::PixelUnpackData::Slice(&decoded.data),
                            );
                            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
                        }
                    }

                    swapchains[i]
                        .release_image()
                        .map_err(|e| VrError::Adapter(format!("OpenXR release: {e:?}")))?;

                    if views.len() > i {
                        let sub_image = unsafe {
                            xr::SwapchainSubImage::from_raw(xr::sys::SwapchainSubImage {
                                swapchain: swapchains[i].as_raw(),
                                image_rect: xr::Rect2Di {
                                    offset: xr::Offset2Di { x: 0, y: 0 },
                                    extent: xr::Extent2Di { width, height },
                                },
                                image_array_index: 0,
                            })
                        };
                        layer_views[i] = xr::CompositionLayerProjectionView::new()
                            .pose(views[i].pose)
                            .fov(views[i].fov)
                            .sub_image(sub_image);
                    }
                }

                let layer = xr::CompositionLayerProjection::new()
                    .space(&reference_space)
                    .views(&layer_views);
                let layers: [&xr::CompositionLayerBase<xr::OpenGL>; 1] = [&layer];

                frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        xr::EnvironmentBlendMode::OPAQUE,
                        &layers,
                    )
                    .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
            }
        } else {
            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    xr::EnvironmentBlendMode::OPAQUE,
                    &[],
                )
                .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
        }
    }
}

fn run_vulkan(state: Arc<SharedState>) -> VrResult<()> {
    let entry = unsafe { xr::Entry::load() }
        .map_err(|e| VrError::Adapter(format!("OpenXR load failed: {e:?}")))?;
    let available_exts = entry
        .enumerate_extensions()
        .map_err(|e| VrError::Adapter(format!("OpenXR ext enumerate: {e:?}")))?;
    if !available_exts.khr_vulkan_enable {
        return Err(VrError::Unavailable(
            "OpenXR KHR_vulkan_enable not available".to_string(),
        ));
    }
    let mut exts = xr::ExtensionSet::default();
    exts.khr_vulkan_enable = true;
    if available_exts.khr_vulkan_enable2 {
        exts.khr_vulkan_enable2 = true;
    }
    if available_exts.ext_hand_tracking {
        exts.ext_hand_tracking = true;
    }

    let app_info = xr::ApplicationInfo {
        application_name: "Wavry",
        application_version: 1,
        engine_name: "Wavry",
        engine_version: 1,
        api_version: xr::Version::new(1, 0, 0),
    };
    let instance = entry
        .create_instance(&app_info, &exts, &[])
        .map_err(|e| VrError::Adapter(format!("OpenXR create_instance: {e:?}")))?;
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|e| VrError::Adapter(format!("OpenXR system: {e:?}")))?;

    let mut vk_ctx = VulkanContext::new(&instance, system)?;

    let create_info = xr::vulkan::SessionCreateInfo {
        instance: vk_ctx.instance.handle().as_raw() as *const _,
        physical_device: vk_ctx.physical_device.as_raw() as *const _,
        device: vk_ctx.device.handle().as_raw() as *const _,
        queue_family_index: vk_ctx.queue_family_index,
        queue_index: 0,
    };

    let (session, mut frame_waiter, mut frame_stream) = unsafe {
        instance
            .create_session::<xr::Vulkan>(system, &create_info)
            .map_err(|e| VrError::Adapter(format!("OpenXR create_session: {e:?}")))?
    };
    wavry_vr::set_pcvr_status("PCVR: Linux Wayland Vulkan runtime active".to_string());
    let mut input_actions = InputActions::new(&instance, &session).ok();
    let hand_tracking = if available_exts.ext_hand_tracking {
        HandTrackingState::new(&session).ok()
    } else {
        None
    };

    let reference_space = session
        .create_reference_space(
            xr::ReferenceSpaceType::LOCAL,
            xr::Posef {
                orientation: xr::Quaternionf {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                },
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            },
        )
        .map_err(|e| VrError::Adapter(format!("OpenXR reference space: {e:?}")))?;

    let mut event_buffer = xr::EventDataBuffer::new();
    let mut session_running = false;
    let mut decoder: Option<GstDecoder> = None;
    let mut swapchains: Option<[xr::Swapchain<xr::Vulkan>; VIEW_COUNT]> = None;
    let mut swapchain_images: Option<[Vec<vk::Image>; VIEW_COUNT]> = None;
    let mut image_layouts: Option<[Vec<vk::ImageLayout>; VIEW_COUNT]> = None;
    let mut last_decoded: Option<DecodedFrame> = None;
    let mut last_refresh_hz: Option<f32> = None;

    loop {
        while let Some(event) = instance
            .poll_event(&mut event_buffer)
            .map_err(|e| VrError::Adapter(format!("OpenXR poll_event: {e:?}")))?
        {
            if let xr::Event::SessionStateChanged(e) = event {
                match e.state() {
                    xr::SessionState::READY => {
                        session
                            .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                            .map_err(|e| {
                                VrError::Adapter(format!("OpenXR session begin: {e:?}"))
                            })?;
                        session_running = true;
                    }
                    xr::SessionState::STOPPING => {
                        session
                            .end()
                            .map_err(|e| VrError::Adapter(format!("OpenXR session end: {e:?}")))?;
                        session_running = false;
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if state.stop.load(Ordering::Relaxed) {
            if session_running {
                let _ = session.end();
            }
            return Ok(());
        }

        if !session_running {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let frame_state = frame_waiter
            .wait()
            .map_err(|e| VrError::Adapter(format!("OpenXR wait: {e:?}")))?;
        frame_stream
            .begin()
            .map_err(|e| VrError::Adapter(format!("OpenXR begin: {e:?}")))?;

        let (_view_state, views) = session
            .locate_views(
                xr::ViewConfigurationType::PRIMARY_STEREO,
                frame_state.predicted_display_time,
                &reference_space,
            )
            .map_err(|e| VrError::Adapter(format!("OpenXR locate_views: {e:?}")))?;

        if !views.is_empty() {
            let pose = to_pose(views[0].pose);
            let timestamp_us = (frame_state.predicted_display_time.as_nanos() / 1_000) as u64;
            state.callbacks.on_pose_update(pose, timestamp_us);
            if let Some(actions) = input_actions.as_mut() {
                if let Ok(inputs) = actions.poll(&session, timestamp_us) {
                    for input in inputs {
                        state.callbacks.on_gamepad_input(input);
                    }
                }
            }
            if let Some(tracking) = hand_tracking.as_ref() {
                for hand_pose in tracking.poll(&reference_space, frame_state.predicted_display_time)
                {
                    state.callbacks.on_hand_pose_update(hand_pose, timestamp_us);
                }
            }
        }

        let period_ns = frame_state.predicted_display_period.as_nanos();
        if period_ns > 0 {
            let refresh_hz = 1_000_000_000.0 / period_ns as f32;
            let send = last_refresh_hz.is_none_or(|prev| (prev - refresh_hz).abs() > 0.1);
            if send {
                state.callbacks.on_vr_timing(VrTiming {
                    refresh_hz,
                    vsync_offset_us: 0,
                });
                last_refresh_hz = Some(refresh_hz);
            }
        }

        if decoder.is_none() {
            if let Some(cfg) = state.stream_config.lock().ok().and_then(|c| *c) {
                decoder = Some(GstDecoder::new(cfg)?);
            }
        }

        if let Some(frame) = state.take_latest_frame() {
            if let Some(decoder) = decoder.as_ref() {
                if let Some(decoded) = decoder.decode(&frame.data, frame.timestamp_us)? {
                    last_decoded = Some(decoded);
                }
            }
        }

        if swapchains.is_none() {
            if let Some(cfg) = state.stream_config.lock().ok().and_then(|c| *c) {
                let layout = eye_layout(cfg);
                let formats = session
                    .enumerate_swapchain_formats()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain formats: {e:?}")))?;
                let (format, format_name, format_srgb) = choose_vk_swapchain_format(&formats);
                log_swapchain_validation_u32(
                    &instance,
                    "linux-vulkan",
                    &formats,
                    format,
                    format_name,
                    format_srgb,
                );

                let create_info = xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                    format,
                    sample_count: 1,
                    width: layout.eye_width,
                    height: layout.eye_height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                };
                let sc0 = session
                    .create_swapchain(&create_info)
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain: {e:?}")))?;
                let sc1 = session
                    .create_swapchain(&create_info)
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain: {e:?}")))?;
                let imgs0 = sc0
                    .enumerate_images()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain images: {e:?}")))?;
                let imgs1 = sc1
                    .enumerate_images()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain images: {e:?}")))?;

                let imgs0: Vec<vk::Image> = imgs0.into_iter().map(vk::Image::from_raw).collect();
                let imgs1: Vec<vk::Image> = imgs1.into_iter().map(vk::Image::from_raw).collect();
                let layouts0 = vec![vk::ImageLayout::UNDEFINED; imgs0.len()];
                let layouts1 = vec![vk::ImageLayout::UNDEFINED; imgs1.len()];

                swapchains = Some([sc0, sc1]);
                swapchain_images = Some([imgs0, imgs1]);
                image_layouts = Some([layouts0, layouts1]);
            }
        }

        if frame_state.should_render {
            if let (Some(swapchains), Some(swapchain_images), Some(image_layouts)) = (
                swapchains.as_mut(),
                swapchain_images.as_ref(),
                image_layouts.as_mut(),
            ) {
                let cfg = state.stream_config.lock().ok().and_then(|c| *c);
                let layout = cfg.map(eye_layout);
                let (width, height, is_sbs) = match layout {
                    Some(layout) => (
                        layout.eye_width as i32,
                        layout.eye_height as i32,
                        layout.is_sbs,
                    ),
                    None => (0, 0, false),
                };

                let mut layer_views: [xr::CompositionLayerProjectionView<xr::Vulkan>; VIEW_COUNT] = [
                    xr::CompositionLayerProjectionView::new(),
                    xr::CompositionLayerProjectionView::new(),
                ];

                for i in 0..VIEW_COUNT {
                    let image_index = swapchains[i]
                        .acquire_image()
                        .map_err(|e| VrError::Adapter(format!("OpenXR acquire: {e:?}")))?;
                    swapchains[i]
                        .wait_image(xr::Duration::from_nanos(5_000_000))
                        .map_err(|e| VrError::Adapter(format!("OpenXR wait_image: {e:?}")))?;

                    if let Some(decoded) = last_decoded.as_ref() {
                        let image = swapchain_images[i][image_index as usize];
                        let old_layout = image_layouts[i][image_index as usize];
                        let eye_width = width.max(0) as u32;
                        let eye_height = height.max(0) as u32;
                        let sbs_available = is_sbs
                            && decoded.width >= eye_width * 2
                            && decoded.height >= eye_height;
                        let src_offset_x = if sbs_available {
                            eye_width * i as u32
                        } else {
                            0
                        };
                        let copy_width = if sbs_available {
                            eye_width
                        } else {
                            decoded.width.min(eye_width)
                        };
                        let copy_height = decoded.height.min(eye_height);
                        let new_layout = vk_ctx.upload_rgba_region(
                            image,
                            old_layout,
                            decoded.width,
                            decoded.height,
                            src_offset_x,
                            copy_width,
                            copy_height,
                            &decoded.data,
                        )?;
                        image_layouts[i][image_index as usize] = new_layout;
                    }

                    swapchains[i]
                        .release_image()
                        .map_err(|e| VrError::Adapter(format!("OpenXR release: {e:?}")))?;

                    if views.len() > i {
                        let sub_image = unsafe {
                            xr::SwapchainSubImage::from_raw(xr::sys::SwapchainSubImage {
                                swapchain: swapchains[i].as_raw(),
                                image_rect: xr::Rect2Di {
                                    offset: xr::Offset2Di { x: 0, y: 0 },
                                    extent: xr::Extent2Di { width, height },
                                },
                                image_array_index: 0,
                            })
                        };
                        layer_views[i] = xr::CompositionLayerProjectionView::new()
                            .pose(views[i].pose)
                            .fov(views[i].fov)
                            .sub_image(sub_image);
                    }
                }

                let layer = xr::CompositionLayerProjection::new()
                    .space(&reference_space)
                    .views(&layer_views);
                let layers: [&xr::CompositionLayerBase<xr::Vulkan>; 1] = [&layer];

                frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        xr::EnvironmentBlendMode::OPAQUE,
                        &layers,
                    )
                    .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
            }
        } else {
            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    xr::EnvironmentBlendMode::OPAQUE,
                    &[],
                )
                .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
        }
    }
}
