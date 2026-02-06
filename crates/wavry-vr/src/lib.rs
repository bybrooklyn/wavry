#![forbid(unsafe_code)]

pub mod adapter;
pub mod status;
pub mod types;

pub use adapter::{VrAdapter, VrAdapterCallbacks};
pub use status::{pcvr_status, set_pcvr_status};
pub use types::{EncoderControl, GamepadAxis, GamepadButton, GamepadInput, NetworkStats, Pose, PoseVelocity, StreamConfig, VideoCodec, VideoFrame, VrTiming};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VrError {
    #[error("adapter unavailable: {0}")]
    Unavailable(String),
    #[error("adapter error: {0}")]
    Adapter(String),
}

pub type VrResult<T> = Result<T, VrError>;
