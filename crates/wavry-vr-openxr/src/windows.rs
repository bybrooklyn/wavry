use std::mem::ManuallyDrop;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use openxr as xr;
use wavry_vr::types::{VideoCodec, VrTiming};
use wavry_vr::{VrError, VrResult};

use windows::core::Interface;
use windows::Win32::Foundation::{E_FAIL, HMODULE};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

use crate::common::{eye_layout, to_pose, HandTrackingState, InputActions};
use crate::SharedState;

const VIEW_COUNT: usize = 2;

fn describe_dxgi_swapchain_format(format: u32) -> (&'static str, bool) {
    let format_i32 = format as i32;
    if format_i32 == DXGI_FORMAT_B8G8R8A8_UNORM.0 {
        ("DXGI_FORMAT_B8G8R8A8_UNORM", false)
    } else if format_i32 == DXGI_FORMAT_R8G8B8A8_UNORM.0 {
        ("DXGI_FORMAT_R8G8B8A8_UNORM", false)
    } else if format_i32 == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 {
        ("DXGI_FORMAT_B8G8R8A8_UNORM_SRGB", true)
    } else if format_i32 == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 {
        ("DXGI_FORMAT_R8G8B8A8_UNORM_SRGB", true)
    } else {
        ("UNKNOWN_DXGI_FORMAT", false)
    }
}

fn choose_dxgi_swapchain_format(formats: &[u32]) -> (u32, &'static str, bool) {
    let preferred = [
        DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32,
        DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32,
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
    ];
    if let Some(format) = preferred.iter().copied().find(|fmt| formats.contains(fmt)) {
        let (name, srgb) = describe_dxgi_swapchain_format(format);
        return (format, name, srgb);
    }
    let fallback = formats
        .first()
        .copied()
        .unwrap_or(DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32);
    let (name, srgb) = describe_dxgi_swapchain_format(fallback);
    (fallback, name, srgb)
}

fn log_swapchain_validation(
    instance: &xr::Instance,
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
        "OpenXR swapchain validation [windows-d3d11]: runtime='{}' selected={} ({}) gamma_mode={} available={:?}",
        runtime_name, selected_name, selected, gamma_mode, available
    );
}

struct MfDecoder {
    decoder: IMFTransform,
}

