use std::sync::Arc;
use std::thread::JoinHandle;

use wavry_vr::{VrError, VrResult};

use crate::vendor::SharedState;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::env;
    use std::ffi::CString;
    use std::str::FromStr;
    use std::ptr;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::{Duration, Instant};

    use ash::vk::Handle;
    use ash::{vk, Entry as VkEntry};
    use glow::HasContext;
    use openxr as xr;
    use x11::{glx, xlib};

    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;

    use wavry_vr::types::{
        GamepadAxis, GamepadButton, GamepadInput, HandPose, Pose, StreamConfig, VideoCodec,
        VrTiming,
    };

    const VIEW_COUNT: usize = 2;
    const INPUT_SEND_INTERVAL: Duration = Duration::from_millis(20);
    const AXIS_EPS: f32 = 0.01;
    const STICK_DEADZONE: f32 = 0.05;
    const VULKAN_FENCE_SKIP_LIMIT: u32 = 2;
    const VULKAN_FENCE_WAIT_TIMEOUT_NS: u64 = 1_000_000;

    #[derive(Clone, Copy, Default)]
    struct GamepadSnapshot {
        axes: [f32; 4],
        buttons: [bool; 2],
        active: bool,
    }

    struct InputActions {
        action_set: xr::ActionSet,
        trigger: xr::Action<f32>,
        trigger_click: xr::Action<bool>,
        grip: xr::Action<f32>,
        grip_click: xr::Action<bool>,
        stick: xr::Action<xr::Vector2f>,
        primary: xr::Action<bool>,
        secondary: xr::Action<bool>,
        left: xr::Path,
        right: xr::Path,
        last_sent: [GamepadSnapshot; 2],
        last_sent_at: [Instant; 2],
    }

    impl InputActions {
        fn new<G>(instance: &xr::Instance, session: &xr::Session<G>) -> VrResult<Self> {
            let action_set = instance
                .create_action_set("wavry", "Wavry", 0)
                .map_err(|e| VrError::Adapter(format!("OpenXR action set: {e:?}")))?;

            let left = instance
                .string_to_path("/user/hand/left")
                .map_err(|e| VrError::Adapter(format!("OpenXR path left: {e:?}")))?;
            let right = instance
                .string_to_path("/user/hand/right")
                .map_err(|e| VrError::Adapter(format!("OpenXR path right: {e:?}")))?;
            let subaction_paths = [left, right];

            let trigger = action_set
                .create_action("trigger", "Trigger", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger: {e:?}")))?;
            let trigger_click = action_set
                .create_action("trigger_click", "Trigger Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger_click: {e:?}")))?;
            let grip = action_set
                .create_action("grip", "Grip", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip: {e:?}")))?;
            let grip_click = action_set
                .create_action("grip_click", "Grip Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip_click: {e:?}")))?;
            let stick = action_set
                .create_action("thumbstick", "Thumbstick", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action thumbstick: {e:?}")))?;
            let primary = action_set
                .create_action("primary", "Primary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action primary: {e:?}")))?;
            let secondary = action_set
                .create_action("secondary", "Secondary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action secondary: {e:?}")))?;

            let profile_paths = [
                "/interaction_profiles/khr/simple_controller",
                "/interaction_profiles/oculus/touch_controller",
                "/interaction_profiles/valve/index_controller",
                "/interaction_profiles/microsoft/motion_controller",
                "/interaction_profiles/htc/vive_controller",
            ];

            for profile in profile_paths {
                let profile_path = instance
                    .string_to_path(profile)
                    .map_err(|e| VrError::Adapter(format!("OpenXR profile path: {e:?}")))?;
                let bindings = Self::bindings_for_profile(
                    instance,
                    profile,
                    &trigger,
                    &trigger_click,
                    &grip,
                    &grip_click,
                    &stick,
                    &primary,
                    &secondary,
                )?;
                if let Err(err) =
                    instance.suggest_interaction_profile_bindings(profile_path, &bindings)
                {
                    eprintln!(
                        "OpenXR binding suggestion rejected for {}: {:?}",
                        profile, err
                    );
                }
            }

            session
                .attach_action_sets(&[&action_set])
                .map_err(|e| VrError::Adapter(format!("OpenXR attach actions: {e:?}")))?;

            Ok(Self {
                action_set,
                trigger,
                trigger_click,
                grip,
                grip_click,
                stick,
                primary,
                secondary,
                left,
                right,
                last_sent: [GamepadSnapshot::default(), GamepadSnapshot::default()],
                last_sent_at: [Instant::now(), Instant::now()],
            })
        }

        fn bindings_for_profile<'a>(
            instance: &'a xr::Instance,
            profile: &'a str,
            trigger: &'a xr::Action<f32>,
            trigger_click: &'a xr::Action<bool>,
            grip: &'a xr::Action<f32>,
            grip_click: &'a xr::Action<bool>,
            stick: &'a xr::Action<xr::Vector2f>,
            primary: &'a xr::Action<bool>,
            secondary: &'a xr::Action<bool>,
        ) -> VrResult<Vec<xr::Binding<'a>>> {
            let mut bindings = Vec::with_capacity(24);
            macro_rules! bind_f32 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_vec2 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_bool {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }

            match profile {
                "/interaction_profiles/khr/simple_controller" => {
                    bind_bool!(trigger_click, "/user/hand/left/input/select/click");
                    bind_bool!(trigger_click, "/user/hand/right/input/select/click");
                    bind_bool!(primary, "/user/hand/left/input/select/click");
                    bind_bool!(primary, "/user/hand/right/input/select/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                "/interaction_profiles/oculus/touch_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                    bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_bool!(primary, "/user/hand/left/input/x/click");
                    bind_bool!(primary, "/user/hand/right/input/a/click");
                    bind_bool!(secondary, "/user/hand/left/input/y/click");
                    bind_bool!(secondary, "/user/hand/right/input/b/click");
                }
                "/interaction_profiles/valve/index_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                    bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_bool!(primary, "/user/hand/left/input/a/click");
                    bind_bool!(primary, "/user/hand/right/input/a/click");
                    bind_bool!(secondary, "/user/hand/left/input/b/click");
                    bind_bool!(secondary, "/user/hand/right/input/b/click");
                }
                "/interaction_profiles/microsoft/motion_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                    bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/left/input/trackpad");
                    bind_vec2!(stick, "/user/hand/right/input/trackpad");
                    bind_bool!(primary, "/user/hand/left/input/thumbstick/click");
                    bind_bool!(primary, "/user/hand/right/input/thumbstick/click");
                    bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                    bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                "/interaction_profiles/htc/vive_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                    bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                    bind_vec2!(stick, "/user/hand/left/input/trackpad");
                    bind_vec2!(stick, "/user/hand/right/input/trackpad");
                    bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                    bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                _ => {}
            }

            Ok(bindings)
        }

        fn poll<G>(
            &mut self,
            session: &xr::Session<G>,
            timestamp_us: u64,
        ) -> VrResult<Vec<GamepadInput>> {
            session
                .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])
                .map_err(|e| VrError::Adapter(format!("OpenXR sync actions: {e:?}")))?;

            let mut outputs = Vec::new();
            let now = Instant::now();
            let hands = [(self.left, 0usize), (self.right, 1usize)];

            for (path, index) in hands {
                let trigger = self.trigger.state(session, path).ok();
                let trigger_click = self.trigger_click.state(session, path).ok();
                let grip = self.grip.state(session, path).ok();
                let grip_click = self.grip_click.state(session, path).ok();
                let stick = self.stick.state(session, path).ok();
                let primary = self.primary.state(session, path).ok();
                let secondary = self.secondary.state(session, path).ok();

                let active = trigger.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || trigger_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || stick.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || primary.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || secondary.as_ref().map(|s| s.is_active).unwrap_or(false);

                let mut axes = [0.0f32; 4];
                let mut buttons = [false; 2];
                if active {
                    let trigger_val = trigger.map(|s| s.current_state).unwrap_or(0.0).max(
                        if trigger_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let grip_val = grip.map(|s| s.current_state).unwrap_or(0.0).max(
                        if grip_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let stick_val = stick
                        .map(|s| s.current_state)
                        .unwrap_or(xr::Vector2f { x: 0.0, y: 0.0 });
                    let stick_x = if stick_val.x.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.x
                    };
                    let stick_y = if stick_val.y.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.y
                    };
                    axes = [stick_x, stick_y, trigger_val, grip_val];
                    buttons = [
                        primary.map(|s| s.current_state).unwrap_or(false),
                        secondary.map(|s| s.current_state).unwrap_or(false),
                    ];
                }

                let snapshot = GamepadSnapshot {
                    axes,
                    buttons,
                    active,
                };

                let should_send = Self::should_send(
                    snapshot,
                    self.last_sent[index],
                    now,
                    self.last_sent_at[index],
                );
                if should_send {
                    self.last_sent[index] = snapshot;
                    self.last_sent_at[index] = now;

                    let axes_out = vec![
                        GamepadAxis {
                            axis: 0,
                            value: axes[0],
                        },
                        GamepadAxis {
                            axis: 1,
                            value: axes[1],
                        },
                        GamepadAxis {
                            axis: 2,
                            value: axes[2],
                        },
                        GamepadAxis {
                            axis: 3,
                            value: axes[3],
                        },
                    ];
                    let buttons_out = vec![
                        GamepadButton {
                            button: 0,
                            pressed: buttons[0],
                        },
                        GamepadButton {
                            button: 1,
                            pressed: buttons[1],
                        },
                    ];
                    outputs.push(GamepadInput {
                        timestamp_us,
                        gamepad_id: index as u32,
                        axes: axes_out,
                        buttons: buttons_out,
                    });
                }
            }

            Ok(outputs)
        }

        fn should_send(
            current: GamepadSnapshot,
            last: GamepadSnapshot,
            now: Instant,
            last_sent_at: Instant,
        ) -> bool {
            if current.active || last.active {
                if now.duration_since(last_sent_at) >= INPUT_SEND_INTERVAL {
                    return true;
                }
                for i in 0..current.axes.len() {
                    if (current.axes[i] - last.axes[i]).abs() > AXIS_EPS {
                        return true;
                    }
                }
                for i in 0..current.buttons.len() {
                    if current.buttons[i] != last.buttons[i] {
                        return true;
                    }
                }
            }
            false
        }
    }

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
            let colormap =
                xlib::XCreateColormap(display, root, (*visual_info).visual, xlib::AllocNone);

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

            let context = glx::glXCreateNewContext(
                display,
                fb_config,
                glx::GLX_RGBA_TYPE,
                ptr::null_mut(),
                1,
            );
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
                VideoCodec::Av1 => {
                    "video/x-av1,stream-format=(string)obu-stream,alignment=(string)tu"
                }
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

    // Wayland PCVR path: minimal Vulkan upload pipeline for OpenXR swapchains.
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
                .map_err(|e| {
                    VrError::Adapter(format!("OpenXR Vulkan instance extensions: {e:?}"))
                })?;
            let instance_exts = parse_extension_list(&instance_exts);
            let instance_ext_ptrs: Vec<*const i8> =
                instance_exts.iter().map(|s| s.as_ptr()).collect();

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
                xr_instance
                    .vulkan_graphics_device(system, instance.handle().as_raw() as *const _)
                    .map_err(|e| {
                        VrError::Adapter(format!("OpenXR Vulkan graphics device: {e:?}"))
                    })?
            };
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
                    .map_err(|e| {
                        VrError::Adapter(format!("Vulkan command pool create failed: {e}"))
                    })?
            };

            let command_buffer_info = vk::CommandBufferAllocateInfo::builder()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let command_buffer = unsafe {
                device
                    .allocate_command_buffers(&command_buffer_info)
                    .map_err(|e| {
                        VrError::Adapter(format!("Vulkan command buffer alloc failed: {e}"))
                    })?
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
                            return Err(VrError::Adapter(format!(
                                "Vulkan fence wait failed: {err}"
                            )));
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
                    .map_err(|e| {
                        VrError::Adapter(format!("Vulkan end command buffer failed: {e}"))
                    })?;

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
        let families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
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

    pub(super) fn spawn(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
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
        // Prefer linear UNORM for video passthrough to avoid implicit gamma transforms.
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
        if let Some(format) = preferred.iter().copied().find(|&fmt| formats.contains(&fmt)) {
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
        if selected_srgb {
            eprintln!(
                "OpenXR swapchain validation [{}]: selected sRGB fallback; monitor scene gamma for double/under correction.",
                backend
            );
        }
    }

    fn log_swapchain_validation_i64(
        instance: &xr::Instance,
        backend: &str,
        available: &[i64],
        selected: i64,
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
            "OpenXR swapchain validation [{}]: runtime='{}' selected={} ({}) gamma_mode={} available={:?}",
            backend, runtime_name, selected_name, selected, gamma_mode, available
        );
        if selected_srgb {
            eprintln!(
                "OpenXR swapchain validation [{}]: selected sRGB fallback; monitor scene gamma for double/under correction.",
                backend
            );
        }
    }

    struct EyeLayout {
        eye_width: u32,
        eye_height: u32,
        is_sbs: bool,
    }

    fn eye_layout(cfg: StreamConfig) -> EyeLayout {
        let width = cfg.width as u32;
        let height = cfg.height as u32;
        let is_sbs = width >= height * 2 && width % 2 == 0;
        let eye_width = if is_sbs { width / 2 } else { width };
        EyeLayout {
            eye_width,
            eye_height: height,
            is_sbs,
        }
    }

    fn to_pose(pose: xr::Posef) -> Pose {
        Pose {
            position: [pose.position.x, pose.position.y, pose.position.z],
            orientation: [
                pose.orientation.x,
                pose.orientation.y,
                pose.orientation.z,
                pose.orientation.w,
            ],
        }
    }

    struct HandTrackingState {
        left: xr::HandTracker,
        right: xr::HandTracker,
    }

    impl HandTrackingState {
        fn new<G>(session: &xr::Session<G>) -> VrResult<Self> {
            let left = session
                .create_hand_tracker(xr::Hand::LEFT)
                .map_err(|e| VrError::Adapter(format!("OpenXR create hand tracker left: {e:?}")))?;
            let right = session.create_hand_tracker(xr::Hand::RIGHT).map_err(|e| {
                VrError::Adapter(format!("OpenXR create hand tracker right: {e:?}"))
            })?;
            Ok(Self { left, right })
        }

        fn poll(&self, reference_space: &xr::Space, time: xr::Time) -> Vec<HandPose> {
            let mut out = Vec::with_capacity(2);
            if let Ok(Some((locations, velocities))) =
                reference_space.relate_hand_joints(&self.left, time)
            {
                if let Some(hand) = hand_pose_from_joints(0, &locations, &velocities) {
                    out.push(hand);
                }
            }
            if let Ok(Some((locations, velocities))) =
                reference_space.relate_hand_joints(&self.right, time)
            {
                if let Some(hand) = hand_pose_from_joints(1, &locations, &velocities) {
                    out.push(hand);
                }
            }
            out
        }
    }

    fn hand_pose_from_joints(
        hand_id: u32,
        locations: &xr::HandJointLocations,
        velocities: &xr::HandJointVelocities,
    ) -> Option<HandPose> {
        let palm_location = locations[xr::HandJoint::PALM];
        let has_position = palm_location
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID);
        let has_orientation = palm_location
            .location_flags
            .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
        if !has_position || !has_orientation {
            return None;
        }

        let palm_velocity = velocities[xr::HandJoint::PALM];
        let linear_velocity = if palm_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
        {
            [
                palm_velocity.linear_velocity.x,
                palm_velocity.linear_velocity.y,
                palm_velocity.linear_velocity.z,
            ]
        } else {
            [0.0; 3]
        };
        let angular_velocity = if palm_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
        {
            [
                palm_velocity.angular_velocity.x,
                palm_velocity.angular_velocity.y,
                palm_velocity.angular_velocity.z,
            ]
        } else {
            [0.0; 3]
        };

        Some(HandPose {
            hand_id,
            pose: to_pose(palm_location.pose),
            linear_velocity,
            angular_velocity,
        })
    }

    // X11 path: OpenXR + OpenGL via GLX.
    fn run_glx(state: Arc<SharedState>) -> VrResult<()> {
        let glx = unsafe { GlxContext::new()? };
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let s = CString::new(s).unwrap();
                unsafe { std::mem::transmute(glx::glXGetProcAddress(s.as_ptr() as *const u8)) }
            })
        };
        unsafe {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        }

        let entry = xr::Entry::load()
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
                            session.end().map_err(|e| {
                                VrError::Adapter(format!("OpenXR session end: {e:?}"))
                            })?;
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
                    for hand_pose in
                        tracking.poll(&reference_space, frame_state.predicted_display_time)
                    {
                        state.callbacks.on_hand_pose_update(hand_pose, timestamp_us);
                    }
                }
            }

            let period_ns = frame_state.predicted_display_period.as_nanos();
            if period_ns > 0 {
                let refresh_hz = 1_000_000_000.0 / period_ns as f32;
                let send = last_refresh_hz.map_or(true, |prev| (prev - refresh_hz).abs() > 0.1);
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
                    let formats = session.enumerate_swapchain_formats().map_err(|e| {
                        VrError::Adapter(format!("OpenXR swapchain formats: {e:?}"))
                    })?;
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
                            gl.bind_texture(glow::TEXTURE_2D, std::mem::transmute(*tex));
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

            if frame_state.should_render && swapchains.is_some() && swapchain_images.is_some() {
                let swapchains = swapchains.as_mut().unwrap();
                let swapchain_images = swapchain_images.as_ref().unwrap();
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
                            gl.bind_texture(glow::TEXTURE_2D, std::mem::transmute(tex));
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

    // Wayland path: OpenXR + Vulkan (GLX is X11-only).
    fn run_vulkan(state: Arc<SharedState>) -> VrResult<()> {
        let entry = xr::Entry::load()
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
                            session.end().map_err(|e| {
                                VrError::Adapter(format!("OpenXR session end: {e:?}"))
                            })?;
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
                    for hand_pose in
                        tracking.poll(&reference_space, frame_state.predicted_display_time)
                    {
                        state.callbacks.on_hand_pose_update(hand_pose, timestamp_us);
                    }
                }
            }

            let period_ns = frame_state.predicted_display_period.as_nanos();
            if period_ns > 0 {
                let refresh_hz = 1_000_000_000.0 / period_ns as f32;
                let send = last_refresh_hz.map_or(true, |prev| (prev - refresh_hz).abs() > 0.1);
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
                    let formats = session.enumerate_swapchain_formats().map_err(|e| {
                        VrError::Adapter(format!("OpenXR swapchain formats: {e:?}"))
                    })?;
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

                    let imgs0: Vec<vk::Image> = imgs0
                        .into_iter()
                        .map(|img| vk::Image::from_raw(img))
                        .collect();
                    let imgs1: Vec<vk::Image> = imgs1
                        .into_iter()
                        .map(|img| vk::Image::from_raw(img))
                        .collect();
                    let layouts0 = vec![vk::ImageLayout::UNDEFINED; imgs0.len()];
                    let layouts1 = vec![vk::ImageLayout::UNDEFINED; imgs1.len()];

                    swapchains = Some([sc0, sc1]);
                    swapchain_images = Some([imgs0, imgs1]);
                    image_layouts = Some([layouts0, layouts1]);
                }
            }

            if frame_state.should_render && swapchains.is_some() && swapchain_images.is_some() {
                let swapchains = swapchains.as_mut().unwrap();
                let swapchain_images = swapchain_images.as_ref().unwrap();
                let image_layouts = image_layouts.as_mut().unwrap();
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

    fn run(state: Arc<SharedState>) -> VrResult<()> {
        let is_wayland = env::var_os("WAYLAND_DISPLAY").is_some()
            || matches!(env::var("XDG_SESSION_TYPE"), Ok(ref v) if v == "wayland");

        if is_wayland {
            // Wayland requires Vulkan OpenXR on Linux (no GLX).
            run_vulkan(state)
        } else {
            run_glx(state)
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::{Duration, Instant};

    use openxr as xr;
    use wavry_vr::types::{
        GamepadAxis, GamepadButton, GamepadInput, HandPose, Pose, StreamConfig, VideoCodec,
        VrTiming,
    };
    use windows::core::ComInterface;
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;

    const VIEW_COUNT: usize = 2;
    const INPUT_SEND_INTERVAL: Duration = Duration::from_millis(20);
    const AXIS_EPS: f32 = 0.01;
    const STICK_DEADZONE: f32 = 0.05;

    #[derive(Clone, Copy, Default)]
    struct GamepadSnapshot {
        axes: [f32; 4],
        buttons: [bool; 2],
        active: bool,
    }

    struct InputActions {
        action_set: xr::ActionSet,
        trigger: xr::Action<f32>,
        trigger_click: xr::Action<bool>,
        grip: xr::Action<f32>,
        grip_click: xr::Action<bool>,
        stick: xr::Action<xr::Vector2f>,
        primary: xr::Action<bool>,
        secondary: xr::Action<bool>,
        left: xr::Path,
        right: xr::Path,
        last_sent: [GamepadSnapshot; 2],
        last_sent_at: [Instant; 2],
    }

    impl InputActions {
        fn new<G>(instance: &xr::Instance, session: &xr::Session<G>) -> VrResult<Self> {
            let action_set = instance
                .create_action_set("wavry", "Wavry", 0)
                .map_err(|e| VrError::Adapter(format!("OpenXR action set: {e:?}")))?;

            let left = instance
                .string_to_path("/user/hand/left")
                .map_err(|e| VrError::Adapter(format!("OpenXR path left: {e:?}")))?;
            let right = instance
                .string_to_path("/user/hand/right")
                .map_err(|e| VrError::Adapter(format!("OpenXR path right: {e:?}")))?;
            let subaction_paths = [left, right];

            let trigger = action_set
                .create_action("trigger", "Trigger", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger: {e:?}")))?;
            let trigger_click = action_set
                .create_action("trigger_click", "Trigger Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger_click: {e:?}")))?;
            let grip = action_set
                .create_action("grip", "Grip", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip: {e:?}")))?;
            let grip_click = action_set
                .create_action("grip_click", "Grip Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip_click: {e:?}")))?;
            let stick = action_set
                .create_action("thumbstick", "Thumbstick", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action thumbstick: {e:?}")))?;
            let primary = action_set
                .create_action("primary", "Primary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action primary: {e:?}")))?;
            let secondary = action_set
                .create_action("secondary", "Secondary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action secondary: {e:?}")))?;

            let profile_paths = [
                "/interaction_profiles/khr/simple_controller",
                "/interaction_profiles/oculus/touch_controller",
                "/interaction_profiles/valve/index_controller",
                "/interaction_profiles/microsoft/motion_controller",
                "/interaction_profiles/htc/vive_controller",
            ];

            for profile in profile_paths {
                let profile_path = instance
                    .string_to_path(profile)
                    .map_err(|e| VrError::Adapter(format!("OpenXR profile path: {e:?}")))?;
                let bindings = Self::bindings_for_profile(
                    instance,
                    profile,
                    &trigger,
                    &trigger_click,
                    &grip,
                    &grip_click,
                    &stick,
                    &primary,
                    &secondary,
                )?;
                if let Err(err) =
                    instance.suggest_interaction_profile_bindings(profile_path, &bindings)
                {
                    eprintln!(
                        "OpenXR binding suggestion rejected for {}: {:?}",
                        profile, err
                    );
                }
            }

            session
                .attach_action_sets(&[&action_set])
                .map_err(|e| VrError::Adapter(format!("OpenXR attach actions: {e:?}")))?;

            Ok(Self {
                action_set,
                trigger,
                trigger_click,
                grip,
                grip_click,
                stick,
                primary,
                secondary,
                left,
                right,
                last_sent: [GamepadSnapshot::default(), GamepadSnapshot::default()],
                last_sent_at: [Instant::now(), Instant::now()],
            })
        }

        fn bindings_for_profile<'a>(
            instance: &'a xr::Instance,
            profile: &'a str,
            trigger: &'a xr::Action<f32>,
            trigger_click: &'a xr::Action<bool>,
            grip: &'a xr::Action<f32>,
            grip_click: &'a xr::Action<bool>,
            stick: &'a xr::Action<xr::Vector2f>,
            primary: &'a xr::Action<bool>,
            secondary: &'a xr::Action<bool>,
        ) -> VrResult<Vec<xr::Binding<'a>>> {
            let mut bindings = Vec::with_capacity(24);
            macro_rules! bind_f32 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_vec2 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_bool {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }

            match profile {
                "/interaction_profiles/khr/simple_controller" => {
                    bind_bool!(trigger_click, "/user/hand/left/input/select/click");
                    bind_bool!(trigger_click, "/user/hand/right/input/select/click");
                    bind_bool!(primary, "/user/hand/left/input/select/click");
                    bind_bool!(primary, "/user/hand/right/input/select/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                "/interaction_profiles/oculus/touch_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                    bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_bool!(primary, "/user/hand/left/input/x/click");
                    bind_bool!(primary, "/user/hand/right/input/a/click");
                    bind_bool!(secondary, "/user/hand/left/input/y/click");
                    bind_bool!(secondary, "/user/hand/right/input/b/click");
                }
                "/interaction_profiles/valve/index_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                    bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_bool!(primary, "/user/hand/left/input/a/click");
                    bind_bool!(primary, "/user/hand/right/input/a/click");
                    bind_bool!(secondary, "/user/hand/left/input/b/click");
                    bind_bool!(secondary, "/user/hand/right/input/b/click");
                }
                "/interaction_profiles/microsoft/motion_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                    bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/left/input/trackpad");
                    bind_vec2!(stick, "/user/hand/right/input/trackpad");
                    bind_bool!(primary, "/user/hand/left/input/thumbstick/click");
                    bind_bool!(primary, "/user/hand/right/input/thumbstick/click");
                    bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                    bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                "/interaction_profiles/htc/vive_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_bool!(grip_click, "/user/hand/left/input/squeeze/click");
                    bind_bool!(grip_click, "/user/hand/right/input/squeeze/click");
                    bind_vec2!(stick, "/user/hand/left/input/trackpad");
                    bind_vec2!(stick, "/user/hand/right/input/trackpad");
                    bind_bool!(primary, "/user/hand/left/input/trackpad/click");
                    bind_bool!(primary, "/user/hand/right/input/trackpad/click");
                    bind_bool!(secondary, "/user/hand/left/input/menu/click");
                    bind_bool!(secondary, "/user/hand/right/input/menu/click");
                }
                _ => {}
            }

            Ok(bindings)
        }

        fn poll<G>(
            &mut self,
            session: &xr::Session<G>,
            timestamp_us: u64,
        ) -> VrResult<Vec<GamepadInput>> {
            session
                .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])
                .map_err(|e| VrError::Adapter(format!("OpenXR sync actions: {e:?}")))?;

            let mut outputs = Vec::new();
            let now = Instant::now();
            let hands = [(self.left, 0usize), (self.right, 1usize)];

            for (path, index) in hands {
                let trigger = self.trigger.state(session, path).ok();
                let trigger_click = self.trigger_click.state(session, path).ok();
                let grip = self.grip.state(session, path).ok();
                let grip_click = self.grip_click.state(session, path).ok();
                let stick = self.stick.state(session, path).ok();
                let primary = self.primary.state(session, path).ok();
                let secondary = self.secondary.state(session, path).ok();

                let active = trigger.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || trigger_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || stick.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || primary.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || secondary.as_ref().map(|s| s.is_active).unwrap_or(false);

                let mut axes = [0.0f32; 4];
                let mut buttons = [false; 2];
                if active {
                    let trigger_val = trigger.map(|s| s.current_state).unwrap_or(0.0).max(
                        if trigger_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let grip_val = grip.map(|s| s.current_state).unwrap_or(0.0).max(
                        if grip_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let stick_val = stick
                        .map(|s| s.current_state)
                        .unwrap_or(xr::Vector2f { x: 0.0, y: 0.0 });
                    let stick_x = if stick_val.x.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.x
                    };
                    let stick_y = if stick_val.y.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.y
                    };
                    axes = [stick_x, stick_y, trigger_val, grip_val];
                    buttons = [
                        primary.map(|s| s.current_state).unwrap_or(false),
                        secondary.map(|s| s.current_state).unwrap_or(false),
                    ];
                }

                let snapshot = GamepadSnapshot {
                    axes,
                    buttons,
                    active,
                };

                let should_send = Self::should_send(
                    snapshot,
                    self.last_sent[index],
                    now,
                    self.last_sent_at[index],
                );
                if should_send {
                    self.last_sent[index] = snapshot;
                    self.last_sent_at[index] = now;

                    let axes_out = vec![
                        GamepadAxis {
                            axis: 0,
                            value: axes[0],
                        },
                        GamepadAxis {
                            axis: 1,
                            value: axes[1],
                        },
                        GamepadAxis {
                            axis: 2,
                            value: axes[2],
                        },
                        GamepadAxis {
                            axis: 3,
                            value: axes[3],
                        },
                    ];
                    let buttons_out = vec![
                        GamepadButton {
                            button: 0,
                            pressed: buttons[0],
                        },
                        GamepadButton {
                            button: 1,
                            pressed: buttons[1],
                        },
                    ];
                    outputs.push(GamepadInput {
                        timestamp_us,
                        gamepad_id: index as u32,
                        axes: axes_out,
                        buttons: buttons_out,
                    });
                }
            }

            Ok(outputs)
        }

        fn should_send(
            current: GamepadSnapshot,
            last: GamepadSnapshot,
            now: Instant,
            last_sent_at: Instant,
        ) -> bool {
            if current.active || last.active {
                if now.duration_since(last_sent_at) >= INPUT_SEND_INTERVAL {
                    return true;
                }
                for i in 0..current.axes.len() {
                    if (current.axes[i] - last.axes[i]).abs() > AXIS_EPS {
                        return true;
                    }
                }
                for i in 0..current.buttons.len() {
                    if current.buttons[i] != last.buttons[i] {
                        return true;
                    }
                }
            }
            false
        }
    }

    struct EyeLayout {
        eye_width: u32,
        eye_height: u32,
        is_sbs: bool,
    }

    fn eye_layout(cfg: StreamConfig) -> EyeLayout {
        let width = cfg.width as u32;
        let height = cfg.height as u32;
        let is_sbs = width >= height * 2 && width % 2 == 0;
        let eye_width = if is_sbs { width / 2 } else { width };
        EyeLayout {
            eye_width,
            eye_height: height,
            is_sbs,
        }
    }

    fn describe_dxgi_swapchain_format(format: i64) -> (&'static str, bool) {
        if format == DXGI_FORMAT_B8G8R8A8_UNORM.0 as i64 {
            ("DXGI_FORMAT_B8G8R8A8_UNORM", false)
        } else if format == DXGI_FORMAT_R8G8B8A8_UNORM.0 as i64 {
            ("DXGI_FORMAT_R8G8B8A8_UNORM", false)
        } else if format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as i64 {
            ("DXGI_FORMAT_B8G8R8A8_UNORM_SRGB", true)
        } else if format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as i64 {
            ("DXGI_FORMAT_R8G8B8A8_UNORM_SRGB", true)
        } else {
            ("UNKNOWN_DXGI_FORMAT", false)
        }
    }

    fn choose_dxgi_swapchain_format(formats: &[i64]) -> (i64, &'static str, bool) {
        let preferred = [
            DXGI_FORMAT_B8G8R8A8_UNORM.0 as i64,
            DXGI_FORMAT_R8G8B8A8_UNORM.0 as i64,
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as i64,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as i64,
        ];
        if let Some(format) = preferred.iter().copied().find(|fmt| formats.contains(fmt)) {
            let (name, srgb) = describe_dxgi_swapchain_format(format);
            return (format, name, srgb);
        }
        let fallback = formats
            .first()
            .copied()
            .unwrap_or(DXGI_FORMAT_B8G8R8A8_UNORM.0 as i64);
        let (name, srgb) = describe_dxgi_swapchain_format(fallback);
        (fallback, name, srgb)
    }

    fn log_swapchain_validation(
        instance: &xr::Instance,
        available: &[i64],
        selected: i64,
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
            "OpenXR swapchain validation [windows-d3d11]: runtime='{}' selected={} ({}) gamma_mode={} available={:?}",
            runtime_name, selected_name, selected, gamma_mode, available
        );
        if selected_srgb {
            eprintln!(
                "OpenXR swapchain validation [windows-d3d11]: selected sRGB fallback; monitor scene gamma for double/under correction."
            );
        }
    }

    struct MfDecoder {
        decoder: IMFTransform,
    }

    impl MfDecoder {
        fn new(device: &ID3D11Device, codec: VideoCodec) -> VrResult<Self> {
            unsafe {
                MFStartup(MF_VERSION, MFSTARTUP_FULL)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MFStartup failed: {e:?}")))?;

                let input_guid = match codec {
                    VideoCodec::Av1 => MFVideoFormat_AV1,
                    VideoCodec::Hevc => MFVideoFormat_HEVC,
                    VideoCodec::H264 => MFVideoFormat_H264,
                };

                let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
                let mut count = 0;
                let input_type = MFT_REGISTER_TYPE_INFO {
                    guidMajorType: MFMediaType_Video,
                    guidSubtype: input_guid,
                };

                MFTEnumEx(
                    MFT_CATEGORY_VIDEO_DECODER,
                    MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                    Some(&input_type),
                    None,
                    &mut activate_list,
                    &mut count,
                )
                .ok()
                .map_err(|e| VrError::Adapter(format!("MFTEnumEx failed: {e:?}")))?;

                if count == 0 {
                    return Err(VrError::Adapter("No hardware decoders found".to_string()));
                }

                let activate = std::slice::from_raw_parts(activate_list, count as usize)[0]
                    .as_ref()
                    .ok_or_else(|| VrError::Adapter("Decoder activation failed".to_string()))?;
                let decoder: IMFTransform = activate
                    .ActivateObject()
                    .map_err(|e| VrError::Adapter(format!("ActivateObject failed: {e:?}")))?;

                CoTaskMemFree(Some(activate_list as _));

                let input_type: IMFMediaType = MFCreateMediaType()
                    .map_err(|e| VrError::Adapter(format!("MFCreateMediaType: {e:?}")))?;
                input_type
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set input major: {e:?}")))?;
                input_type
                    .SetGUID(&MF_MT_SUBTYPE, &input_guid)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set input subtype: {e:?}")))?;
                decoder
                    .SetInputType(0, Some(&input_type), 0)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set input type: {e:?}")))?;

                let output_type: IMFMediaType = MFCreateMediaType()
                    .map_err(|e| VrError::Adapter(format!("MFCreateMediaType: {e:?}")))?;
                output_type
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set output major: {e:?}")))?;
                output_type
                    .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set output subtype: {e:?}")))?;
                decoder
                    .SetOutputType(0, Some(&output_type), 0)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set output type: {e:?}")))?;

                if let Ok(attributes) = decoder.GetAttributes() {
                    let _ = attributes.SetUINT32(&MF_SA_D3D11_AWARE, 1);
                }

                let mut device_manager = None;
                let mut reset_token = 0;
                MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MFCreateDXGIDeviceManager: {e:?}")))?;
                let device_manager = device_manager
                    .ok_or_else(|| VrError::Adapter("MF device manager missing".to_string()))?;
                device_manager
                    .ResetDevice(device, reset_token)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF ResetDevice failed: {e:?}")))?;
                decoder
                    .ProcessMessage(
                        MFT_MESSAGE_SET_D3D_MANAGER,
                        device_manager.as_raw() as usize,
                    )
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF set D3D manager: {e:?}")))?;

                Ok(Self { decoder })
            }
        }

        fn decode(&self, payload: &[u8], timestamp_us: u64) -> VrResult<Option<ID3D11Texture2D>> {
            unsafe {
                let buffer = MFCreateMemoryBuffer(payload.len() as u32)
                    .map_err(|e| VrError::Adapter(format!("MFCreateMemoryBuffer: {e:?}")))?;
                let mut ptr = std::ptr::null_mut();
                let mut max_length = 0;
                let mut current_length = 0;
                buffer
                    .Lock(&mut ptr, Some(&mut max_length), Some(&mut current_length))
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF buffer lock: {e:?}")))?;
                std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
                buffer
                    .Unlock()
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF buffer unlock: {e:?}")))?;
                buffer
                    .SetCurrentLength(payload.len() as u32)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF buffer len: {e:?}")))?;

                let sample = MFCreateSample()
                    .map_err(|e| VrError::Adapter(format!("MFCreateSample: {e:?}")))?;
                sample
                    .AddBuffer(&buffer)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF AddBuffer: {e:?}")))?;
                sample
                    .SetSampleTime(timestamp_us as i64 * 10)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF SetSampleTime: {e:?}")))?;

                self.decoder
                    .ProcessInput(0, Some(&sample), 0)
                    .ok()
                    .map_err(|e| VrError::Adapter(format!("MF ProcessInput: {e:?}")))?;

                let mut output = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: None,
                    dwStatus: 0,
                    pEvents: None,
                };
                let mut status = 0;
                match self.decoder.ProcessOutput(0, 1, &mut output, &mut status) {
                    Ok(_) => {
                        if let Some(sample) = output.pSample {
                            let buffer = sample.GetBufferByIndex(0).map_err(|e| {
                                VrError::Adapter(format!("MF GetBufferByIndex: {e:?}"))
                            })?;
                            let access: IDirect3DDxgiInterfaceAccess = buffer
                                .cast()
                                .map_err(|e| VrError::Adapter(format!("MF buffer cast: {e:?}")))?;
                            let texture: ID3D11Texture2D = access
                                .GetInterface()
                                .map_err(|e| VrError::Adapter(format!("MF GetInterface: {e:?}")))?;
                            Ok(Some(texture))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
                    Err(e) if e.code() == E_FAIL => Ok(None),
                    Err(e) => Err(VrError::Adapter(format!("MF ProcessOutput: {e:?}"))),
                }
            }
        }
    }

    pub(super) fn spawn(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
        thread::Builder::new()
            .name("wavry-pcvr-windows".to_string())
            .spawn(move || {
                wavry_vr::set_pcvr_status("PCVR: starting (Windows runtime)".to_string());
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

    fn to_pose(pose: xr::Posef) -> Pose {
        Pose {
            position: [pose.position.x, pose.position.y, pose.position.z],
            orientation: [
                pose.orientation.x,
                pose.orientation.y,
                pose.orientation.z,
                pose.orientation.w,
            ],
        }
    }

    struct HandTrackingState {
        left: xr::HandTracker,
        right: xr::HandTracker,
    }

    impl HandTrackingState {
        fn new<G>(session: &xr::Session<G>) -> VrResult<Self> {
            let left = session
                .create_hand_tracker(xr::Hand::LEFT)
                .map_err(|e| VrError::Adapter(format!("OpenXR create hand tracker left: {e:?}")))?;
            let right = session.create_hand_tracker(xr::Hand::RIGHT).map_err(|e| {
                VrError::Adapter(format!("OpenXR create hand tracker right: {e:?}"))
            })?;
            Ok(Self { left, right })
        }

        fn poll(&self, reference_space: &xr::Space, time: xr::Time) -> Vec<HandPose> {
            let mut out = Vec::with_capacity(2);
            if let Ok(Some((locations, velocities))) =
                reference_space.relate_hand_joints(&self.left, time)
            {
                if let Some(hand) = hand_pose_from_joints(0, &locations, &velocities) {
                    out.push(hand);
                }
            }
            if let Ok(Some((locations, velocities))) =
                reference_space.relate_hand_joints(&self.right, time)
            {
                if let Some(hand) = hand_pose_from_joints(1, &locations, &velocities) {
                    out.push(hand);
                }
            }
            out
        }
    }

    fn hand_pose_from_joints(
        hand_id: u32,
        locations: &xr::HandJointLocations,
        velocities: &xr::HandJointVelocities,
    ) -> Option<HandPose> {
        let palm_location = locations[xr::HandJoint::PALM];
        let has_position = palm_location
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID);
        let has_orientation = palm_location
            .location_flags
            .contains(xr::SpaceLocationFlags::ORIENTATION_VALID);
        if !has_position || !has_orientation {
            return None;
        }

        let palm_velocity = velocities[xr::HandJoint::PALM];
        let linear_velocity = if palm_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::LINEAR_VALID)
        {
            [
                palm_velocity.linear_velocity.x,
                palm_velocity.linear_velocity.y,
                palm_velocity.linear_velocity.z,
            ]
        } else {
            [0.0; 3]
        };
        let angular_velocity = if palm_velocity
            .velocity_flags
            .contains(xr::SpaceVelocityFlags::ANGULAR_VALID)
        {
            [
                palm_velocity.angular_velocity.x,
                palm_velocity.angular_velocity.y,
                palm_velocity.angular_velocity.z,
            ]
        } else {
            [0.0; 3]
        };

        Some(HandPose {
            hand_id,
            pose: to_pose(palm_location.pose),
            linear_velocity,
            angular_velocity,
        })
    }

    fn run(state: Arc<SharedState>) -> VrResult<()> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| VrError::Adapter(format!("CoInitializeEx: {e:?}")))?;
        }

        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| VrError::Adapter(format!("D3D11CreateDevice: {e:?}")))?;
        }
        let device = device.ok_or_else(|| VrError::Adapter("D3D11 device missing".to_string()))?;
        let context =
            context.ok_or_else(|| VrError::Adapter("D3D11 context missing".to_string()))?;

        let entry = xr::Entry::load()
            .map_err(|e| VrError::Adapter(format!("OpenXR load failed: {e:?}")))?;
        let available_exts = entry
            .enumerate_extensions()
            .map_err(|e| VrError::Adapter(format!("OpenXR ext enumerate: {e:?}")))?;
        if !available_exts.khr_d3d11_enable {
            return Err(VrError::Unavailable(
                "OpenXR KHR_d3d11_enable not available".to_string(),
            ));
        }
        let mut exts = xr::ExtensionSet::default();
        exts.khr_d3d11_enable = true;
        if available_exts.ext_hand_tracking {
            exts.ext_hand_tracking = true;
        }

        let app_info = xr::ApplicationInfo {
            application_name: "Wavry",
            application_version: 1,
            engine_name: "Wavry",
            engine_version: 1,
        };
        let instance = entry
            .create_instance(&app_info, &exts, &[])
            .map_err(|e| VrError::Adapter(format!("OpenXR create_instance: {e:?}")))?;
        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|e| VrError::Adapter(format!("OpenXR system: {e:?}")))?;

        let create_info = xr::d3d::SessionCreateInfo {
            device: device.as_raw(),
        };

        let (session, mut frame_waiter, mut frame_stream) = unsafe {
            instance
                .create_session::<xr::D3D11>(system, &create_info)
                .map_err(|e| VrError::Adapter(format!("OpenXR create_session: {e:?}")))?
        };
        wavry_vr::set_pcvr_status("PCVR: Windows D3D11 runtime active".to_string());
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
        let mut decoder: Option<MfDecoder> = None;
        let mut swapchains: Option<[xr::Swapchain<xr::D3D11>; VIEW_COUNT]> = None;
        let mut swapchain_images: Option<[Vec<*mut ID3D11Texture2D>; VIEW_COUNT]> = None;
        let mut last_texture: Option<ID3D11Texture2D> = None;
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
                            session.end().map_err(|e| {
                                VrError::Adapter(format!("OpenXR session end: {e:?}"))
                            })?;
                            session_running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            unsafe { CoUninitialize() };
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
                unsafe { CoUninitialize() };
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
                    for hand_pose in
                        tracking.poll(&reference_space, frame_state.predicted_display_time)
                    {
                        state.callbacks.on_hand_pose_update(hand_pose, timestamp_us);
                    }
                }
            }

            let period_ns = frame_state.predicted_display_period.as_nanos();
            if period_ns > 0 {
                let refresh_hz = 1_000_000_000.0 / period_ns as f32;
                let send = last_refresh_hz.map_or(true, |prev| (prev - refresh_hz).abs() > 0.1);
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
                    decoder = Some(MfDecoder::new(&device, cfg.codec)?);
                }
            }

            if let Some(frame) = state.take_latest_frame() {
                if let Some(decoder) = decoder.as_ref() {
                    if let Some(texture) = decoder.decode(&frame.data, frame.timestamp_us)? {
                        last_texture = Some(texture);
                    }
                }
            }

            if swapchains.is_none() {
                if let Some(cfg) = state.stream_config.lock().ok().and_then(|c| *c) {
                    let layout = eye_layout(cfg);
                    let formats = session.enumerate_swapchain_formats().map_err(|e| {
                        VrError::Adapter(format!("OpenXR swapchain formats: {e:?}"))
                    })?;
                    let (format, format_name, format_srgb) = choose_dxgi_swapchain_format(&formats);
                    log_swapchain_validation(&instance, &formats, format, format_name, format_srgb);

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
                    swapchains = Some([sc0, sc1]);
                    swapchain_images = Some([imgs0, imgs1]);
                }
            }

            let mut layer_views: [xr::CompositionLayerProjectionView<xr::D3D11>; VIEW_COUNT] = [
                xr::CompositionLayerProjectionView::new(),
                xr::CompositionLayerProjectionView::new(),
            ];

            if frame_state.should_render && swapchains.is_some() && swapchain_images.is_some() {
                let swapchains = swapchains.as_mut().unwrap();
                let swapchain_images = swapchain_images.as_ref().unwrap();
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

                for i in 0..VIEW_COUNT {
                    let image_index = swapchains[i]
                        .acquire_image()
                        .map_err(|e| VrError::Adapter(format!("OpenXR acquire: {e:?}")))?;
                    swapchains[i]
                        .wait_image(xr::Duration::from_nanos(5_000_000))
                        .map_err(|e| VrError::Adapter(format!("OpenXR wait_image: {e:?}")))?;

                    if let Some(tex) = last_texture.as_ref() {
                        let target = swapchain_images[i][image_index as usize];
                        unsafe {
                            let target =
                                windows::core::from_raw_borrowed::<ID3D11Texture2D>(&target)
                                    .ok_or_else(|| {
                                        VrError::Adapter("Invalid swapchain texture".to_string())
                                    })?;
                            if is_sbs && width > 0 && height > 0 {
                                let eye_width = width as u32;
                                let eye_height = height as u32;
                                let src_left = eye_width * i as u32;
                                let src_right = src_left + eye_width;
                                let src_box = D3D11_BOX {
                                    left: src_left,
                                    top: 0,
                                    front: 0,
                                    right: src_right,
                                    bottom: eye_height,
                                    back: 1,
                                };
                                context.CopySubresourceRegion(
                                    target,
                                    0,
                                    0,
                                    0,
                                    0,
                                    tex,
                                    0,
                                    Some(&src_box),
                                );
                            } else {
                                context.CopyResource(target, tex);
                            }
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
            }

            let layer = xr::CompositionLayerProjection::new()
                .space(&reference_space)
                .views(&layer_views);
            let layers: [&xr::CompositionLayerBase<xr::D3D11>; 1] = [&layer];

            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    xr::EnvironmentBlendMode::OPAQUE,
                    &layers,
                )
                .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use ash::vk::{self, Handle};
    use ash::Entry as VkEntry;
    use openxr as xr;
    use std::ffi::CString;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::{Duration, Instant};
    use wavry_vr::types::{GamepadAxis, GamepadButton, GamepadInput};

    const INPUT_SEND_INTERVAL: Duration = Duration::from_millis(20);
    const AXIS_EPS: f32 = 0.01;
    const STICK_DEADZONE: f32 = 0.05;

    #[derive(Clone, Copy, Default)]
    struct GamepadSnapshot {
        axes: [f32; 4],
        buttons: [bool; 2],
        active: bool,
    }

    struct InputActions {
        action_set: xr::ActionSet,
        trigger: xr::Action<f32>,
        trigger_click: xr::Action<bool>,
        grip: xr::Action<f32>,
        grip_click: xr::Action<bool>,
        stick: xr::Action<xr::Vector2f>,
        primary: xr::Action<bool>,
        secondary: xr::Action<bool>,
        left: xr::Path,
        right: xr::Path,
        last_sent: [GamepadSnapshot; 2],
        last_sent_at: [Instant; 2],
    }

    impl InputActions {
        fn new<G>(instance: &xr::Instance, session: &xr::Session<G>) -> VrResult<Self> {
            let action_set = instance
                .create_action_set("wavry", "Wavry", 0)
                .map_err(|e| VrError::Adapter(format!("OpenXR action set: {e:?}")))?;

            let left = instance
                .string_to_path("/user/hand/left")
                .map_err(|e| VrError::Adapter(format!("OpenXR path left: {e:?}")))?;
            let right = instance
                .string_to_path("/user/hand/right")
                .map_err(|e| VrError::Adapter(format!("OpenXR path right: {e:?}")))?;
            let subaction_paths = [left, right];

            let trigger = action_set
                .create_action("trigger", "Trigger", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger: {e:?}")))?;
            let trigger_click = action_set
                .create_action("trigger_click", "Trigger Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action trigger_click: {e:?}")))?;
            let grip = action_set
                .create_action("grip", "Grip", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip: {e:?}")))?;
            let grip_click = action_set
                .create_action("grip_click", "Grip Click", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action grip_click: {e:?}")))?;
            let stick = action_set
                .create_action("thumbstick", "Thumbstick", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action thumbstick: {e:?}")))?;
            let primary = action_set
                .create_action("primary", "Primary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action primary: {e:?}")))?;
            let secondary = action_set
                .create_action("secondary", "Secondary", &subaction_paths)
                .map_err(|e| VrError::Adapter(format!("OpenXR action secondary: {e:?}")))?;

            let profile_paths = [
                "/interaction_profiles/oculus/touch_controller",
                "/interaction_profiles/khr/simple_controller",
            ];

            for profile in profile_paths {
                let profile_path = instance
                    .string_to_path(profile)
                    .map_err(|e| VrError::Adapter(format!("OpenXR profile path: {e:?}")))?;
                let bindings = Self::bindings_for_profile(
                    instance,
                    profile,
                    &trigger,
                    &trigger_click,
                    &grip,
                    &grip_click,
                    &stick,
                    &primary,
                    &secondary,
                )?;
                if let Err(err) =
                    instance.suggest_interaction_profile_bindings(profile_path, &bindings)
                {
                    eprintln!(
                        "OpenXR binding suggestion rejected for {}: {:?}",
                        profile, err
                    );
                }
            }

            session
                .attach_action_sets(&[&action_set])
                .map_err(|e| VrError::Adapter(format!("OpenXR attach actions: {e:?}")))?;

            Ok(Self {
                action_set,
                trigger,
                trigger_click,
                grip,
                grip_click,
                stick,
                primary,
                secondary,
                left,
                right,
                last_sent: [GamepadSnapshot::default(), GamepadSnapshot::default()],
                last_sent_at: [Instant::now(), Instant::now()],
            })
        }

        fn bindings_for_profile<'a>(
            instance: &'a xr::Instance,
            profile: &'a str,
            trigger: &'a xr::Action<f32>,
            trigger_click: &'a xr::Action<bool>,
            grip: &'a xr::Action<f32>,
            grip_click: &'a xr::Action<bool>,
            stick: &'a xr::Action<xr::Vector2f>,
            primary: &'a xr::Action<bool>,
            secondary: &'a xr::Action<bool>,
        ) -> VrResult<Vec<xr::Binding<'a>>> {
            let mut bindings = Vec::with_capacity(24);
            macro_rules! bind_f32 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_vec2 {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }
            macro_rules! bind_bool {
                ($action:expr, $path:expr) => {
                    if let Ok(path) = instance.string_to_path($path) {
                        bindings.push(xr::Binding::new($action, path));
                    }
                };
            }

            match profile {
                "/interaction_profiles/oculus/touch_controller" => {
                    bind_f32!(trigger, "/user/hand/left/input/trigger/value");
                    bind_f32!(trigger, "/user/hand/right/input/trigger/value");
                    bind_f32!(grip, "/user/hand/left/input/squeeze/value");
                    bind_f32!(grip, "/user/hand/right/input/squeeze/value");
                    bind_vec2!(stick, "/user/hand/left/input/thumbstick");
                    bind_vec2!(stick, "/user/hand/right/input/thumbstick");
                    bind_bool!(primary, "/user/hand/left/input/x/click");
                    bind_bool!(primary, "/user/hand/right/input/a/click");
                    bind_bool!(secondary, "/user/hand/left/input/y/click");
                    bind_bool!(secondary, "/user/hand/right/input/b/click");
                }
                "/interaction_profiles/khr/simple_controller" => {
                    bind_bool!(trigger_click, "/user/hand/left/input/select/click");
                    bind_bool!(trigger_click, "/user/hand/right/input/select/click");
                    bind_bool!(primary, "/user/hand/left/input/menu/click");
                    bind_bool!(primary, "/user/hand/right/input/menu/click");
                }
                _ => {}
            }

            Ok(bindings)
        }

        fn poll<G>(
            &mut self,
            session: &xr::Session<G>,
            timestamp_us: u64,
        ) -> VrResult<Vec<GamepadInput>> {
            session
                .sync_actions(&[xr::ActiveActionSet::new(&self.action_set)])
                .map_err(|e| VrError::Adapter(format!("OpenXR sync actions: {e:?}")))?;

            let mut outputs = Vec::new();
            let now = Instant::now();
            let hands = [(self.left, 0usize), (self.right, 1usize)];

            for (path, index) in hands {
                let trigger = self.trigger.state(session, path).ok();
                let trigger_click = self.trigger_click.state(session, path).ok();
                let grip = self.grip.state(session, path).ok();
                let grip_click = self.grip_click.state(session, path).ok();
                let stick = self.stick.state(session, path).ok();
                let primary = self.primary.state(session, path).ok();
                let secondary = self.secondary.state(session, path).ok();

                let active = trigger.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || trigger_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || grip_click.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || stick.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || primary.as_ref().map(|s| s.is_active).unwrap_or(false)
                    || secondary.as_ref().map(|s| s.is_active).unwrap_or(false);

                let mut axes = [0.0f32; 4];
                let mut buttons = [false; 2];
                if active {
                    let trigger_val = trigger.map(|s| s.current_state).unwrap_or(0.0).max(
                        if trigger_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let grip_val = grip.map(|s| s.current_state).unwrap_or(0.0).max(
                        if grip_click.map(|s| s.current_state).unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                    );
                    let stick_val = stick
                        .map(|s| s.current_state)
                        .unwrap_or(xr::Vector2f { x: 0.0, y: 0.0 });
                    let stick_x = if stick_val.x.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.x
                    };
                    let stick_y = if stick_val.y.abs() < STICK_DEADZONE {
                        0.0
                    } else {
                        stick_val.y
                    };
                    axes = [stick_x, stick_y, trigger_val, grip_val];
                    buttons = [
                        primary.map(|s| s.current_state).unwrap_or(false),
                        secondary.map(|s| s.current_state).unwrap_or(false),
                    ];
                }

                let snapshot = GamepadSnapshot {
                    axes,
                    buttons,
                    active,
                };

                let should_send = Self::should_send(
                    snapshot,
                    self.last_sent[index],
                    now,
                    self.last_sent_at[index],
                );
                if should_send {
                    self.last_sent[index] = snapshot;
                    self.last_sent_at[index] = now;

                    let axes_out = vec![
                        GamepadAxis {
                            axis: 0,
                            value: axes[0],
                        },
                        GamepadAxis {
                            axis: 1,
                            value: axes[1],
                        },
                        GamepadAxis {
                            axis: 2,
                            value: axes[2],
                        },
                        GamepadAxis {
                            axis: 3,
                            value: axes[3],
                        },
                    ];
                    let buttons_out = vec![
                        GamepadButton {
                            button: 0,
                            pressed: buttons[0],
                        },
                        GamepadButton {
                            button: 1,
                            pressed: buttons[1],
                        },
                    ];
                    outputs.push(GamepadInput {
                        timestamp_us,
                        gamepad_id: index as u32,
                        axes: axes_out,
                        buttons: buttons_out,
                    });
                }
            }

            Ok(outputs)
        }

        fn should_send(
            current: GamepadSnapshot,
            last: GamepadSnapshot,
            now: Instant,
            last_sent_at: Instant,
        ) -> bool {
            if current.active || last.active {
                if now.duration_since(last_sent_at) >= INPUT_SEND_INTERVAL {
                    return true;
                }
                for i in 0..current.axes.len() {
                    if (current.axes[i] - last.axes[i]).abs() > AXIS_EPS {
                        return true;
                    }
                }
                for i in 0..current.buttons.len() {
                    if current.buttons[i] != last.buttons[i] {
                        return true;
                    }
                }
            }
            false
        }
    }

    struct VulkanContext {
        instance: ash::Instance,
        device: ash::Device,
        physical_device: vk::PhysicalDevice,
        queue_family_index: u32,
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
                .map_err(|e| {
                    VrError::Adapter(format!("OpenXR Vulkan instance extensions: {e:?}"))
                })?;
            let instance_exts = parse_extension_list(&instance_exts);
            let instance_ext_ptrs: Vec<*const std::os::raw::c_char> =
                instance_exts.iter().map(|s| s.as_ptr() as *const std::os::raw::c_char).collect();

            let app_name = CString::new("Wavry").unwrap();
            let engine_name = CString::new("Wavry").unwrap();
            let app_info = vk::ApplicationInfo {
                p_application_name: app_name.as_ptr() as *const std::os::raw::c_char,
                application_version: 1,
                p_engine_name: engine_name.as_ptr() as *const std::os::raw::c_char,
                engine_version: 1,
                api_version,
                ..Default::default()
            };

            let create_info = vk::InstanceCreateInfo {
                p_application_info: &app_info,
                enabled_extension_count: instance_ext_ptrs.len() as u32,
                pp_enabled_extension_names: instance_ext_ptrs.as_ptr() as *const *const std::os::raw::c_char,
                ..Default::default()
            };

            let instance = unsafe {
                entry
                    .create_instance(&create_info, None)
                    .map_err(|e| VrError::Adapter(format!("Vulkan instance create failed: {e}")))?
            };

            let physical_device = unsafe {
                xr_instance
                    .vulkan_graphics_device(system, instance.handle().as_raw() as *const _)
                    .map_err(|e| {
                        VrError::Adapter(format!("OpenXR Vulkan graphics device: {e:?}"))
                    })?
            };
            let physical_device = vk::PhysicalDevice::from_raw(physical_device as u64);

            let queue_family_index = find_graphics_queue_family(&instance, physical_device)?;

            let device_exts = xr_instance
                .vulkan_legacy_device_extensions(system)
                .map_err(|e| VrError::Adapter(format!("OpenXR Vulkan device extensions: {e:?}")))?;
            let device_exts = parse_extension_list(&device_exts);
            let device_ext_ptrs: Vec<*const std::os::raw::c_char> =
                device_exts.iter().map(|s| s.as_ptr() as *const std::os::raw::c_char).collect();

            let priorities = [1.0f32];
            let queue_info = vk::DeviceQueueCreateInfo {
                queue_family_index,
                queue_count: 1,
                p_queue_priorities: priorities.as_ptr(),
                ..Default::default()
            };

            let device_create = vk::DeviceCreateInfo {
                queue_create_info_count: 1,
                p_queue_create_infos: &queue_info,
                enabled_extension_count: device_ext_ptrs.len() as u32,
                pp_enabled_extension_names: device_ext_ptrs.as_ptr() as *const *const std::os::raw::c_char,
                ..Default::default()
            };

            let device = unsafe {
                instance
                    .create_device(physical_device, &device_create, None)
                    .map_err(|e| VrError::Adapter(format!("Vulkan device create failed: {e}")))?
            };

            Ok(Self {
                instance,
                device,
                physical_device,
                queue_family_index,
            })
        }
    }

    impl Drop for VulkanContext {
        fn drop(&mut self) {
            unsafe {
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
        let families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        families
            .iter()
            .enumerate()
            .find(|(_, family)| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(idx, _)| idx as u32)
            .ok_or_else(|| VrError::Adapter("No Vulkan graphics queue family".to_string()))
    }

    pub(super) fn spawn(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
        thread::Builder::new()
            .name("wavry-pcvr-android".to_string())
            .spawn(move || {
                wavry_vr::set_pcvr_status("PCVR: starting (Android/Quest runtime)".to_string());
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

    fn run(state: Arc<SharedState>) -> VrResult<()> {
        log::info!("Quest/Android VR runtime started");

        let entry = unsafe { xr::Entry::load() }
            .map_err(|e| VrError::Adapter(format!("OpenXR load failed: {e:?}")))?;

        #[cfg(target_os = "android")]
        {
            entry.initialize_android_loader().map_err(|e| {
                VrError::Adapter(format!("OpenXR android loader init failed: {e:?}"))
            })?;
        }

        let mut exts = xr::ExtensionSet::default();
        exts.khr_vulkan_enable = true;
        exts.khr_android_create_instance = true;

        let app_info = xr::ApplicationInfo {
            application_name: "Wavry",
            application_version: 1,
            engine_name: "Wavry",
            engine_version: 1,
        };

        let instance = entry
            .create_instance(&app_info, &exts, &[])
            .map_err(|e| VrError::Adapter(format!("OpenXR create_instance: {e:?}")))?;

        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|e| VrError::Adapter(format!("OpenXR system: {e:?}")))?;

        let vk_ctx = VulkanContext::new(&instance, system)?;

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

        wavry_vr::set_pcvr_status("PCVR: Quest OpenXR active".to_string());

        let mut input_actions = InputActions::new(&instance, &session).ok();
        let mut event_buffer = xr::EventDataBuffer::new();
        let mut session_running = false;

        while !state.stop.load(Ordering::Relaxed) {
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
                            session.end().map_err(|e| {
                                VrError::Adapter(format!("OpenXR session end: {e:?}"))
                            })?;
                            session_running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }

            if !session_running {
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            let frame_state = frame_waiter
                .wait()
                .map_err(|e| VrError::Adapter(format!("OpenXR wait: {e:?}")))?;
            frame_stream
                .begin()
                .map_err(|e| VrError::Adapter(format!("OpenXR begin: {e:?}")))?;

            if let Some(actions) = input_actions.as_mut() {
                let timestamp_us = (frame_state.predicted_display_time.as_nanos() / 1_000) as u64;
                if let Ok(inputs) = actions.poll(&session, timestamp_us) {
                    for input in inputs {
                        state.callbacks.on_gamepad_input(input);
                    }
                }
            }

            // We must end the frame even if we don't render anything
            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    xr::EnvironmentBlendMode::OPAQUE,
                    &[],
                )
                .map_err(|e| VrError::Adapter(format!("OpenXR end: {e:?}")))?;
        }

        if session_running {
            let _ = session.end();
        }

        Ok(())
    }
}

pub(crate) fn spawn_runtime(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
    #[cfg(target_os = "linux")]
    {
        return linux::spawn(state);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::spawn(state);
    }
    #[cfg(target_os = "android")]
    {
        return android::spawn(state);
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    {
        let _ = state;
        Err(VrError::Unavailable(
            "ALVR PCVR adapter only supported on Linux, Windows and Android".to_string(),
        ))
    }
}
