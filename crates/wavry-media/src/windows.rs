// Windows implementation for wavry-media
// Using Windows.Graphics.Capture (WGC) for high-performance screen capture.

use anyhow::{anyhow, Result};
use crate::{DecodeConfig, EncodeConfig, EncodedFrame, Renderer, RawFrame, FrameData, FrameFormat};
use std::sync::Arc;

#[cfg(target_os = "windows")]
use windows::{
    core::*,
    Foundation::TypedEventHandler,
    Graphics::Capture::*,
    Graphics::DirectX::Direct3D11::IDirect3DDevice,
    Graphics::DirectX::DirectXPixelFormat,
    Win32::Graphics::Direct3D11::*,
    Win32::Graphics::Dxgi::*,
    Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
    Win32::UI::WindowsAndMessaging::GetDesktopWindow,
};

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
}

impl WindowsEncoder {
    pub async fn new(config: EncodeConfig) -> Result<Self> {
        unsafe {
            // Initialize Media Foundation
            MFStartup(MF_VERSION, MFSTARTUP_FULL).ok()?;

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
            )?;
            let device = device.ok_or_else(|| anyhow!("Failed to create D3D11 device"))?;
            let context = context.ok_or_else(|| anyhow!("Failed to create D3D11 context"))?;

            // Get Desktop Window for capture
            let hwnd = GetDesktopWindow();
            let interop: IGraphicsCaptureItemInterop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let capture_item: GraphicsCaptureItem = interop.CreateForWindow(hwnd)?;

            let item_size = capture_item.Size()?;

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

            // Initialize Media Foundation H.264 Encoder (MFT)
            let mut activate_list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0;

            let mft_category = MFT_CATEGORY_VIDEO_ENCODER;
            let output_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_H264,
            };

            MFTEnumEx(
                mft_category,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&output_type),
                &mut activate_list,
                &mut count,
            )?;

            if count == 0 {
                return Err(anyhow!("No H.264 hardware encoders found"));
            }

            let activate = std::slice::from_raw_parts(activate_list, count as usize)[0].as_ref().unwrap();
            let transform: IMFTransform = activate.ActivateObject()?;
            
            // Set Output Type
            let output_media_type: IMFMediaType = MFCreateMediaType()?;
            output_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output_media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output_media_type.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate_kbps * 1000)?;
            MFSetAttributeRatio(&output_media_type, &MF_MT_FRAME_RATE, config.fps, 1)?;
            MFSetAttributeSize(&output_media_type, &MF_MT_FRAME_SIZE, item_size.Width as u32, item_size.Height as u32)?;
            output_media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            
            transform.SetOutputType(0, Some(&output_media_type), 0)?;

            // Set Input Type
            let input_media_type: IMFMediaType = MFCreateMediaType()?;
            input_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input_media_type.SetGUID(&MF_MT_SUBTYPE, &MF_BGR32)?; // B8G8R8A8 in MF is often BGR32
            MFSetAttributeSize(&input_media_type, &MF_MT_FRAME_SIZE, item_size.Width as u32, item_size.Height as u32)?;
            MFSetAttributeRatio(&input_media_type, &MF_MT_FRAME_RATE, config.fps, 1)?;
            
            transform.SetInputType(0, Some(&input_media_type), 0)?;

            CoTaskMemFree(Some(activate_list as _));

            Ok(Self {
                config,
                device,
                context,
                capture_item,
                capture_session,
                frame_pool,
                transform,
            })
        }
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        unsafe {
            if let Ok(frame) = self.frame_pool.TryGetNextFrame() {
                let surface = frame.Surface()?;
                let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
                let texture: ID3D11Texture2D = access.GetInterface()?;

                // TODO: Process texture through Media Foundation MFT
                // For now, we'll return a stub or just the raw bytes if small enough
                // In a real implementation, we'd copy the texture to an MF sample
                
                Ok(EncodedFrame {
                    timestamp_us: frame.SystemRelativeTime().Duration as u64 / 10, // 100ns to us
                    keyframe: true,
                    data: vec![0, 0, 0, 1, 0x67, 0x42, 0x80, 0x1e], // Fake SPS stub
                })
            } else {
                // If no frame is available yet, return a "quiet" error or wait
                Err(anyhow!("No frame available"))
            }
        }
    }
    pub fn set_bitrate(&mut self, _bitrate_kbps: u32) -> Result<()> {
        Ok(())
    }
}

/// Windows video renderer using D3D11
pub struct WindowsRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
}

impl WindowsRenderer {
    pub fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            let mut swap_chain = None;

            let sc_desc = DXGI_SWAP_CHAIN_DESC {
                BufferDesc: DXGI_MODE_DESC {
                    Width: 0,
                    Height: 0,
                    RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    ..Default::default()
                },
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                OutputWindow: hwnd,
                Windowed: true.into(),
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                ..Default::default()
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

            Ok(Self {
                device,
                context,
                swap_chain,
            })
        }
    }
}

impl Renderer for WindowsRenderer {
    fn render(&mut self, _payload: &[u8], _timestamp_us: u64) -> Result<()> {
        unsafe {
            // TODO: Implement frame decoding and texture copy
            
            // Present
            self.swap_chain.Present(1, 0).ok()?;
            Ok(())
        }
    }
}

/// Windows audio renderer
pub struct WindowsAudioRenderer;

impl WindowsAudioRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn play(&mut self, _data: &[u8]) -> Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::*;

/// Windows audio capturer
pub struct WindowsAudioCapturer {
    audio_client: IAudioClient,
    capture_client: IAudioCaptureClient,
    format: *mut WAVEFORMATEX,
}

impl WindowsAudioCapturer {
    pub async fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;

            let enumerator: IMMDeviceEnumerator = CoCreateInstance(
                &MMDeviceEnumerator,
                None,
                CLSCTX_ALL,
            )?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            
            let mut format = std::ptr::null_mut();
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

            Ok(Self {
                audio_client,
                capture_client,
                format,
            })
        }
    }

    pub fn next_frame(&mut self) -> Result<EncodedFrame> {
        unsafe {
            let mut data = std::ptr::null_mut();
            let mut frames_available = 0;
            let mut flags = 0;
            
            match self.capture_client.GetBuffer(&mut data, &mut frames_available, &mut flags, None, None) {
                Ok(_) => {
                    if frames_available > 0 {
                        // Calculate byte size based on format
                        let block_align = (*self.format).nBlockAlign as usize;
                        let byte_size = frames_available as usize * block_align;
                        
                        let payload = std::slice::from_raw_parts(data, byte_size).to_vec();
                        
                        self.capture_client.ReleaseBuffer(frames_available)?;
                        
                        Ok(EncodedFrame {
                            timestamp_us: 0, // TODO: Get actual timing
                            keyframe: true,
                            data: payload,
                        })
                    } else {
                        self.capture_client.ReleaseBuffer(0)?;
                        Err(anyhow!("No audio frames available"))
                    }
                }
                Err(e) => Err(anyhow!("Failed to get audio buffer: {}", e)),
            }
        }
    }
}
