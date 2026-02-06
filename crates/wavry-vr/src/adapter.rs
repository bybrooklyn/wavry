use std::sync::Arc;

use crate::{
    types::{
        EncoderControl, GamepadInput, HandPose, NetworkStats, Pose, StreamConfig, VideoFrame,
        VrTiming,
    },
    VrResult,
};

pub trait VrAdapterCallbacks: Send + Sync {
    // ALVR -> Wavry
    fn on_video_frame(&self, frame: VideoFrame, timestamp_us: u64, frame_id: u64);
    fn on_pose_update(&self, pose: Pose, timestamp_us: u64);
    fn on_hand_pose_update(&self, hand_pose: HandPose, timestamp_us: u64);
    fn on_vr_timing(&self, timing: VrTiming);
    fn on_gamepad_input(&self, input: GamepadInput);
}

pub trait VrAdapter: Send {
    fn start(&mut self, cb: Arc<dyn VrAdapterCallbacks>) -> VrResult<()>;
    fn stop(&mut self);

    // Wavry -> ALVR (frame submission)
    fn submit_video(&mut self, frame: VideoFrame) -> VrResult<()>;
    fn submit_pose(&mut self, pose: Pose, timestamp_us: u64) -> VrResult<()>;
    fn configure_stream(&mut self, config: StreamConfig);

    // Wavry -> ALVR (transport/encoder signals)
    fn on_network_stats(&mut self, stats: NetworkStats);
    fn on_encoder_control(&mut self, control: EncoderControl);
}
