use bytes::Bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy)]
pub struct StreamConfig {
    pub codec: VideoCodec,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Pose {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PoseVelocity {
    pub linear: [f32; 3],
    pub angular: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
pub struct HandPose {
    pub hand_id: u32, // 0 = left, 1 = right
    pub pose: Pose,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
pub struct VrTiming {
    pub refresh_hz: f32,
    pub vsync_offset_us: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GamepadAxis {
    pub axis: u32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GamepadButton {
    pub button: u32,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GamepadInput {
    pub timestamp_us: u64,
    pub gamepad_id: u32,
    pub axes: Vec<GamepadAxis>,
    pub buttons: Vec<GamepadButton>,
}

#[derive(Debug, Clone, Copy)]
pub struct NetworkStats {
    pub rtt_us: u64,
    pub jitter_us: u32,
    pub loss_ratio: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EncoderControl {
    pub skip_frames: u32,
}

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub timestamp_us: u64,
    pub frame_id: u64,
    pub keyframe: bool,
    pub data: Bytes,
}
