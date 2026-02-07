use std::sync::{atomic::AtomicU32, Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// Global session state for the desktop app
pub struct SessionState {
    pub stop_tx: Option<oneshot::Sender<()>>,
    pub cc_config_tx: Option<mpsc::UnboundedSender<rift_core::cc::DeltaConfig>>,
    pub current_bitrate: Arc<AtomicU32>,
    pub cc_state: Arc<Mutex<String>>,
}

pub struct ClientSessionState {
    pub stop_tx: Option<oneshot::Sender<()>>,
}

pub struct AuthState {
    pub token: String,
    pub signaling_url: String,
}

pub static SESSION_STATE: Mutex<Option<SessionState>> = Mutex::new(None);
pub static CLIENT_SESSION_STATE: Mutex<Option<ClientSessionState>> = Mutex::new(None);
pub static AUTH_STATE: Mutex<Option<AuthState>> = Mutex::new(None);
pub static IDENTITY_KEY: Mutex<Option<rift_crypto::IdentityKeypair>> = Mutex::new(None);
