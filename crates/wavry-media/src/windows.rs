// Windows implementation for wavry-media
// Using Windows.Graphics.Capture (WGC) for high-performance screen capture.

use crate::{
    Codec, DecodeConfig, EncodeConfig, EncodedFrame, FrameData, FrameFormat, RawFrame, Renderer,
};
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, Stream, StreamConfig, SupportedBufferSize};
use libloading::{Library, Symbol};
use opus::{Application, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use crate::audio::{
    opus_frame_duration_us, AUDIO_MAX_BUFFER_SAMPLES, OPUS_BITRATE_BPS, OPUS_CHANNELS,
    OPUS_FRAME_SAMPLES, OPUS_MAX_FRAME_SAMPLES, OPUS_MAX_PACKET_BYTES, OPUS_SAMPLE_RATE,
};

#[cfg(target_os = "windows")]
use windows::{
    core::*, Foundation::*, Graphics::Capture::*, Graphics::DirectX::Direct3D11::IDirect3DDevice,
    Graphics::DirectX::DirectXPixelFormat, Win32::Foundation::*, Win32::Graphics::Direct3D::*,
    Win32::Graphics::Direct3D11::*, Win32::Graphics::Dxgi::Common::*, Win32::Graphics::Dxgi::*,
    Win32::Graphics::Gdi::*, Win32::Media::Audio::*, Win32::Media::MediaFoundation::*,
    Win32::System::Com::*, Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
    Win32::UI::Input::KeyboardAndMouse::*, Win32::UI::WindowsAndMessaging::GetDesktopWindow,
    Win32::UI::WindowsAndMessaging::EnumDisplayMonitors, Win32::UI::WindowsAndMessaging::GetMonitorInfoW,
    Win32::UI::WindowsAndMessaging::MONITORINFOEXW, Graphics::SizeInt32,
    Win32::Graphics::Dxgi::DXGI_PRESENT,
};

#[cfg(target_os = "windows")]
use std::mem::ManuallyDrop;

#[cfg(target_os = "windows")]
const MF_BGR32: GUID = GUID::from_u128(0x00000016_0000_0010_8000_00aa00389b71); // MFVideoFormat_RGB32
#[cfg(target_os = "windows")]
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
#[cfg(target_os = "windows")]
const KSDATAFORMAT_SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

#[cfg(target_os = "windows")]
fn pack_u64(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

#[cfg(target_os = "windows")]
unsafe fn MFSetAttributeSize(
    attributes: &IMFAttributes,
    guid: &GUID,
    width: u32,
    height: u32,
) -> Result<()> {
    attributes
        .SetUINT64(guid, pack_u64(width, height))
        .map_err(|e| anyhow!(e))?;
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn MFSetAttributeRatio(
    attributes: &IMFAttributes,
    guid: &GUID,
    numerator: u32,
    denominator: u32,
) -> Result<()> {
    attributes
        .SetUINT64(guid, pack_u64(numerator, denominator))
        .map_err(|e| anyhow!(e))?;
    Ok(())
}

#[cfg(target_os = "windows")]
extern "system" {
    fn CreateDirect3D11DeviceFromDXGIDevice(
        dxgidevice: *mut std::ffi::c_void,
        graphicsdevice: *mut *mut std::ffi::c_void,
    ) -> HRESULT;
}

#[cfg(target_os = "windows")]
fn create_direct3d_device(device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi_device: IDXGIDevice = device.cast()?;
    let mut inspectable = std::ptr::null_mut();
    unsafe {
        CreateDirect3D11DeviceFromDXGIDevice(dxgi_device.as_raw(), &mut inspectable).ok()?;
        let device: IDirect3DDevice = std::mem::transmute_copy(&inspectable);
        Ok(device)
    }
}

#[cfg(target_os = "windows")]
use windows::Win32::Media::MediaFoundation::*;

/// Windows screen encoder using Media Foundation
pub struct WindowsEncoder {
    config: EncodeConfig,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    capture_item: GraphicsCaptureItem,
    capture_session: GraphicsCaptureSession,
    frame_pool: Direct3D11CaptureFramePool,
    transform: IMFTransform,
    frame_width: u32,
    frame_height: u32,
}

#[cfg(target_os = "windows")]
unsafe impl Send for WindowsEncoder {}
#[cfg(target_os = "windows")]
unsafe impl Sync for WindowsEncoder {}

impl WindowsEncoder {
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        unsafe {
            // Initialize Media Foundation
            MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup failed")?;

            // Initialize D3D11 Device
            let mut device = None;
            let mut context = None;
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
            ).context("D3D11CreateDevice failed")?;
            let device = device.ok_or_else(|| anyhow!("Failed to create D3D11 device"))?;
            let context = context.ok_or_else(|| anyhow!("Failed to create D3D11 context"))?;

            // Get Target for capture
            let interop: IGraphicsCaptureItemInterop =
                windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let capture_item: GraphicsCaptureItem = if let Some(id) = config.display_id {
                interop.CreateForMonitor(HMONITOR(id as _))?
            } else {
                let hwnd = GetDesktopWindow();
                interop.CreateForWindow(hwnd)?
            };

            let item_size = capture_item.Size()?;
            let frame_width = item_size.Width.max(1) as u32;
            let frame_height = item_size.Height.max(1) as u32;

            // Setup WinRT Device
            let winrt_device = create_direct3d_device(&device)?;

            // Setup Frame Pool
            let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &winrt_device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                item_size,
            )?;

            let capture_session = frame_pool.CreateCaptureSession(&capture_item)?;
            capture_session.StartCapture()?;

            // Initialize Media Foundation Encoder (MFT)
            let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0;

            let mft_category = MFT_CATEGORY_VIDEO_ENCODER;
            let output_subtype = mf_subtype_for_codec(config.codec);
            let output_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: output_subtype,
            };

            MFTEnumEx(
                mft_category,
                MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 as u32 | MFT_ENUM_FLAG_SORTANDFILTER.0 as u32),
                None,
                Some(&output_type),
                &mut activate_list,
                &mut count,
            ).context("MFTEnumEx failed")?;

            if count == 0 {
                return Err(anyhow!("No hardware encoders found for {:?}", config.codec));
            }

            let activate = std::slice::from_raw_parts(activate_list, count as usize)[0]
                .as_ref()
                .unwrap();
            let transform: IMFTransform = activate.ActivateObject()?;

            // Set Output Type
            let output_media_type: IMFMediaType = MFCreateMediaType()?;
            output_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_media_type.SetGUID(&MF_MT_SUBTYPE, &output_subtype)?;
            output_media_type.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate_kbps * 1000)?;
            MFSetAttributeRatio(output_media_type.cast()?, &MF_MT_FRAME_RATE, config.fps as u32, 1)?;
            MFSetAttributeSize(
                output_media_type.cast()?,
                &MF_MT_FRAME_SIZE,
                frame_width,
                frame_height,
            )?;
            output_media_type
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;

            transform.SetOutputType(0, Some(&output_media_type), 0)?;

            // Set Input Type
            let input_media_type: IMFMediaType = MFCreateMediaType()?;
            input_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input_media_type.SetGUID(&MF_MT_SUBTYPE, &MF_BGR32)?; // B8G8R8A8 in MF is often BGR32
            MFSetAttributeSize(
                input_media_type.cast()?,
                &MF_MT_FRAME_SIZE,
                item_size.Width as u32,
                item_size.Height as u32,
            )?;
            MFSetAttributeRatio(input_media_type.cast()?, &MF_MT_FRAME_RATE, config.fps as u32, 1)?;

            transform.SetInputType(0, Some(&input_media_type), 0)?;

            // Enable D3D11 awareness
            if let Ok(attributes) = transform.GetAttributes() {
                attributes.SetUINT32(&MF_SA_D3D11_AWARE, 1)?;
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }

            let mut device_manager = None;
            let mut reset_token = 0;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager)?;
            let device_manager =
                device_manager.ok_or_else(|| anyhow!("Failed to create MF device manager"))?;
            device_manager.ResetDevice(&device, reset_token)?;

            transform.ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                device_manager.as_raw() as usize,
            )?;

            CoTaskMemFree(Some(activate_list as *const _));

            Ok(Self {
                config,
                device,
                context,
                capture_item,
                capture_session,
                frame_pool,
                transform,
                frame_width,
                frame_height,
            })
        }
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        unsafe {
            if let Ok(frame) = self.frame_pool.TryGetNextFrame() {
                let size = frame.ContentSize()?;
                let new_width = size.Width.max(1) as u32;
                let new_height = size.Height.max(1) as u32;
                if new_width != self.frame_width || new_height != self.frame_height {
                    self.reconfigure_frame_pool(new_width, new_height)?;
                    return Err(anyhow!("Capture size changed, reconfigured"));
                }
                let timestamp = frame.SystemRelativeTime()?.Duration as u64 / 10;
                let surface = frame.Surface()?;
                let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
                let texture: ID3D11Texture2D = access.GetInterface()?;

                // Wrap D3D11 texture in an MF Sample
                let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Resource::IID, &texture, 0, false)?;

                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(timestamp as i64 * 10)?;
                sample.SetSampleDuration(166666)?; // 60fps approx in 100ns units

                // Pass to MFT
                self.transform.ProcessInput(0, Some(&sample), 0)?;

                // Drain Output
                let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(None),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                };

                // For hardware encoders, we usually need to provide a sample with a buffer
                // unless it supports MF_TRANSFORM_OUTPUT_CAN_PROVIDE_SAMPLES
                let output_sample = MFCreateSample()?;
                let output_buffer = MFCreateMemoryBuffer(1024 * 1024)?; // 1MB buffer
                output_sample.AddBuffer(&output_buffer)?;
                output_data_buffer.pSample = ManuallyDrop::new(Some(output_sample));

                let mut status = 0;
                match self.transform.ProcessOutput(
                    0,
                    std::slice::from_mut(&mut output_data_buffer),
                    &mut status,
                ) {
                    Ok(_) => {
                        let sample = unsafe { (*output_data_buffer.pSample).as_ref().unwrap() };
                        let total_length = sample.GetTotalLength()?;
                        let mut data = vec![0u8; total_length as usize];

                        let buffer = sample.ConvertToContiguousBuffer()?;
                        let mut ptr = std::ptr::null_mut();
                        let mut current_length = 0;
                        buffer.Lock(&mut ptr, None, Some(&mut current_length))?;
                        std::ptr::copy_nonoverlapping(
                            ptr,
                            data.as_mut_ptr(),
                            current_length as usize,
                        );
                        buffer.Unlock()?;

                        let is_keyframe =
                            sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;

                        Ok(EncodedFrame {
                            timestamp_us: timestamp,
                            keyframe: is_keyframe,
                            data,
                        })
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                        Err(anyhow!("Need more input"))
                    }
                    Err(e) => Err(anyhow!("MFT ProcessOutput failed: {:?}", e)),
                }
            } else {
                Err(anyhow!("No frame available"))
            }
        }
    }
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        unsafe {
            let media_type: IMFMediaType = MFCreateMediaType()?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            let subtype = mf_subtype_for_codec(self.config.codec);
            media_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
            media_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_kbps * 1000)?;
            self.transform.SetOutputType(0, Some(&media_type), 0)?;
            Ok(())
        }
    }

    fn reconfigure_frame_pool(&mut self, width: u32, height: u32) -> Result<()> {
        let winrt_device = create_direct3d_device(&self.device)?;
        let new_size = SizeInt32 {
            Width: width as i32,
            Height: height as i32,
        };
        self.frame_pool.Recreate(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            new_size,
        )?;

        self.config.resolution.width = width as u16;
        self.config.resolution.height = height as u16;
        self.frame_width = width;
        self.frame_height = height;

        let output_media_type: IMFMediaType = MFCreateMediaType()?;
        output_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        output_media_type.SetGUID(&MF_MT_SUBTYPE, &mf_subtype_for_codec(self.config.codec))?;
        output_media_type.SetUINT32(&MF_MT_AVG_BITRATE, self.config.bitrate_kbps * 1000)?;
        MFSetAttributeRatio(output_media_type.cast()?, &MF_MT_FRAME_RATE, self.config.fps as u32, 1)?;
        MFSetAttributeSize(output_media_type.cast()?, &MF_MT_FRAME_SIZE, width, height)?;
        output_media_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        self.transform
            .SetOutputType(0, Some(&output_media_type), 0)?;

        let input_media_type: IMFMediaType = MFCreateMediaType()?;
        input_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input_media_type.SetGUID(&MF_MT_SUBTYPE, &MF_BGR32)?;
        MFSetAttributeSize(input_media_type.cast()?, &MF_MT_FRAME_SIZE, width, height)?;
        MFSetAttributeRatio(input_media_type.cast()?, &MF_MT_FRAME_RATE, self.config.fps as u32, 1)?;
        self.transform.SetInputType(0, Some(&input_media_type), 0)?;

        Ok(())
    }
}

