use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
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

fn validate_signaling_url(url: &str) -> Result<()> {
    let insecure = url.trim().to_ascii_lowercase().starts_with("ws://");
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

    Ok(())
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
        validate_signaling_url(&self.gateway_url)?;
        let (mut ws_stream, _) = connect_async(&self.gateway_url).await?;
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