impl MfDecoder {
    fn new(device: &ID3D11Device, codec: VideoCodec) -> VrResult<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|e| VrError::Adapter(format!("MFStartup failed: {e:?}")))?;

            let input_guid = match codec {
                VideoCodec::Av1 => MFVideoFormat_AV1,
                VideoCodec::Hevc => MFVideoFormat_HEVC,
                VideoCodec::H264 => MFVideoFormat_H264,
            };

            let mut count = 0;
            let input_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: input_guid,
            };

            let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_DECODER,
                MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0),
                Some(&input_type),
                None,
                &mut activate_list,
                &mut count,
            )
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

            let input_type_obj: IMFMediaType = MFCreateMediaType()
                .map_err(|e| VrError::Adapter(format!("MFCreateMediaType: {e:?}")))?;
            input_type_obj
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| VrError::Adapter(format!("MF set input major: {e:?}")))?;
            input_type_obj
                .SetGUID(&MF_MT_SUBTYPE, &input_guid)
                .map_err(|e| VrError::Adapter(format!("MF set input subtype: {e:?}")))?;
            decoder
                .SetInputType(0, Some(&input_type_obj), 0)
                .map_err(|e| VrError::Adapter(format!("MF set input type: {e:?}")))?;

            let output_type: IMFMediaType = MFCreateMediaType()
                .map_err(|e| VrError::Adapter(format!("MFCreateMediaType: {e:?}")))?;
            output_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| VrError::Adapter(format!("MF set output major: {e:?}")))?;
            output_type
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                .map_err(|e| VrError::Adapter(format!("MF set output subtype: {e:?}")))?;
            decoder
                .SetOutputType(0, Some(&output_type), 0)
                .map_err(|e| VrError::Adapter(format!("MF set output type: {e:?}")))?;

            if let Ok(attributes) = decoder.GetAttributes() {
                let _ = attributes.SetUINT32(&MF_SA_D3D11_AWARE, 1);
            }

            let mut device_manager = None;
            let mut reset_token = 0;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager)
                .map_err(|e| VrError::Adapter(format!("MFCreateDXGIDeviceManager: {e:?}")))?;
            let device_manager = device_manager
                .ok_or_else(|| VrError::Adapter("MF device manager missing".to_string()))?;
            device_manager
                .ResetDevice(device, reset_token)
                .map_err(|e| VrError::Adapter(format!("MF ResetDevice failed: {e:?}")))?;

            let manager_ptr = std::mem::transmute::<IMFDXGIDeviceManager, *mut std::ffi::c_void>(
                device_manager.clone(),
            );
            decoder
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager_ptr as usize)
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
                .map_err(|e| VrError::Adapter(format!("MF buffer lock: {e:?}")))?;
            std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
            buffer
                .Unlock()
                .map_err(|e| VrError::Adapter(format!("MF buffer unlock: {e:?}")))?;
            buffer
                .SetCurrentLength(payload.len() as u32)
                .map_err(|e| VrError::Adapter(format!("MF buffer len: {e:?}")))?;

            let sample =
                MFCreateSample().map_err(|e| VrError::Adapter(format!("MFCreateSample: {e:?}")))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| VrError::Adapter(format!("MF AddBuffer: {e:?}")))?;
            sample
                .SetSampleTime(timestamp_us as i64 * 10)
                .map_err(|e| VrError::Adapter(format!("MF SetSampleTime: {e:?}")))?;

            self.decoder
                .ProcessInput(0, Some(&sample), 0)
                .map_err(|e| VrError::Adapter(format!("MF ProcessInput: {e:?}")))?;

            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(None),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0;
            match self
                .decoder
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            {
                Ok(_) => {
                    if let Some(sample) = output.pSample.as_ref() {
                        let buffer = sample
                            .GetBufferByIndex(0)
                            .map_err(|e| VrError::Adapter(format!("MF GetBufferByIndex: {e:?}")))?;
                        let texture: ID3D11Texture2D = buffer.cast().map_err(|e| {
                            VrError::Adapter(format!("MF buffer cast to texture: {e:?}"))
                        })?;
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

pub fn spawn(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
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

fn run(state: Arc<SharedState>) -> VrResult<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
    }

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
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
    let context = context.ok_or_else(|| VrError::Adapter("D3D11 context missing".to_string()))?;

    let entry = unsafe { xr::Entry::load() }
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
        api_version: xr::Version::new(1, 0, 0),
    };
    let instance = entry
        .create_instance(&app_info, &exts, &[])
        .map_err(|e| VrError::Adapter(format!("OpenXR create_instance: {e:?}")))?;
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|e| VrError::Adapter(format!("OpenXR system: {e:?}")))?;

    let create_info = xr::d3d::SessionCreateInfoD3D11 {
        device: unsafe {
            std::mem::transmute::<ID3D11Device, *mut std::ffi::c_void>(device.clone())
        },
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
        .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
        .map_err(|e| VrError::Adapter(format!("OpenXR reference space: {e:?}")))?;

    let mut event_buffer = xr::EventDataBuffer::new();
    let mut session_running = false;
    let mut decoder: Option<MfDecoder> = None;
    let mut swapchains: Option<[xr::Swapchain<xr::D3D11>; VIEW_COUNT]> = None;
    let mut swapchain_images: Option<[Vec<ID3D11Texture2D>; VIEW_COUNT]> = None;
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
                        session
                            .end()
                            .map_err(|e| VrError::Adapter(format!("OpenXR session end: {e:?}")))?;
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
                let formats = session
                    .enumerate_swapchain_formats()
                    .map_err(|e| VrError::Adapter(format!("OpenXR swapchain formats: {e:?}")))?;
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

                let imgs0: Vec<ID3D11Texture2D> = imgs0
                    .into_iter()
                    .map(|p| unsafe { std::mem::transmute(p) })
                    .collect();
                let imgs1: Vec<ID3D11Texture2D> = imgs1
                    .into_iter()
                    .map(|p| unsafe { std::mem::transmute(p) })
                    .collect();

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

                let mut layer_views: [xr::CompositionLayerProjectionView<xr::D3D11>; VIEW_COUNT] = [
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

                    if let Some(texture) = last_texture.as_ref() {
                        let swapchain_texture = &swapchain_images[i][image_index as usize];
                        unsafe {
                            let eye_width = width.max(0) as u32;
                            let eye_height = height.max(0) as u32;
                            let src_offset_x = if is_sbs { eye_width * i as u32 } else { 0 };

                            let box_region = D3D11_BOX {
                                left: src_offset_x,
                                top: 0,
                                front: 0,
                                right: src_offset_x + eye_width,
                                bottom: eye_height,
                                back: 1,
                            };

                            context.CopySubresourceRegion(
                                swapchain_texture,
                                0,
                                0,
                                0,
                                0,
                                texture,
                                0,
                                Some(&box_region),
                            );
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
                let layers: [&xr::CompositionLayerBase<xr::D3D11>; 1] = [&layer];

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
