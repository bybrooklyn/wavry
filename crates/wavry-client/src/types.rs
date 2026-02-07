use anyhow::Result;
use rift_crypto::connection::SecureClient;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Arc, Mutex,
};
use uuid::Uuid;
use wavry_media::{DecodeConfig, Renderer, Resolution as MediaResolution};
use wavry_vr::VrAdapter;

#[derive(Clone)]
pub struct ClientConfig {
    pub connect_addr: Option<SocketAddr>,
    pub client_name: String,
    pub no_encrypt: bool,
    pub identity_key: Option<[u8; 32]>,
    pub relay_info: Option<RelayInfo>,
    pub max_resolution: Option<MediaResolution>,
    pub gamepad_enabled: bool,
    pub gamepad_deadzone: f32,
    pub vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>>,
    pub runtime_stats: Option<Arc<ClientRuntimeStats>>,
}

#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub addr: SocketAddr,
    pub token: String,
    pub session_id: Uuid,
}

#[derive(Debug, Default)]
pub struct ClientRuntimeStats {
    pub connected: AtomicBool,
    pub frames_decoded: AtomicU64,
}

pub type RendererFactory = Box<dyn Fn(DecodeConfig) -> Result<Box<dyn Renderer + Send>> + Send>;

/// Crypto state for the client
pub enum CryptoState {
    /// No encryption (--no-encrypt mode)
    Disabled,
    /// Crypto handshake in progress
    Handshaking(SecureClient),
    /// Crypto established
    Established(SecureClient),
}

impl fmt::Debug for CryptoState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "Disabled"),
            Self::Handshaking(_) => write!(f, "Handshaking"),
            Self::Established(_) => write!(f, "Established"),
        }
    }
}

pub enum VrOutbound {
    Pose(rift_core::PoseUpdate),
    HandPose(rift_core::HandPoseUpdate),
    Timing(rift_core::VrTiming),
    Gamepad(rift_core::InputMessage),
}