/// Windows video renderer using D3D11
pub struct WindowsRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    decoder: IMFTransform,
    codec: Codec,
}

#[cfg(target_os = "windows")]
unsafe impl Send for WindowsRenderer {}
#[cfg(target_os = "windows")]
unsafe impl Sync for WindowsRenderer {}

impl WindowsRenderer {
    pub fn new(hwnd: HWND) -> Result<Self> {
        Self::new_with_codec(hwnd, Codec::H264)
    }

    pub fn new_with_codec(hwnd: HWND, codec: Codec) -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            let mut swap_chain = None;

            let sc_desc = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: DXGI_MODE_DESC {
                    Width: 0,
                    Height: 0,
                    RefreshRate: DXGI_RATIONAL {
                        Numerator: 60,
                        Denominator: 1,
                    },
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    ..Default::default()
                },
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                OutputWindow: hwnd,
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                Flags: 0,
            };

            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&sc_desc),
                Some(&mut swap_chain),
                Some(&mut device),
                None,
                Some(&mut context),
            )?;

            let device = device.ok_or_else(|| anyhow!("Failed to create D3D11 device"))?;
            let context = context.ok_or_else(|| anyhow!("Failed to create D3D11 context"))?;
            let swap_chain = swap_chain.ok_or_else(|| anyhow!("Failed to create swap chain"))?;

            // Initialize Decoder MFT
            let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0;
            let input_subtype = mf_subtype_for_codec(codec);
            let input_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: input_subtype,
            };

            MFTEnumEx(
                MFT_CATEGORY_VIDEO_DECODER,
                MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 as u32 | MFT_ENUM_FLAG_SORTANDFILTER.0 as u32),
                Some(&input_type),
                None,
                &mut activate_list,
                &mut count,
            ).context("MFTEnumEx failed")?;

            if count == 0 {
                return Err(anyhow!("No hardware decoders found for {:?}", codec));
            }

            let activate = std::slice::from_raw_parts(activate_list, count as usize)[0]
                .as_ref()
                .unwrap();
            let decoder: IMFTransform = activate.ActivateObject()?;
            CoTaskMemFree(Some(activate_list as *const _));

            // Set Decoder Input Type
            let input_type: IMFMediaType = MFCreateMediaType()?;
            input_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input_type.SetGUID(&MF_MT_SUBTYPE, &input_subtype)?;
            decoder.SetInputType(0, Some(&input_type), 0)?;

            // Set Decoder Output Type (B8G8R8A8)
            let output_type: IMFMediaType = MFCreateMediaType()?;
            output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?; // Or BGR32
            decoder.SetOutputType(0, Some(&output_type), 0)?;

            // Setup D3D11 awareness for decoder
            if let Ok(attributes) = decoder.GetAttributes() {
                attributes.SetUINT32(&MF_SA_D3D11_AWARE, 1)?;
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }

            let mut device_manager = None;
            let mut reset_token = 0;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager)?;
            let device_manager =
                device_manager.ok_or_else(|| anyhow!("Failed to create MF device manager"))?;
            device_manager.ResetDevice(&device, reset_token)?;
            decoder.ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                device_manager.as_raw() as usize,
            )?;

            Ok(Self {
                device,
                context,
                swap_chain,
                decoder,
                codec,
            })
        }
    }
}

