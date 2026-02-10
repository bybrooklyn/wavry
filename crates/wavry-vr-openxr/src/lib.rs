use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::thread::JoinHandle;

use wavry_vr::types::{StreamConfig, VideoFrame};
use wavry_vr::{VrAdapterCallbacks, VrResult};

pub mod common;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "android")]
pub mod android;

pub struct SharedState {
    pub callbacks: Arc<dyn VrAdapterCallbacks>,
    pub latest_frame: Mutex<Option<VideoFrame>>,
    pub stream_config: Mutex<Option<StreamConfig>>,
    pub stop: AtomicBool,
}

impl SharedState {
    pub fn new(callbacks: Arc<dyn VrAdapterCallbacks>) -> Self {
        Self {
            callbacks,
            latest_frame: Mutex::new(None),
            stream_config: Mutex::new(None),
            stop: AtomicBool::new(false),
        }
    }

    pub fn take_latest_frame(&self) -> Option<VideoFrame> {
        self.latest_frame.lock().ok()?.take()
    }
}

pub fn spawn_runtime(state: Arc<SharedState>) -> VrResult<JoinHandle<()>> {
    #[cfg(target_os = "linux")]
    return linux::spawn(state);

    #[cfg(target_os = "windows")]
    return windows::spawn(state);

    #[cfg(target_os = "android")]
    return android::spawn(state);

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
    {
        let _ = state;
        Err(wavry_vr::VrError::Unavailable(
            "Unsupported platform for OpenXR".to_string(),
        ))
    }
}
