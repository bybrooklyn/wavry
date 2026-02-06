// Derived from ALVR (MIT)
// Original copyright preserved

//! ALVR adapter wiring for Wavry.
//!
//! This module intentionally keeps transport out of ALVR. It uses the adapter
//! traits in `wavry-vr` and connects to the VR runtime backends.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;

use wavry_vr::{VrAdapter, VrAdapterCallbacks, VrError, VrResult};
use wavry_vr::types::{EncoderControl, NetworkStats, Pose, StreamConfig, VideoFrame};
use glam::{Quat, Vec3};

// Minimal ALVR primitives (vendored) for compatibility with ALVR types.
#[allow(dead_code)]
#[path = "../../../third_party/alvr/alvr/common/src/primitives.rs"]
mod alvr_primitives;

#[path = "pcvr.rs"]
mod pcvr;

pub(crate) struct SharedState {
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    pub(crate) callbacks: Arc<dyn VrAdapterCallbacks>,
    pub(crate) latest_frame: Mutex<Option<VideoFrame>>,
    pub(crate) stream_config: Mutex<Option<StreamConfig>>,
    pub(crate) stop: AtomicBool,
}

impl SharedState {
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    pub(crate) fn take_latest_frame(&self) -> Option<VideoFrame> {
        self.latest_frame.lock().ok()?.take()
    }
}

pub struct AlvrAdapter {
    state: Option<Arc<SharedState>>,
    runtime: Option<JoinHandle<()>>,
}

impl AlvrAdapter {
    pub fn new() -> Self {
        Self {
            state: None,
            runtime: None,
        }
    }
}

impl Default for AlvrAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl VrAdapter for AlvrAdapter {
    fn start(&mut self, cb: Arc<dyn VrAdapterCallbacks>) -> VrResult<()> {
        let state = Arc::new(SharedState {
            callbacks: cb,
            latest_frame: Mutex::new(None),
            stream_config: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        let runtime = pcvr::spawn_runtime(state.clone())?;
        self.state = Some(state);
        self.runtime = Some(runtime);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(state) = self.state.as_ref() {
            state.stop.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.runtime.take() {
            let _ = handle.join();
        }
    }

    fn submit_video(&mut self, frame: VideoFrame) -> VrResult<()> {
        if let Some(state) = self.state.as_ref() {
            if let Ok(mut slot) = state.latest_frame.lock() {
                *slot = Some(frame);
            }
            Ok(())
        } else {
            Err(VrError::Adapter("adapter not started".to_string()))
        }
    }

    fn submit_pose(&mut self, pose: Pose, _timestamp_us: u64) -> VrResult<()> {
        // Pose submission hook for server-side OpenVR integration.
        let _alvr_pose = alvr_primitives::Pose {
            orientation: Quat::from_xyzw(
                pose.orientation[0],
                pose.orientation[1],
                pose.orientation[2],
                pose.orientation[3],
            ),
            position: Vec3::new(pose.position[0], pose.position[1], pose.position[2]),
        };
        Ok(())
    }

    fn configure_stream(&mut self, config: StreamConfig) {
        if let Some(state) = self.state.as_ref() {
            if let Ok(mut cfg) = state.stream_config.lock() {
                *cfg = Some(config);
            }
        }
    }

    fn on_network_stats(&mut self, stats: NetworkStats) {
        let _ = stats;
    }

    fn on_encoder_control(&mut self, control: EncoderControl) {
        let _ = control;
    }
}