impl Renderer for WindowsRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        unsafe {
            // 1. Wrap payload in an IMFSample
            let buffer = MFCreateMemoryBuffer(payload.len() as u32)?;
            let mut ptr = std::ptr::null_mut();
            let mut max_length = 0;
            let mut current_length = 0;
            buffer.Lock(&mut ptr, Some(&mut max_length), Some(&mut current_length))?;
            std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(payload.len() as u32)?;

            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(timestamp_us as i64 * 10)?;

            // 2. Feed to decoder
            self.decoder.ProcessInput(0, Some(&sample), 0)?;

            // 3. Drain output
            let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(None),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };

            let mut status = 0;
            match self.decoder.ProcessOutput(
                0,
                std::slice::from_mut(&mut output_data_buffer),
                &mut status,
            ) {
                Ok(_) => {
                    // Decoder provided a sample (likely containing a D3D11 texture)
                    if let Some(output_sample) = unsafe { (*output_data_buffer.pSample).as_ref() } {
                        let buffer = output_sample.GetBufferByIndex(0)?;
                        let access: IDirect3DDxgiInterfaceAccess = buffer.cast()?;
                        let texture: ID3D11Texture2D = access.GetInterface()?;

                        // Get swap chain back buffer
                        let back_buffer: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;

                        // Copy decoded texture to back buffer
                        self.context.CopyResource(&back_buffer, &texture);

                        // Present
                        self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    // No output yet
                }
                Err(e) => return Err(anyhow!("MFT Decoder ProcessOutput failed: {:?}", e)),
            }

            Ok(())
        }
    }
}

