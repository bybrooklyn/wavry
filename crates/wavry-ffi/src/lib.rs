#![allow(clippy::missing_safety_doc)]

use once_cell::sync::Lazy;
use std::ffi::{c_char, CStr};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

#[cfg(not(target_os = "macos"))]
use wavry_media::DummyRenderer as MacVideoRenderer;
#[cfg(target_os = "macos")]
use wavry_media::{MacInputInjector, MacVideoRenderer};

// Stub for Linux input injector if needed, or use a dummy
#[cfg(not(target_os = "macos"))]
pub struct MacInputInjector;
#[cfg(not(target_os = "macos"))]
impl MacInputInjector {
    pub fn new(_w: u32, _h: u32) -> Self {
        Self
    }
    pub fn inject(&mut self, _e: InputEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

use wavry_media::InputEvent;
#[cfg(target_os = "macos")]
use wavry_media::InputInjector;

mod session;
use session::{run_client, run_host, HostRuntimeConfig, SessionHandle, SessionStats};

mod identity;
mod signaling_ffi;

// Global State
static RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

static SESSION: Mutex<Option<SessionHandle>> = Mutex::new(None);

// Shared media resources (FFI -> Rust)
// We wrap MacVideoRenderer in a way that it can be used by both FFI Init and Client Loop
// But MacVideoRenderer is not Sync/Send? It is Send, not Sync?
// Actually I implemented `unsafe impl Send for MacVideoRenderer` so it's Send.
// Mutex makes it Sync.
static VIDEO_RENDERER: Lazy<Arc<Mutex<Option<Box<MacVideoRenderer>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));
#[allow(dead_code)]
static INPUT_INJECTOR: Lazy<Arc<Mutex<Option<MacInputInjector>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

#[no_mangle]
pub extern "C" fn wavry_init() {
    // Initialize logger if not already
    let _ = env_logger::try_init();
    log::info!("Wavry Core (FFI) Initialized 🚀");
}

#[no_mangle]
pub unsafe extern "C" fn wavry_init_identity(storage_path_ptr: *const c_char) -> i32 {
    if storage_path_ptr.is_null() {
        return -1;
    }
    let c_str = CStr::from_ptr(storage_path_ptr);
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };

    match identity::init_identity(path_str) {
        Ok(_) => 0,
        Err(e) => {
            log::error!("Failed to init identity: {}", e);
            -3
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_get_public_key(out_buffer: *mut u8) -> i32 {
    if out_buffer.is_null() {
        return -1;
    }

    if let Some(pub_key) = identity::get_public_key() {
        std::ptr::copy_nonoverlapping(pub_key.as_ptr(), out_buffer, 32);
        0
    } else {
        // Identity not initialized
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
        return -1;
    }

    let stats = Arc::new(SessionStats::default());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (init_tx, init_rx) = tokio::sync::oneshot::channel();

    let stats_clone = stats.clone();
    RUNTIME.spawn(async move {
        if let Err(e) = run_host(port, host_config, stats_clone, rx, init_tx).await {
            log::error!("Host error: {}", e);
        }
    });

    match init_rx.blocking_recv() {
        Ok(Ok(())) => {
            *guard = Some(SessionHandle {
                stop_tx: Some(tx),
                stats,
            });
            log::info!(
                "Started Host on port {} ({}x{} @ {}fps, {} kbps, keyframe {}ms, display {:?})",
                port,
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
            -2
        }
        Err(_) => {
            log::error!("Host init channel closed unexpectedly");
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
        return -4;
    }

    let raw = &*config_ptr;
    let config = normalize_host_config(raw);
    start_host_internal(port, config)
}

/// Start Client Mode (UDP Stream -> Remote Display)
#[no_mangle]
pub unsafe extern "C" fn wavry_start_client(host_ip: *const c_char, port: u16) -> i32 {
    let mut guard = SESSION.lock().unwrap();
    if guard.is_some() {
        log::warn!("Session already running");
        return -1;
    }

    if host_ip.is_null() {
        return -2;
    }
    let c_str = CStr::from_ptr(host_ip);
    let host_str = match c_str.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -3,
    };

    let stats = Arc::new(SessionStats::default());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (init_tx, init_rx) = tokio::sync::oneshot::channel();

    let stats_clone = stats.clone();
    let renderer = VIDEO_RENDERER.clone(); // Shared Reference

    RUNTIME.spawn(async move {
        if let Err(e) = run_client(host_str, port, renderer, stats_clone, rx, init_tx).await {
            log::error!("Client error: {}", e);
        }
    });

    // Wait for initialization
    match init_rx.blocking_recv() {
        Ok(Ok(())) => {
            *guard = Some(SessionHandle {
                stop_tx: Some(tx),
                stats,
            });
            log::info!(
                "Started Client connecting to {}:{}",
                c_str.to_string_lossy(),
                port
            );
            0
        }
        Ok(Err(e)) => {
            log::error!("Failed to start client: {}", e);
            -4
        }
        Err(_) => {
            log::error!("Client init channel closed");
            -5
        }
    }
}

#[no_mangle]
pub extern "C" fn wavry_stop() -> i32 {
    let mut guard = SESSION.lock().unwrap();
    if let Some(mut handle) = guard.take() {
        handle.stop();
        log::info!("Session stopped");
        0
    } else {
        log::warn!("No session to stop");
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
        0
    }
}

#[no_mangle]
pub extern "C" fn wavry_init_renderer(layer_ptr: *mut std::ffi::c_void) -> i32 {
    log::info!("FFI: Init renderer with ptr {:?}", layer_ptr);
    #[cfg(target_os = "macos")]
    {
        match MacVideoRenderer::new(layer_ptr) {
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
    #[cfg(not(target_os = "macos"))]
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
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

#[no_mangle]
pub extern "C" fn wavry_test_input_injection() -> i32 {
    #[cfg(target_os = "macos")]
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
    #[cfg(not(target_os = "macos"))]
    {
        -1
    }
}
