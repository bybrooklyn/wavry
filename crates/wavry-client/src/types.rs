use anyhow::Result;
use rift_crypto::connection::SecureClient;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
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
    pub master_url: Option<String>,
    pub max_resolution: Option<MediaResolution>,
    pub gamepad_enabled: bool,
    pub gamepad_deadzone: f32,
    pub vr_adapter: Option<Arc<Mutex<dyn VrAdapter>>>,
    pub runtime_stats: Option<Arc<ClientRuntimeStats>>,
    pub recorder_config: Option<wavry_media::RecorderConfig>,
    pub send_files: Vec<PathBuf>,
    pub file_out_dir: PathBuf,
    pub file_max_bytes: u64,
    pub file_command_bus: Option<tokio::sync::broadcast::Sender<FileTransferCommand>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTransferAction {
    Pause,
    Resume,
    Cancel,
    Retry,
}

impl FileTransferAction {
    pub const fn as_protocol_message(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Cancel => "cancel",
            Self::Retry => "retry",
        }
    }
}

impl fmt::Display for FileTransferAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_protocol_message())
    }
}

impl FromStr for FileTransferAction {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pause" => Ok(Self::Pause),
            "resume" => Ok(Self::Resume),
            "cancel" | "canceled" => Ok(Self::Cancel),
            "retry" => Ok(Self::Retry),
            _ => Err("expected one of: pause, resume, cancel, retry"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTransferCommand {
    pub file_id: u64,
    pub action: FileTransferAction,
}

#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub relay_id: String,
    pub addr: SocketAddr,
    pub token: String,
    pub session_id: Uuid,
}

#[derive(Debug, Default)]
pub struct ClientRuntimeStats {
    pub connected: AtomicBool,
    pub frames_decoded: AtomicU64,
    pub monitors: Mutex<Vec<rift_core::MonitorInfo>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_creation() {
        let config = ClientConfig {
            connect_addr: Some("192.168.1.1:5000".parse().unwrap()),
            client_name: "TestClient".to_string(),
            no_encrypt: false,
            identity_key: Some([42u8; 32]),
            relay_info: None,
            master_url: None,
            max_resolution: None,
            gamepad_enabled: true,
            gamepad_deadzone: 0.15,
            vr_adapter: None,
            runtime_stats: None,
            recorder_config: None,
            send_files: Vec::new(),
            file_out_dir: PathBuf::from("received-files"),
            file_max_bytes: wavry_common::file_transfer::DEFAULT_MAX_FILE_BYTES,
            file_command_bus: None,
        };

        assert_eq!(config.client_name, "TestClient");
        assert!(!config.no_encrypt);
        assert!(config.gamepad_enabled);
        assert_eq!(config.gamepad_deadzone, 0.15);
    }

    #[test]
    fn test_client_config_clone() {
        let config1 = ClientConfig {
            connect_addr: Some("127.0.0.1:5000".parse().unwrap()),
            client_name: "Clone Test".to_string(),
            no_encrypt: true,
            identity_key: None,
            relay_info: None,
            master_url: Some("http://localhost:8080".to_string()),
            max_resolution: Some(wavry_media::Resolution {
                width: 1920,
                height: 1080,
            }),
            gamepad_enabled: false,
            gamepad_deadzone: 0.0,
            vr_adapter: None,
            runtime_stats: None,
            recorder_config: None,
            send_files: Vec::new(),
            file_out_dir: PathBuf::from("received-files"),
            file_max_bytes: wavry_common::file_transfer::DEFAULT_MAX_FILE_BYTES,
            file_command_bus: None,
        };

        let config2 = config1.clone();

        assert_eq!(config1.client_name, config2.client_name);
        assert_eq!(config1.no_encrypt, config2.no_encrypt);
        assert_eq!(config1.connect_addr, config2.connect_addr);
    }

    #[test]
    fn test_relay_info_creation() {
        let relay = RelayInfo {
            relay_id: "relay-001".to_string(),
            addr: "203.0.113.1:9000".parse().unwrap(),
            token: "secret-token-abc123".to_string(),
            session_id: uuid::Uuid::new_v4(),
        };

        assert_eq!(relay.relay_id, "relay-001");
        assert!(!relay.token.is_empty());
    }

    #[test]
    fn test_relay_info_clone() {
        let relay1 = RelayInfo {
            relay_id: "relay-002".to_string(),
            addr: "198.51.100.1:9000".parse().unwrap(),
            token: "token-xyz".to_string(),
            session_id: uuid::Uuid::nil(),
        };

        let relay2 = relay1.clone();

        assert_eq!(relay1.relay_id, relay2.relay_id);
        assert_eq!(relay1.addr, relay2.addr);
        assert_eq!(relay1.session_id, relay2.session_id);
    }

    #[test]
    fn test_client_runtime_stats_default() {
        let stats = ClientRuntimeStats::default();

        assert!(!stats.connected.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(
            stats
                .frames_decoded
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(stats.monitors.lock().unwrap().is_empty());
    }

    #[test]
    fn test_client_runtime_stats_atomic_operations() {
        let stats = ClientRuntimeStats::default();

        stats
            .connected
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(stats.connected.load(std::sync::atomic::Ordering::Relaxed));

        stats
            .frames_decoded
            .store(100, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            stats
                .frames_decoded
                .load(std::sync::atomic::Ordering::Relaxed),
            100
        );

        // Test increment
        stats
            .frames_decoded
            .fetch_add(50, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            stats
                .frames_decoded
                .load(std::sync::atomic::Ordering::Relaxed),
            150
        );
    }

    #[test]
    fn test_crypto_state_debug() {
        let disabled = CryptoState::Disabled;
        assert_eq!(format!("{:?}", disabled), "Disabled");
    }

    #[test]
    fn test_media_resolution_values() {
        let res = wavry_media::Resolution {
            width: 1920,
            height: 1080,
        };

        assert_eq!(res.width, 1920);
        assert_eq!(res.height, 1080);
    }
}