/// Windows audio renderer (Opus decode -> PCM -> output)
pub struct WindowsAudioRenderer {
    inner: crate::audio::CpalAudioRenderer,
}

unsafe impl Send for WindowsAudioRenderer {}

impl WindowsAudioRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: crate::audio::CpalAudioRenderer::new()?,
        })
    }

    pub fn push(&mut self, payload: &[u8]) -> Result<()> {
        self.inner.push(payload)
    }
}

impl Renderer for WindowsAudioRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        self.inner.render(payload, timestamp_us)
    }
}

/// Windows audio capturer
pub struct WindowsAudioCapturer {
    audio_client: IAudioClient,
    capture_client: IAudioCaptureClient,
    format: *mut WAVEFORMATEX,
    encoder: OpusEncoder,
    pcm: VecDeque<i16>,
    next_timestamp_us: Option<u64>,
    frame_duration_us: u64,
    channels: usize,
    sample_rate: u32,
    input_channels: usize,
    input_sample_rate: u32,
    start_time: Instant,
    resample_pos: f64,
    resample_prev: Option<[f32; 2]>,
}

impl WindowsAudioCapturer {
    pub async fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).context("CoInitializeEx failed")?;

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).context("CoCreateInstance failed")?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let mut format: *mut WAVEFORMATEX = std::ptr::null_mut();
            audio_client.GetMixFormat(&mut format)?;

            // Note: Loopback requires a specific initialization
            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                0,
                0,
                format,
                None,
            )?;

            let capture_client: IAudioCaptureClient = audio_client.GetService()?;
            audio_client.Start()?;

            if format.is_null() {
                return Err(anyhow!("Audio mix format not available"));
            }

            let input_sample_rate = (*format).nSamplesPerSec;
            let input_channels = (*format).nChannels as usize;

            let encoder = create_opus_encoder()?;

            Ok(Self {
                audio_client,
                capture_client,
                format,
                encoder,
                pcm: VecDeque::with_capacity(AUDIO_MAX_BUFFER_SAMPLES),
                next_timestamp_us: None,
                frame_duration_us: opus_frame_duration_us(),
                channels: OPUS_CHANNELS,
                sample_rate: OPUS_SAMPLE_RATE,
                input_channels,
                input_sample_rate,
                start_time: Instant::now(),
                resample_pos: 0.0,
                resample_prev: None,
            })
        }
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        self.capture_into_buffer()?;

        let frame_len = OPUS_FRAME_SAMPLES * self.channels;
        if self.pcm.len() < frame_len {
            return Err(anyhow!("Not enough audio samples"));
        }

        let frame: Vec<i16> = self.pcm.drain(..frame_len).collect();
        let mut out = vec![0u8; OPUS_MAX_PACKET_BYTES];
        let encoded = self
            .encoder
            .encode(&frame, &mut out)
            .map_err(|e| anyhow!("Opus encode failed: {}", e))?;
        out.truncate(encoded);

        let timestamp_us = self
            .next_timestamp_us
            .unwrap_or_else(|| self.start_time.elapsed().as_micros() as u64);
        self.next_timestamp_us = Some(timestamp_us.saturating_add(self.frame_duration_us));

        Ok(EncodedFrame {
            timestamp_us,
            keyframe: true,
            data: out,
        })
    }

    fn push_resampled_samples(&mut self, samples: &[i16]) -> Result<()> {
        let frames = interleaved_to_stereo(samples, self.input_channels);
        let resampled = if self.input_sample_rate == OPUS_SAMPLE_RATE {
            frames
        } else {
            self.resample_frames(&frames)
        };

        for frame in resampled {
            self.pcm.push_back(f32_to_i16(frame[0]));
            self.pcm.push_back(f32_to_i16(frame[1]));
        }

        Ok(())
    }

    fn resample_frames(&mut self, frames: &[[f32; 2]]) -> Vec<[f32; 2]> {
        if frames.is_empty() {
            return Vec::new();
        }

        let mut src: Vec<[f32; 2]> = Vec::with_capacity(frames.len() + 1);
        if let Some(prev) = self.resample_prev {
            src.push(prev);
        }
        src.extend_from_slice(frames);

        if src.len() < 2 {
            self.resample_prev = src.last().copied();
            return Vec::new();
        }

        let step = self.input_sample_rate as f64 / OPUS_SAMPLE_RATE as f64;
        let mut pos = self.resample_pos;
        let mut out = Vec::new();
        while pos + 1.0 < src.len() as f64 {
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = src[idx];
            let b = src[idx + 1];
            let sample = [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac];
            out.push(sample);
            pos += step;
        }

        self.resample_pos = pos - (src.len().saturating_sub(1) as f64);
        self.resample_prev = src.last().copied();
        out
    }

    fn capture_into_buffer(&mut self) -> Result<()> {
        unsafe {
            let mut data = std::ptr::null_mut();
            let mut frames_available = 0;
            let mut flags = 0;

            match self.capture_client.GetBuffer(
                &mut data,
                &mut frames_available,
                &mut flags,
                None,
                None,
            ) {
                Ok(_) => {
                    if frames_available == 0 {
                        self.capture_client.ReleaseBuffer(0)?;
                        return Err(anyhow!("No audio frames available"));
                    }

                    let total_samples = frames_available as usize * self.input_channels;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        let zeros = vec![0i16; total_samples];
                        self.push_resampled_samples(&zeros)?;
                    } else {
                        let block_align = (*self.format).nBlockAlign as usize;
                        let byte_size = frames_available as usize * block_align;
                        let bytes = std::slice::from_raw_parts(data as *const u8, byte_size);
                        let samples = decode_pcm_samples(bytes, &*self.format, total_samples)?;
                        self.push_resampled_samples(&samples)?;
                    }

                    self.capture_client.ReleaseBuffer(frames_available)?;
                    if self.next_timestamp_us.is_none() || self.pcm.is_empty() {
                        self.next_timestamp_us = Some(self.start_time.elapsed().as_micros() as u64);
                    }

                    if self.pcm.len() > AUDIO_MAX_BUFFER_SAMPLES {
                        let drop = self.pcm.len() - AUDIO_MAX_BUFFER_SAMPLES;
                        let aligned_drop = drop - (drop % self.channels.max(1));
                        for _ in 0..aligned_drop {
                            self.pcm.pop_front();
                        }
                        if let Some(ts) = self.next_timestamp_us.as_mut() {
                            let frames_dropped = aligned_drop / self.channels.max(1);
                            let advance =
                                (frames_dropped as u64) * 1_000_000 / (self.sample_rate as u64);
                            *ts = ts.saturating_add(advance);
                        }
                    }

                    Ok(())
                }
                Err(e) => Err(anyhow!("Failed to get audio buffer: {}", e)),
            }
        }
    }
}

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

