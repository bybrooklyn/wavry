#![allow(unsafe_code)]

#[cfg(feature = "alvr")]
mod vendor;

#[cfg(feature = "alvr")]
pub use vendor::AlvrAdapter;

#[cfg(not(feature = "alvr"))]
mod stub {
    use std::sync::Arc;

    use wavry_vr::{VrAdapter, VrAdapterCallbacks, VrError, VrResult};
    use wavry_vr::types::{Pose, StreamConfig, VideoFrame};

    pub struct AlvrAdapter {
        _callback: Option<Arc<dyn VrAdapterCallbacks>>,
    }

    impl AlvrAdapter {
        pub fn new() -> Self {
            Self { _callback: None }
        }
    }

    impl Default for AlvrAdapter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl VrAdapter for AlvrAdapter {
        fn start(&mut self, cb: Arc<dyn VrAdapterCallbacks>) -> VrResult<()> {
            self._callback = Some(cb);
            Err(VrError::Unavailable(
                "ALVR adapter not enabled. Build with feature 'alvr'.".to_string(),
            ))
        }

        fn stop(&mut self) {}

        fn submit_video(&mut self, _frame: VideoFrame) -> VrResult<()> {
            Err(VrError::Unavailable(
                "ALVR adapter not enabled. Build with feature 'alvr'.".to_string(),
            ))
        }

        fn submit_pose(&mut self, _pose: Pose, _timestamp_us: u64) -> VrResult<()> {
            Err(VrError::Unavailable(
                "ALVR adapter not enabled. Build with feature 'alvr'.".to_string(),
            ))
        }

        fn configure_stream(&mut self, _config: StreamConfig) {}

        fn on_network_stats(&mut self, _stats: wavry_vr::types::NetworkStats) {}

        fn on_encoder_control(&mut self, _control: wavry_vr::types::EncoderControl) {}
    }

}

#[cfg(not(feature = "alvr"))]
pub use stub::AlvrAdapter;
