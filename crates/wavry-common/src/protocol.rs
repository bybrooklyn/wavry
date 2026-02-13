use serde::{Deserialize, Serialize};

/// Global signaling message for coordination and NAT traversal.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[allow(non_camel_case_types)]
pub enum SignalMessage {
    /// Initial binding of a connection to a specific session/token.
    BIND { token: String },

    /// RIFT-v1 SDP Exchange: OFFER (base64 encoded rift::Hello)
    OFFER_RIFT {
        target_username: String,
        hello_base64: String,
    },

    /// RIFT-v1 SDP Exchange: ANSWER (base64 encoded rift::HelloAck)
    ANSWER_RIFT {
        target_username: String,
        ack_base64: String,
    },

    /// WebRTC-style OFFER (legacy/fallback)
    OFFER {
        target_username: String,
        sdp: String,
        public_addr: Option<String>,
    },

    /// WebRTC-style ANSWER (legacy/fallback)
    ANSWER {
        target_username: String,
        sdp: String,
        public_addr: Option<String>,
    },

    /// ICE candidate for NAT traversal (not yet fully used by direct UDP).
    CANDIDATE {
        target_username: String,
        candidate: String,
    },

    /// Request a fallback blind relay for the target session.
    REQUEST_RELAY {
        target_username: String,
        #[serde(default)]
        region: Option<String>,
    },

    /// Received credentials for a blind relay session.
    RELAY_CREDENTIALS {
        relay_id: String,
        token: String,
        addr: String,
        session_id: uuid::Uuid,
    },

    /// Generic error message from the signaling server.
    ERROR { code: Option<u16>, message: String },
}

/// Request for a relay to register with the Master server.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayRegisterRequest {
    pub relay_id: String,
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub asn: Option<u32>,
    #[serde(default)]
    pub max_sessions: Option<u32>,
    #[serde(default)]
    pub max_bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub features: Vec<String>,
}

/// Response from the Master server upon successful relay registration.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayRegisterResponse {
    pub heartbeat_interval_ms: u64,
    pub master_public_key: Vec<u8>,
    #[serde(default)]
    pub master_key_id: Option<String>,
}

/// Periodic heartbeat from a relay to the Master server.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayHeartbeatRequest {
    pub relay_id: String,
    pub load_pct: f32,
}

/// Request for a user to register with a display name.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RegisterRequest {
    pub wavry_id: String,
    pub display_name: String,
}

/// Request to verify identity via Ed25519 signature.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VerifyRequest {
    pub wavry_id: String,
    pub signature_hex: String,
}

/// Signed quality report for a relay session.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelayFeedbackRequest {
    pub session_id: uuid::Uuid,
    pub relay_id: String,
    pub quality_score: u8, // 0-100
    pub issues: Vec<String>,
    pub signature: String,
}