enum PcmFormat {
    I16,
    I32,
    F32,
}

fn pcm_format(format: &WAVEFORMATEX) -> Result<PcmFormat> {
    let tag = format.wFormatTag as u32;
    if tag == WAVE_FORMAT_PCM {
        match format.wBitsPerSample {
            16 => Ok(PcmFormat::I16),
            32 => Ok(PcmFormat::I32),
            _ => Err(anyhow!("Unsupported PCM bit depth")),
        }
    } else if tag == WAVE_FORMAT_IEEE_FLOAT {
        Ok(PcmFormat::F32)
    } else if tag == WAVE_FORMAT_EXTENSIBLE {
        unsafe {
            let extensible = &*(format as *const _ as *const WAVEFORMATEXTENSIBLE);
            // Use read_unaligned or copy to local to avoid misaligned reference
            let subformat = std::ptr::read_unaligned(&extensible.SubFormat);
            if subformat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
                Ok(PcmFormat::F32)
            } else if subformat == KSDATAFORMAT_SUBTYPE_PCM {
                match format.wBitsPerSample {
                    16 => Ok(PcmFormat::I16),
                    32 => Ok(PcmFormat::I32),
                    _ => Err(anyhow!("Unsupported PCM bit depth")),
                }
            } else {
                Err(anyhow!("Unsupported audio subformat"))
            }
        }
    } else {
        Err(anyhow!("Unsupported audio format tag: {}", tag))
    }
}

