use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message as WsMessage, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server;
use webrtc::media::Sample;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;

use wavry_common::protocol::SignalMessage;
use wavry_media::EncodedFrame;

const SIGNALING_TLS_PINS_ENV: &str = "WAVRY_SIGNALING_TLS_PINS_SHA256";

pub struct WebRtcBridge {
    gateway_url: String,
    session_token: String,
    video_track: Arc<TrackLocalStaticSample>,
    peer_connection: Arc<Mutex<Option<RTCPeerConnection>>>,
    input_tx: mpsc::UnboundedSender<rift_core::input_message::Event>,
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn normalize_fingerprint(input: &str) -> Result<String> {
    let normalized: String = input.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if normalized.len() != 64 {
        return Err(anyhow!(
            "invalid certificate fingerprint length in {}: expected 64 hex chars",
            SIGNALING_TLS_PINS_ENV
        ));
    }
    Ok(normalized.to_ascii_lowercase())
}

fn parse_tls_pin_set(value: &str) -> Result<HashSet<String>> {
    let mut pins = HashSet::new();
    for raw in value.split([',', ';']) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        pins.insert(normalize_fingerprint(trimmed)?);
    }

    if pins.is_empty() {
        return Err(anyhow!(
            "{} is set but no usable certificate fingerprints were found",
            SIGNALING_TLS_PINS_ENV
        ));
    }
    Ok(pins)
}

fn configured_tls_pin_set() -> Result<Option<HashSet<String>>> {
    match std::env::var(SIGNALING_TLS_PINS_ENV) {
        Ok(value) => {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(parse_tls_pin_set(&value)?))
            }
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(anyhow!("failed to read {}: {e}", SIGNALING_TLS_PINS_ENV)),
    }
}

fn is_insecure_signaling_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("ws://")
}

fn is_secure_signaling_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("wss://")
}

fn validate_signaling_url(url: &str, tls_pin_set: Option<&HashSet<String>>) -> Result<()> {
    let insecure = is_insecure_signaling_url(url);
    let production = env_bool("WAVRY_ENVIRONMENT_PRODUCTION", false)
        || std::env::var("WAVRY_ENVIRONMENT")
            .map(|v| v.eq_ignore_ascii_case("production"))
            .unwrap_or(false);
    let allow_insecure = env_bool("WAVRY_ALLOW_INSECURE_SIGNALING", false);

    if insecure && production && !allow_insecure {
        return Err(anyhow!(
            "refusing insecure ws:// signaling URL in production; use wss:// or set WAVRY_ALLOW_INSECURE_SIGNALING=1"
        ));
    }

    if tls_pin_set.is_some() && !is_secure_signaling_url(url) {
        return Err(anyhow!(
            "{} requires a wss:// signaling URL",
            SIGNALING_TLS_PINS_ENV
        ));
    }

    Ok(())
}

fn fingerprint_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn validate_peer_certificate_pin(
    url: &str,
    ws: &WebSocketStream<MaybeTlsStream<TcpStream>>,
    tls_pin_set: &HashSet<String>,
) -> Result<()> {
    if !is_secure_signaling_url(url) {
        return Ok(());
    }

    let presented_fingerprints = match ws.get_ref() {
        MaybeTlsStream::Rustls(stream) => {
            let (_, session) = stream.get_ref();
            let certs = session
                .peer_certificates()
                .ok_or_else(|| anyhow!("signaling TLS peer did not provide certificates"))?;
            certs
                .iter()
                .map(|cert| fingerprint_sha256(cert.as_ref()))
                .collect::<Vec<_>>()
        }
        MaybeTlsStream::Plain(_) => {
            return Err(anyhow!(
                "expected TLS signaling stream for certificate pinning"
            ));
        }
        _ => {
            return Err(anyhow!(
                "unsupported signaling TLS backend for certificate pinning"
            ));
        }
    };

    if presented_fingerprints
        .iter()
        .any(|fingerprint| tls_pin_set.contains(fingerprint))
    {
        return Ok(());
    }

    let presented = presented_fingerprints
        .first()
        .cloned()
        .unwrap_or_else(|| "<missing>".to_string());
    Err(anyhow!(
        "signaling TLS certificate pin mismatch; expected one of {} configured fingerprint(s), got leaf sha256={}",
        tls_pin_set.len(),
        presented
    ))
}

