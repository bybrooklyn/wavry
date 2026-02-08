use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRtcStartParams {
    pub session_token: String,
    pub offer_sdp: String,
}

/// Skeleton for WebRTC signaling integration.
#[derive(Debug)]
pub struct WebRtcPeer {
    pub peer_id: String,
}

/// Signaling interface between browser and host.
pub trait WebRtcSignaling: Send + Sync + 'static {
    fn on_offer(&self, params: WebRtcStartParams) -> anyhow::Result<String>;
    fn on_answer(&self, peer_id: &str, sdp: String) -> anyhow::Result<()>;
    fn on_ice_candidate(&self, peer_id: &str, candidate: String);
}