fn decode_pcm_samples(
    bytes: &[u8],
    format: &WAVEFORMATEX,
    total_samples: usize,
) -> Result<Vec<i16>> {
    let fmt = pcm_format(format)?;
    let bytes_per_sample = (format.wBitsPerSample / 8) as usize;
    let expected = total_samples * bytes_per_sample;
    if bytes.len() < expected {
        return Err(anyhow!("Audio buffer too small"));
    }

    let mut out = Vec::with_capacity(total_samples);
    match fmt {
        PcmFormat::I16 => {
            for chunk in bytes.chunks_exact(2).take(total_samples) {
                out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
        }
        PcmFormat::I32 => {
            for chunk in bytes.chunks_exact(4).take(total_samples) {
                let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let scaled = (value as f64 / i32::MAX as f64 * i16::MAX as f64) as i16;
                out.push(scaled);
            }
        }
        PcmFormat::F32 => {
            for chunk in bytes.chunks_exact(4).take(total_samples) {
                let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                out.push((value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            }
        }
    }
    Ok(out)
}

fn trigger_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * u8::MAX as f32) as u8
}

const XUSB_GAMEPAD_LEFT_SHOULDER: u16 = 0x0100;
const XUSB_GAMEPAD_RIGHT_SHOULDER: u16 = 0x0200;
const XUSB_GAMEPAD_A: u16 = 0x1000;
const XUSB_GAMEPAD_B: u16 = 0x2000;
const XUSB_GAMEPAD_X: u16 = 0x4000;
const XUSB_GAMEPAD_Y: u16 = 0x8000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XusbReport {
    wButtons: u16,
    bLeftTrigger: u8,
    bRightTrigger: u8,
    sThumbLX: i16,
    sThumbLY: i16,
    sThumbRX: i16,
    sThumbRY: i16,
}

type VigemAlloc = unsafe extern "C" fn() -> *mut c_void;
type VigemFree = unsafe extern "C" fn(*mut c_void);
type VigemConnect = unsafe extern "C" fn(*mut c_void) -> u32;
type VigemDisconnect = unsafe extern "C" fn(*mut c_void);
type VigemTargetAlloc = unsafe extern "C" fn() -> *mut c_void;
type VigemTargetFree = unsafe extern "C" fn(*mut c_void);
type VigemTargetAdd = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
type VigemTargetRemove = unsafe extern "C" fn(*mut c_void, *mut c_void);
type VigemTargetX360Update = unsafe extern "C" fn(*mut c_void, *mut c_void, XusbReport) -> u32;

struct VigemGamepad {
    _lib: Library,
    client: *mut c_void,
    target: *mut c_void,
    vigem_free: VigemFree,
    vigem_disconnect: VigemDisconnect,
    target_free: VigemTargetFree,
    target_remove: VigemTargetRemove,
    x360_update: VigemTargetX360Update,
}

#[cfg(target_os = "windows")]
unsafe impl Send for VigemGamepad {}
#[cfg(target_os = "windows")]
unsafe impl Sync for VigemGamepad {}

impl VigemGamepad {
    fn new() -> Result<Self> {
        let lib = unsafe { Library::new("vigemclient.dll") }?;
        let vigem_alloc: VigemAlloc = unsafe { *lib.get(b"vigem_alloc\0")? };
        let vigem_free: VigemFree = unsafe { *lib.get(b"vigem_free\0")? };
        let vigem_connect: VigemConnect = unsafe { *lib.get(b"vigem_connect\0")? };
        let vigem_disconnect: VigemDisconnect = unsafe { *lib.get(b"vigem_disconnect\0")? };
        let target_alloc: VigemTargetAlloc = unsafe { *lib.get(b"vigem_target_x360_alloc\0")? };
        let target_free: VigemTargetFree = unsafe { *lib.get(b"vigem_target_free\0")? };
        let target_add: VigemTargetAdd = unsafe { *lib.get(b"vigem_target_add\0")? };
        let target_remove: VigemTargetRemove = unsafe { *lib.get(b"vigem_target_remove\0")? };
        let x360_update: VigemTargetX360Update =
            unsafe { *lib.get(b"vigem_target_x360_update\0")? };

        let client = unsafe { vigem_alloc() };
        if client.is_null() {
            return Err(anyhow!("ViGEm alloc failed"));
        }
        let err = unsafe { vigem_connect(client) };
        if err != 0 {
            unsafe { vigem_free(client) };
            return Err(anyhow!("ViGEm connect failed: {}", err));
        }

        let target = unsafe { target_alloc() };
        if target.is_null() {
            unsafe {
                vigem_disconnect(client);
                vigem_free(client);
            }
            return Err(anyhow!("ViGEm target alloc failed"));
        }
        let err = unsafe { target_add(client, target) };
        if err != 0 {
            unsafe {
                target_free(target);
                vigem_disconnect(client);
                vigem_free(client);
            }
            return Err(anyhow!("ViGEm target add failed: {}", err));
        }

        Ok(Self {
            _lib: lib,
            client,
            target,
            vigem_free,
            vigem_disconnect,
            target_free,
            target_remove,
            x360_update,
        })
    }

    fn update(&self, report: XusbReport) -> Result<()> {
        let err = unsafe { (self.x360_update)(self.client, self.target, report) };
        if err != 0 {
            return Err(anyhow!("ViGEm update failed: {}", err));
        }
        Ok(())
    }
}

impl Drop for VigemGamepad {
    fn drop(&mut self) {
        unsafe {
            (self.target_remove)(self.client, self.target);
            (self.target_free)(self.target);
            (self.vigem_disconnect)(self.client);
            (self.vigem_free)(self.client);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct HandState {
    axes: [f32; 4],
    buttons: [bool; 2],
}

#[derive(Default)]
struct GamepadState {
    left: HandState,
    right: HandState,
}

impl GamepadState {
    fn update(
        &mut self,
        id: u32,
        axes: &[crate::GamepadAxis],
        buttons: &[crate::GamepadButton],
        deadzone: f32,
    ) {
        let target = if id == 0 {
            &mut self.left
        } else {
            &mut self.right
        };
        target.axes = [0.0; 4];
        target.buttons = [false; 2];
        for axis in axes {
            let idx = axis.axis as usize;
            if idx < target.axes.len() {
                target.axes[idx] = apply_gamepad_deadzone(axis.value, deadzone);
            }
        }
        for button in buttons {
            let idx = button.button as usize;
            if idx < target.buttons.len() {
                target.buttons[idx] = button.pressed;
            }
        }
    }

    fn to_report(&self) -> XusbReport {
        let mut buttons = 0u16;
        if self.right.buttons[0] {
            buttons |= XUSB_GAMEPAD_A;
        }
        if self.right.buttons[1] {
            buttons |= XUSB_GAMEPAD_B;
        }
        if self.left.buttons[0] {
            buttons |= XUSB_GAMEPAD_X;
        }
        if self.left.buttons[1] {
            buttons |= XUSB_GAMEPAD_Y;
        }
        if self.left.axes[3] > 0.5 {
            buttons |= XUSB_GAMEPAD_LEFT_SHOULDER;
        }
        if self.right.axes[3] > 0.5 {
            buttons |= XUSB_GAMEPAD_RIGHT_SHOULDER;
        }

        XusbReport {
            wButtons: buttons,
            bLeftTrigger: trigger_to_u8(self.left.axes[2]),
            bRightTrigger: trigger_to_u8(self.right.axes[2]),
            sThumbLX: axis_to_i16(self.left.axes[0]),
            sThumbLY: axis_to_i16(self.left.axes[1]),
            sThumbRX: axis_to_i16(self.right.axes[0]),
            sThumbRY: axis_to_i16(self.right.axes[1]),
        }
    }
}

fn axis_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn interleaved_to_stereo(samples: &[i16], channels: usize) -> Vec<[f32; 2]> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = samples.len() / channels;
    let mut out = Vec::with_capacity(frames);
    let mut idx = 0;
    for _ in 0..frames {
        let left = samples[idx] as f32 / i16::MAX as f32;
        let right = if channels > 1 {
            samples[idx + 1] as f32 / i16::MAX as f32
        } else {
            left
        };
        out.push([left, right]);
        idx += channels;
    }
    out
}

fn normalize_gamepad_deadzone(deadzone: f32) -> f32 {
    deadzone.clamp(0.0, 0.95)
}

fn apply_gamepad_deadzone(value: f32, deadzone: f32) -> f32 {
    let deadzone = normalize_gamepad_deadzone(deadzone);
    let abs = value.abs();
    if abs <= deadzone {
        0.0
    } else {
        let scaled = (abs - deadzone) / (1.0 - deadzone);
        scaled.copysign(value).clamp(-1.0, 1.0)
    }
}

/// Windows input injector
pub struct WindowsInputInjector {
    gamepad: Option<VigemGamepad>,
    gamepad_state: GamepadState,
    gamepad_failed: bool,
    gamepad_deadzone: f32,
}

#[cfg(target_os = "windows")]
unsafe impl Send for WindowsInputInjector {}
#[cfg(target_os = "windows")]
unsafe impl Sync for WindowsInputInjector {}

impl Default for WindowsInputInjector {
    fn default() -> Self {
        Self {
            gamepad: None,
            gamepad_state: GamepadState::default(),
            gamepad_failed: false,
            gamepad_deadzone: 0.1,
        }
    }
}

impl WindowsInputInjector {
    pub fn with_gamepad_deadzone(deadzone: f32) -> Self {
        Self {
            gamepad_deadzone: normalize_gamepad_deadzone(deadzone),
            ..Self::default()
        }
    }

    pub fn set_gamepad_deadzone(&mut self, deadzone: f32) {
        self.gamepad_deadzone = normalize_gamepad_deadzone(deadzone);
    }
}

impl crate::InputInjector for WindowsInputInjector {
    fn inject(&mut self, event: crate::InputEvent) -> Result<()> {
        unsafe {
            match event {
                crate::InputEvent::MouseMove { x, y } => {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_MOUSE;
                    input.Anonymous.mi = MOUSEINPUT {
                        dx: (x * 65535.0) as i32,
                        dy: (y * 65535.0) as i32,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                crate::InputEvent::MouseDown { button } => {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_MOUSE;
                    let flag = match button {
                        crate::MouseButton::Left => MOUSEEVENTF_LEFTDOWN,
                        crate::MouseButton::Right => MOUSEEVENTF_RIGHTDOWN,
                        crate::MouseButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
                    };
                    input.Anonymous.mi = MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: flag,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                crate::InputEvent::MouseUp { button } => {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_MOUSE;
                    let flag = match button {
                        crate::MouseButton::Left => MOUSEEVENTF_LEFTUP,
                        crate::MouseButton::Right => MOUSEEVENTF_RIGHTUP,
                        crate::MouseButton::Middle => MOUSEEVENTF_MIDDLEUP,
                    };
                    input.Anonymous.mi = MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: flag,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                crate::InputEvent::KeyDown { key_code } => {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_KEYBOARD;
                    input.Anonymous.ki = KEYBDINPUT {
                        wVk: VIRTUAL_KEY(key_code as u16),
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                crate::InputEvent::KeyUp { key_code } => {
                    let mut input = INPUT::default();
                    input.r#type = INPUT_KEYBOARD;
                    input.Anonymous.ki = KEYBDINPUT {
                        wVk: VIRTUAL_KEY(key_code as u16),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                crate::InputEvent::Scroll { dx, dy } => {
                    if dy != 0.0 {
                        let mut input = INPUT::default();
                        input.r#type = INPUT_MOUSE;
                        input.Anonymous.mi = MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: (dy * 120.0) as i32 as u32,
                            dwFlags: MOUSEEVENTF_WHEEL,
                            time: 0,
                            dwExtraInfo: 0,
                        };
                        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    }
                    if dx != 0.0 {
                        let mut input = INPUT::default();
                        input.r#type = INPUT_MOUSE;
                        input.Anonymous.mi = MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: (dx * 120.0) as i32 as u32,
                            dwFlags: MOUSEEVENTF_HWHEEL,
                            time: 0,
                            dwExtraInfo: 0,
                        };
                        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    }
                }
                crate::InputEvent::Gamepad {
                    gamepad_id,
                    axes,
                    buttons,
                } => {
                    self.gamepad_state
                        .update(gamepad_id, &axes, &buttons, self.gamepad_deadzone);
                    if self.gamepad.is_none() && !self.gamepad_failed {
                        match VigemGamepad::new() {
                            Ok(pad) => self.gamepad = Some(pad),
                            Err(err) => {
                                log::warn!("ViGEm init failed: {}", err);
                                self.gamepad_failed = true;
                            }
                        }
                    }
                    if let Some(pad) = self.gamepad.as_ref() {
                        if let Err(err) = pad.update(self.gamepad_state.to_report()) {
                            log::warn!("ViGEm update failed: {}", err);
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

/// Windows capability probe
pub struct WindowsProbe;

impl crate::CapabilityProbe for WindowsProbe {
    fn supported_encoders(&self) -> Result<Vec<crate::Codec>> {
        supported_mft_codecs(MFT_CATEGORY_VIDEO_ENCODER)
    }

    fn encoder_capabilities(&self) -> Result<Vec<crate::VideoCodecCapability>> {
        Ok(self
            .supported_encoders()?
            .into_iter()
            .map(|codec| {
                let supports_hdr10 = matches!(codec, Codec::Av1 | Codec::Hevc);
                crate::VideoCodecCapability {
                    codec,
                    hardware_accelerated: true,
                    supports_10bit: supports_hdr10,
                    supports_hdr10,
                }
            })
            .collect())
    }

    fn supported_decoders(&self) -> Result<Vec<crate::Codec>> {
        supported_mft_codecs(MFT_CATEGORY_VIDEO_DECODER)
    }

    fn enumerate_displays(&self) -> Result<Vec<crate::DisplayInfo>> {
        unsafe {
            let mut displays = Vec::new();

            unsafe extern "system" fn enum_monitor_callback(
                hmount: HMONITOR,
                _hdc: HDC,
                _rect: *mut RECT,
                lparam: LPARAM,
            ) -> BOOL {
                let displays = &mut *(lparam.0 as *mut Vec<crate::DisplayInfo>);

                let mut info = MONITORINFOEXW::default();
                info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
                if GetMonitorInfoW(hmount, &mut info.monitorInfo as *mut _ as *mut _).as_bool() {
                    let name = String::from_utf16_lossy(&info.szDevice);
                    let name = name.trim_matches(char::from(0)).to_string();

                    displays.push(crate::DisplayInfo {
                        id: hmount.0 as u32,
                        name,
                        resolution: crate::Resolution {
                            width: (info.monitorInfo.rcMonitor.right
                                - info.monitorInfo.rcMonitor.left)
                                as u16,
                            height: (info.monitorInfo.rcMonitor.bottom
                                - info.monitorInfo.rcMonitor.top)
                                as u16,
                        },
                    });
                }

                true.into()
            }

            EnumDisplayMonitors(
                None,
                None,
                Some(enum_monitor_callback),
                LPARAM(&mut displays as *mut _ as isize),
            )
            .ok()?;

            Ok(displays)
        }
    }
}

fn mf_subtype_for_codec(codec: Codec) -> GUID {
    match codec {
        Codec::H264 => MFVideoFormat_H264,
        Codec::Hevc => MFVideoFormat_HEVC,
        Codec::Av1 => MFVideoFormat_AV1,
    }
}

fn supported_mft_codecs(category: GUID) -> Result<Vec<Codec>> {
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup failed")?;
        let mut supported = Vec::new();
        for codec in [Codec::H264, Codec::Hevc, Codec::Av1] {
            let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0;
            let subtype = mf_subtype_for_codec(codec);
            let type_info = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: subtype,
            };
            let is_decoder = category == MFT_CATEGORY_VIDEO_DECODER;
            let input_type = if is_decoder { Some(&type_info as *const _) } else { None };
            let output_type = if is_decoder { None } else { Some(&type_info as *const _) };
            
            let _ = MFTEnumEx(
                category,
                MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 as u32 | MFT_ENUM_FLAG_SORTANDFILTER.0 as u32),
                input_type,
                output_type,
                &mut activate_list,
                &mut count,
            );
            if count > 0 {
                supported.push(codec);
            }
            if !activate_list.is_null() {
                CoTaskMemFree(Some(activate_list as *const _));
            }
        }
        Ok(supported)
    }
}