impl WebRtcBridge {
    pub async fn new(
        gateway_url: String,
        session_token: String,
        input_tx: mpsc::UnboundedSender<rift_core::input_message::Event>,
    ) -> Result<Self> {
        let mut m = MediaEngine::default();
        m.register_default_codecs()?;
        let _api = APIBuilder::new().with_media_engine(m).build();

        let video_track = Arc::new(TrackLocalStaticSample::new(
            webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "webrtc-rs".to_string(),
        ));

        Ok(Self {
            gateway_url,
            session_token,
            video_track,
            peer_connection: Arc::new(Mutex::new(None)),
            input_tx,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let tls_pin_set = configured_tls_pin_set()?;
        validate_signaling_url(&self.gateway_url, tls_pin_set.as_ref())?;
        let (mut ws_stream, _) = connect_async(&self.gateway_url).await?;
        if let Some(tls_pin_set) = tls_pin_set.as_ref() {
            validate_peer_certificate_pin(&self.gateway_url, &ws_stream, tls_pin_set)?;
        }
        info!("Connected to signaling gateway: {}", self.gateway_url);

        // Bind to session
        let bind_msg = SignalMessage::BIND {
            token: self.session_token.clone(),
        };
        ws_stream
            .send(WsMessage::Text(serde_json::to_string(&bind_msg)?))
            .await?;

        let (mut write, mut read) = ws_stream.split();
        let (signal_tx, mut signal_rx) = mpsc::channel::<SignalMessage>(32);

        // Task to send signals back to gateway
        tokio::spawn(async move {
            while let Some(signal) = signal_rx.recv().await {
                if let Ok(text) = serde_json::to_string(&signal) {
                    if let Err(e) = write.send(WsMessage::Text(text)).await {
                        error!("Failed to send signaling message: {}", e);
                        break;
                    }
                }
            }
        });

        while let Some(msg) = read.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    let signal: SignalMessage = match serde_json::from_str(&text) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to parse signaling message: {}", e);
                            continue;
                        }
                    };
                    self.handle_signal(signal, signal_tx.clone()).await?;
                }
                Ok(WsMessage::Close(_)) => break,
                Err(e) => {
                    error!("Signaling WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn handle_signal(
        &self,
        signal: SignalMessage,
        tx: mpsc::Sender<SignalMessage>,
    ) -> Result<()> {
        match signal {
            SignalMessage::OFFER {
                target_username,
                sdp,
                ..
            } => {
                info!("Received WebRTC offer from {}", target_username);
                let answer_sdp = self
                    .create_answer(sdp, target_username.clone(), tx.clone())
                    .await?;
                tx.send(SignalMessage::ANSWER {
                    target_username,
                    sdp: answer_sdp,
                    public_addr: None,
                })
                .await?;
            }
            SignalMessage::CANDIDATE {
                target_username,
                candidate,
            } => {
                debug!("Received ICE candidate from {}", target_username);
                let pc_guard = self.peer_connection.lock().await;
                if let Some(pc) = &*pc_guard {
                    if let Ok(ice_candidate) = serde_json::from_str(&candidate) {
                        pc.add_ice_candidate(ice_candidate).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn create_answer(
        &self,
        offer_sdp: String,
        target_username: String,
        tx: mpsc::Sender<SignalMessage>,
    ) -> Result<String> {
        let mut m = MediaEngine::default();
        m.register_default_codecs()?;
        let api = APIBuilder::new().with_media_engine(m).build();

        let config = webrtc::peer_connection::configuration::RTCConfiguration {
            ice_servers: vec![ice_server::RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let pc = api.new_peer_connection(config).await?;

        let track = Arc::clone(&self.video_track);
        pc.add_track(track as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        let input_tx = self.input_tx.clone();
        pc.on_data_channel(Box::new(move |d| {
            let input_tx = input_tx.clone();
            Box::pin(async move {
                if d.label() == "input" {
                    info!("WebRTC input data channel opened");
                    d.on_message(Box::new(move |msg| {
                        let input_tx = input_tx.clone();
                        Box::pin(async move {
                            if let Ok(input_msg) = rift_core::InputMessage::decode(msg.data) {
                                if let Some(event) = input_msg.event {
                                    let _ = input_tx.send(event);
                                }
                            }
                        })
                    }));
                }
            })
        }));

        pc.on_ice_candidate(Box::new(move |c| {
            let tx = tx.clone();
            let target = target_username.clone();
            Box::pin(async move {
                if let Some(candidate) = c {
                    let _ = tx
                        .send(SignalMessage::CANDIDATE {
                            target_username: target,
                            candidate: candidate.to_json().unwrap().candidate,
                        })
                        .await;
                }
            })
        }));

        pc.set_remote_description(RTCSessionDescription::offer(offer_sdp).unwrap())
            .await?;

        let answer = pc.create_answer(None).await?;
        pc.set_local_description(answer.clone()).await?;

        let mut pc_guard = self.peer_connection.lock().await;
        *pc_guard = Some(pc);

        Ok(answer.sdp)
    }

    pub async fn push_frame(&self, frame: EncodedFrame) -> Result<()> {
        // Only push if we have an active connection
        let pc_guard = self.peer_connection.lock().await;
        if pc_guard.is_none() {
            return Ok(());
        }

        self.video_track
            .write_sample(&Sample {
                data: frame.data.into(),
                duration: std::time::Duration::from_micros(16666), // 60fps approx
                ..Default::default()
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_fingerprint, parse_tls_pin_set};

    #[test]
    fn test_normalize_fingerprint_accepts_colons_and_case() {
        let fp = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";
        let normalized = normalize_fingerprint(fp).expect("normalize");
        assert_eq!(normalized.len(), 64);
        assert!(normalized.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(normalized, normalized.to_ascii_lowercase());
    }

    #[test]
    fn test_parse_tls_pin_set_multiple_entries() {
        let value = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef;aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let pins = parse_tls_pin_set(value).expect("parse");
        assert_eq!(pins.len(), 2);
    }

    #[test]
    fn test_parse_tls_pin_set_rejects_invalid_length() {
        assert!(parse_tls_pin_set("abcd").is_err());
    }
}
