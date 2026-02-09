#![allow(clippy::missing_safety_doc)]

use once_cell::sync::Lazy;
use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use wavry_client::RelayInfo;

#[cfg(target_os = "android")]
use wavry_media::AndroidVideoRenderer as VideoRenderer;
#[cfg(not(any(target_os = "macos", target_os = "android")))]
use wavry_media::DummyRenderer as VideoRenderer;
#[cfg(target_os = "macos")]
use wavry_media::MacVideoRenderer as VideoRenderer;

// Stub for Linux input injector if needed, or use a dummy
#[cfg(not(any(target_os = "macos", target_os = "android")))]
pub struct MacInputInjector;
#[cfg(not(any(target_os = "macos", target_os = "android")))]
impl MacInputInjector {
    pub fn new(_w: u32, _h: u32) -> Self {
        Self
    }
    pub fn inject(&mut self, _e: InputEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "android")]
pub struct AndroidInputInjector;
#[cfg(target_os = "android")]
impl AndroidInputInjector {
    pub fn new(_w: u32, _h: u32) -> Self {
        Self
    }
    pub fn inject(&mut self, _e: InputEvent) -> anyhow::Result<()> {
        // In a real implementation, we would use AInputQueue or similar
        Ok(())
    }
}

use wavry_media::InputEvent;
#[cfg(target_os = "macos")]
use wavry_media::{InputInjector, MacInputInjector};

mod session;
use session::{
    run_client, run_host, ClientSessionParams, HostRuntimeConfig, SessionHandle, SessionStats,
};

mod identity;
mod signaling_ffi;

// Global State
static RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

static SESSION: Mutex<Option<SessionHandle>> = Mutex::new(None);
static LAST_ERROR: Lazy<Mutex<CString>> =
    Lazy::new(|| Mutex::new(CString::new("").expect("empty cstring")));
static LAST_CLOUD_STATUS: Lazy<Mutex<CString>> =
    Lazy::new(|| Mutex::new(CString::new("").expect("empty cstring")));

// Shared media resources (FFI -> Rust)
static VIDEO_RENDERER: Lazy<Arc<Mutex<Option<Box<VideoRenderer>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

#[cfg(target_os = "macos")]
static INPUT_INJECTOR: Lazy<Arc<Mutex<Option<MacInputInjector>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

#[cfg(target_os = "android")]
static INPUT_INJECTOR: Lazy<Arc<Mutex<Option<AndroidInputInjector>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

fn set_last_error(msg: &str) {
    let sanitized = msg.replace('\0', " ");
    let cstr =
        CString::new(sanitized).unwrap_or_else(|_| CString::new("invalid error").expect("cstring"));
    let mut guard = LAST_ERROR.lock().unwrap();
    *guard = cstr;
}

fn clear_last_error() {
    set_last_error("");
}

pub(crate) fn set_cloud_status(msg: &str) {
    let sanitized = msg.replace('\0', " ");
    let cstr = CString::new(sanitized)
        .unwrap_or_else(|_| CString::new("invalid status").expect("cstring"));
    let mut guard = LAST_CLOUD_STATUS.lock().unwrap();
    *guard = cstr;
}

pub(crate) fn clear_cloud_status() {
    set_cloud_status("");
}

#[no_mangle]
pub extern "C" fn wavry_init() {
    // Initialize logger if not already
    let _ = env_logger::try_init();
    clear_last_error();
    clear_cloud_status();
    log::info!("Wavry Core (FFI) Initialized 🚀");
}

#[no_mangle]
pub unsafe extern "C" fn wavry_android_init(
    _vm: *mut std::ffi::c_void,
    _context: *mut std::ffi::c_void,
) {
    #[cfg(target_os = "android")]
    {
        log::info!(
            "FFI: Initializing Android context (VM: {:?}, Context: {:?})",
            _vm,
            _context
        );
        ndk_context::initialize_android_context(_vm, _context);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_init_identity(storage_path_ptr: *const c_char) -> i32 {
    if storage_path_ptr.is_null() {
        set_last_error("Identity init failed: null storage path");
        return -1;
    }
    let c_str = CStr::from_ptr(storage_path_ptr);
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error("Identity init failed: invalid UTF-8 storage path");
            return -2;
        }
    };

    match identity::init_identity(path_str) {
        Ok(_) => {
            clear_last_error();
            0
        }
        Err(e) => {
            log::error!("Failed to init identity: {}", e);
            set_last_error(&format!("Identity init failed: {}", e));
            -3
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_get_public_key(out_buffer: *mut u8) -> i32 {
    if out_buffer.is_null() {
        set_last_error("Public key fetch failed: null output buffer");
        return -1;
    }

    if let Some(pub_key) = identity::get_public_key() {
        std::ptr::copy_nonoverlapping(pub_key.as_ptr(), out_buffer, 32);
        clear_last_error();
        0
    } else {
        // Identity not initialized
        set_last_error("Public key fetch failed: identity not initialized");
        -2
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

#[repr(C)]
pub struct WavryHostConfig {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
    pub bitrate_kbps: u32,
    pub keyframe_interval_ms: u32,
    pub display_id: u32,
}

fn normalize_host_config(raw: &WavryHostConfig) -> HostRuntimeConfig {
    let width = raw.width.clamp(320, 7680);
    let height = raw.height.clamp(240, 4320);
    let fps = raw.fps.clamp(15, 240);
    let bitrate_kbps = raw.bitrate_kbps.clamp(1_000, 100_000);
    let keyframe_interval_ms = raw.keyframe_interval_ms.clamp(250, 10_000);
    let display_id = if raw.display_id == u32::MAX {
        None
    } else {
        Some(raw.display_id)
    };

    HostRuntimeConfig {
        codec: wavry_media::Codec::H264,
        width,
        height,
        fps,
        bitrate_kbps,
        keyframe_interval_ms,
        display_id,
    }
}

fn start_host_internal(port: u16, host_config: HostRuntimeConfig) -> i32 {
    let mut guard = SESSION.lock().unwrap();
    if guard.is_some() {
        log::warn!("Session already running");
        set_last_error("Host start failed: session already running");
        return -1;
    }

    clear_cloud_status();

    let stats = Arc::new(SessionStats::default());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<anyhow::Result<u16>>();

    let stats_clone = stats.clone();
    RUNTIME.spawn(async move {
        if let Err(e) = run_host(port, host_config, stats_clone, rx, init_tx).await {
            log::error!("Host error: {}", e);
        }
    });

    match init_rx.blocking_recv() {
        Ok(Ok(bound_port)) => {
            *guard = Some(SessionHandle {
                stop_tx: Some(tx),
                monitor_tx: None, // Host mode doesn't currently use monitor_tx
                stats,
            });
            clear_last_error();
            set_cloud_status(&format!("Hosting on UDP {}", bound_port));
            log::info!(
                "Started Host (requested port {}, bound port {}) ({}x{} @ {}fps, {} kbps, keyframe {}ms, display {:?})",
                port,
                bound_port,
                host_config.width,
                host_config.height,
                host_config.fps,
                host_config.bitrate_kbps,
                host_config.keyframe_interval_ms,
                host_config.display_id
            );
            0
        }
        Ok(Err(e)) => {
            log::error!("Failed to start host: {}", e);
            set_last_error(&format!("Host start failed: {}", e));
            -2
        }
        Err(_) => {
            log::error!("Host init channel closed unexpectedly");
            set_last_error("Host start failed: initialization channel closed");
            -3
        }
    }
}

/// Start Host Mode (Screen Capture -> UDP Stream)
#[no_mangle]
pub extern "C" fn wavry_start_host(port: u16) -> i32 {
    start_host_internal(port, HostRuntimeConfig::default())
}

/// Start Host Mode with explicit runtime configuration.
#[no_mangle]
pub unsafe extern "C" fn wavry_start_host_with_config(
    port: u16,
    config_ptr: *const WavryHostConfig,
) -> i32 {
    if config_ptr.is_null() {
        set_last_error("Host start failed: null host config pointer");
        return -4;
    }

    let raw = &*config_ptr;
    let config = normalize_host_config(raw);
    start_host_internal(port, config)
}

/// Start Client Mode (UDP Stream -> Remote Display)
fn start_client_internal(
    direct_target: Option<(String, u16)>,
    relay_info: Option<RelayInfo>,
    client_name: String,
) -> i32 {
    let mut guard = SESSION.lock().unwrap();
    if guard.is_some() {
        log::warn!("Session already running");
        set_last_error("Client start failed: session already running");
        return -1;
    }

    if direct_target.is_none() && relay_info.is_none() {
        set_last_error("Client start failed: no connection targets");
        return -2;
    }

    let target_label = if let Some((host, port)) = direct_target.as_ref() {
        format!("{}:{}", host, port)
    } else if let Some(relay) = relay_info.as_ref() {
        format!("relay {}", relay.addr)
    } else {
        "unknown target".to_string()
    };

    let stats = Arc::new(SessionStats::default());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (init_tx, init_rx) = tokio::sync::oneshot::channel();
    let (monitor_tx, monitor_rx) = tokio::sync::mpsc::unbounded_channel::<u32>();

    let stats_clone = stats.clone();
    let renderer = VIDEO_RENDERER.clone(); // Shared Reference

    RUNTIME.spawn(async move {
        if let Err(e) = run_client(ClientSessionParams {
            direct_target,
            relay_info,
            client_name,
            renderer_handle: renderer,
            stats: stats_clone,
            stop_rx: rx,
            init_tx,
            monitor_rx,
        })
        .await
        {
            set_last_error(&format!("Client runtime error: {}", e));
            log::error!("Client error: {}", e);
        }
    });

    // Wait for initialization
    match init_rx.blocking_recv() {
        Ok(Ok(())) => {
            *guard = Some(SessionHandle {
                stop_tx: Some(tx),
                monitor_tx: Some(monitor_tx),
                stats,
            });
            clear_last_error();
            log::info!("Started Client connecting to {}", target_label);
            0
        }
        Ok(Err(e)) => {
            log::error!("Failed to start client: {}", e);
            set_last_error(&format!("Client start failed: {}", e));
            -4
        }
        Err(_) => {
            log::error!("Client init channel closed");
            set_last_error("Client start failed: initialization channel closed");
            -5
        }
    }
}

pub(crate) fn start_client_with_targets(
    direct_target: Option<(String, u16)>,
    relay_info: Option<RelayInfo>,
) -> i32 {
    start_client_internal(direct_target, relay_info, "WavryAndroid".to_string())
}

#[no_mangle]
pub unsafe extern "C" fn wavry_start_client(host_ip: *const c_char, port: u16) -> i32 {
    if host_ip.is_null() {
        set_last_error("Client start failed: null host IP");
        return -2;
    }
    let c_str = CStr::from_ptr(host_ip);
    let host_str = match c_str.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            set_last_error("Client start failed: host IP is not UTF-8");
            return -3;
        }
    };

    clear_cloud_status();
    start_client_internal(Some((host_str, port)), None, "WavryAndroid".to_string())
}

#[no_mangle]
pub extern "C" fn wavry_stop() -> i32 {
    let mut guard = SESSION.lock().unwrap();
    if let Some(mut handle) = guard.take() {
        handle.stop();
        clear_last_error();
        clear_cloud_status();
        log::info!("Session stopped");
        0
    } else {
        log::warn!("No session to stop");
        set_last_error("Stop failed: no active session");
        -1
    }
}

// Stats Struct for C
#[repr(C)]
pub struct WavryStats {
    pub connected: bool,
    pub fps: u32,
    pub rtt_ms: u32,
    pub bitrate_kbps: u32,
    pub frames_encoded: u64,
    pub frames_decoded: u64,
}

#[no_mangle]
pub unsafe extern "C" fn wavry_get_stats(out: *mut WavryStats) -> i32 {
    if out.is_null() {
        set_last_error("Stats fetch failed: null output struct");
        return -1;
    }

    let guard = SESSION.lock().unwrap();
    if let Some(handle) = guard.as_ref() {
        let s = &handle.stats;
        let stats = WavryStats {
            connected: s.connected.load(std::sync::atomic::Ordering::Relaxed),
            fps: s.fps.load(std::sync::atomic::Ordering::Relaxed),
            rtt_ms: s.rtt_ms.load(std::sync::atomic::Ordering::Relaxed),
            bitrate_kbps: s.bitrate_kbps.load(std::sync::atomic::Ordering::Relaxed),
            frames_encoded: s.frames_encoded.load(std::sync::atomic::Ordering::Relaxed),
            frames_decoded: s.frames_decoded.load(std::sync::atomic::Ordering::Relaxed),
        };
        *out = stats;
        clear_last_error();
        0
    } else {
        // Zero out if not running
        *out = WavryStats {
            connected: false,
            fps: 0,
            rtt_ms: 0,
            bitrate_kbps: 0,
            frames_encoded: 0,
            frames_decoded: 0,
        };
        clear_last_error();
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_copy_last_error(
    out_buffer: *mut c_char,
    out_buffer_len: u32,
) -> i32 {
    if out_buffer.is_null() || out_buffer_len == 0 {
        return -1;
    }

    let guard = LAST_ERROR.lock().unwrap();
    let bytes = guard.as_bytes_with_nul();
    let max_len = out_buffer_len as usize;
    let copy_len = bytes.len().min(max_len);

    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, out_buffer, copy_len);
    if copy_len == max_len {
        *out_buffer.add(max_len - 1) = 0;
    }

    copy_len.saturating_sub(1) as i32
}

#[no_mangle]
pub unsafe extern "C" fn wavry_copy_last_cloud_status(
    out_buffer: *mut c_char,
    out_buffer_len: u32,
) -> i32 {
    if out_buffer.is_null() || out_buffer_len == 0 {
        return -1;
    }

    let guard = LAST_CLOUD_STATUS.lock().unwrap();
    let bytes = guard.as_bytes_with_nul();
    let max_len = out_buffer_len as usize;
    let copy_len = bytes.len().min(max_len);

    std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, out_buffer, copy_len);
    if copy_len == max_len {
        *out_buffer.add(max_len - 1) = 0;
    }

    copy_len.saturating_sub(1) as i32
}

#[no_mangle]
pub extern "C" fn wavry_init_renderer(layer_ptr: *mut std::ffi::c_void) -> i32 {
    log::info!("FFI: Init renderer with ptr {:?}", layer_ptr);
    #[cfg(target_os = "macos")]
    {
        match VideoRenderer::new(layer_ptr) {
            Ok(renderer) => {
                let mut guard = VIDEO_RENDERER.lock().unwrap();
                *guard = Some(Box::new(renderer));
                log::info!("FFI: Renderer initialized successfully");
                0
            }
            Err(e) => {
                log::error!("Failed to init renderer: {}", e);
                -1
            }
        }
    }
    #[cfg(target_os = "android")]
    {
        use wavry_media::{Codec, DecodeConfig, Resolution};
        // For Android, we default to H264/1080p for now, as we don't have the hello info yet
        let config = DecodeConfig {
            codec: Codec::H264,
            resolution: Resolution {
                width: 1920,
                height: 1080,
            },
            enable_10bit: false,
            enable_hdr: false,
        };
        match VideoRenderer::new(config, layer_ptr) {
            Ok(renderer) => {
                let mut guard = VIDEO_RENDERER.lock().unwrap();
                *guard = Some(Box::new(renderer));
                log::info!("FFI: Android Renderer initialized successfully");
                0
            }
            Err(e) => {
                log::error!("Failed to init Android renderer: {}", e);
                -1
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    {
        log::error!("FFI: Renderer init not supported on this platform via FFI");
        -1
    }
}

#[no_mangle]
pub extern "C" fn wavry_init_injector(width: u32, height: u32) -> i32 {
    #![allow(unused_variables)]
    #[cfg(target_os = "macos")]
    {
        let injector = MacInputInjector::new(width, height);
        let mut guard = INPUT_INJECTOR.lock().unwrap();
        *guard = Some(injector);
        log::info!("FFI: Input Injector initialized ({}x{})", width, height);
        0
    }
    #[cfg(target_os = "android")]
    {
        let injector = AndroidInputInjector::new(width, height);
        let mut guard = INPUT_INJECTOR.lock().unwrap();
        *guard = Some(injector);
        log::info!(
            "FFI: Android Input Injector initialized ({}x{})",
            width,
            height
        );
        0
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    {
        0
    }
}

#[no_mangle]
pub extern "C" fn wavry_test_input_injection() -> i32 {
    #[cfg(any(target_os = "macos", target_os = "android"))]
    {
        let mut guard = INPUT_INJECTOR.lock().unwrap();
        if let Some(injector) = guard.as_mut() {
            // Test: Move mouse to center
            if let Err(e) = injector.inject(InputEvent::MouseMove { x: 0.5, y: 0.5 }) {
                log::error!("Failed to inject test input: {}", e);
                return -2;
            }
            log::info!("FFI: Injected Mouse Center");
            0
        } else {
            -1
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "android")))]
    {
        -1
    }
}
