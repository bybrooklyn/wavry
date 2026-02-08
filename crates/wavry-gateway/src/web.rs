use axum::{
    extract::{ConnectInfo, Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::SocketAddr;

use crate::signal::{ConnectionMap, SignalMessage};
use crate::{db, security};

#[cfg(feature = "webtransport-runtime")]
use serde_json;
#[cfg(feature = "webtransport-runtime")]
use std::collections::HashMap;
#[cfg(feature = "webtransport-runtime")]
use std::sync::Arc;
#[cfg(feature = "webtransport-runtime")]
use tokio::sync::{mpsc, RwLock};
#[cfg(feature = "webtransport-runtime")]
use wavry_common as common;
#[cfg(feature = "webtransport-runtime")]
use wavry_web as web_transport;

const MAX_SDP_BYTES: usize = 32 * 1024;
const MAX_CANDIDATE_BYTES: usize = 4096;

#[derive(Debug, Deserialize)]
pub struct WebRtcOfferRequest {
    pub session_token: String,
    pub target_username: String,
    pub sdp: String,
}

#[derive(Debug, Deserialize)]
pub struct WebRtcAnswerRequest {
    pub session_token: String,
    pub target_username: String,
    pub sdp: String,
}

#[derive(Debug, Deserialize)]
pub struct WebRtcCandidateRequest {
    pub session_token: String,
    pub target_username: String,
    pub candidate: String,
}

#[derive(Debug, Serialize)]
pub struct WebRtcRelayResponse {
    pub from_username: String,
    pub target_username: String,
    pub relayed: bool,
}

#[derive(Debug, Serialize)]
pub struct WebRtcConfigResponse {
    pub ws_signaling_url: String,
    pub offer_endpoint: String,
    pub answer_endpoint: String,
    pub candidate_endpoint: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

fn error_response(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
        .into_response()
}

fn ensure_webrtc_rate_limit(scope: &str, addr: SocketAddr) -> bool {
    let key = format!("{scope}:{}", addr.ip());
    security::allow_webrtc_request(&key)
}

fn validate_common(session_token: &str, target_username: &str) -> Result<(), &'static str> {
    if !security::is_valid_session_token(session_token) {
        return Err("Invalid session token");
    }
    if !security::is_valid_username(target_username) {
        return Err("Invalid target username");
    }
    Ok(())
}

pub async fn webrtc_config() -> impl IntoResponse {
    let ws_signaling_url =
        std::env::var("WS_SIGNALING_URL").unwrap_or_else(|_| "ws://localhost:3000/ws".to_string());
    Json(WebRtcConfigResponse {
        ws_signaling_url,
        offer_endpoint: "/webrtc/offer".to_string(),
        answer_endpoint: "/webrtc/answer".to_string(),
        candidate_endpoint: "/webrtc/candidate".to_string(),
    })
}

pub async fn webrtc_offer(
    State(pool): State<SqlitePool>,
    State(connections): State<ConnectionMap>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<WebRtcOfferRequest>,
) -> impl IntoResponse {
    if !ensure_webrtc_rate_limit("offer", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many signaling requests");
    }
    if let Err(msg) = validate_common(&payload.session_token, &payload.target_username) {
        return error_response(StatusCode::BAD_REQUEST, msg);
    }
    if payload.sdp.is_empty() || payload.sdp.len() > MAX_SDP_BYTES {
        return error_response(StatusCode::BAD_REQUEST, "Invalid SDP size");
    }

    relay_webrtc_message(
        pool,
        connections,
        payload.session_token,
        payload.target_username,
        |from_username| SignalMessage::Offer {
            target_username: from_username,
            sdp: payload.sdp,
        },
    )
    .await
}

pub async fn webrtc_answer(
    State(pool): State<SqlitePool>,
    State(connections): State<ConnectionMap>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<WebRtcAnswerRequest>,
) -> impl IntoResponse {
    if !ensure_webrtc_rate_limit("answer", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many signaling requests");
    }
    if let Err(msg) = validate_common(&payload.session_token, &payload.target_username) {
        return error_response(StatusCode::BAD_REQUEST, msg);
    }
    if payload.sdp.is_empty() || payload.sdp.len() > MAX_SDP_BYTES {
        return error_response(StatusCode::BAD_REQUEST, "Invalid SDP size");
    }

    relay_webrtc_message(
        pool,
        connections,
        payload.session_token,
        payload.target_username,
        |from_username| SignalMessage::Answer {
            target_username: from_username,
            sdp: payload.sdp,
        },
    )
    .await
}

pub async fn webrtc_candidate(
    State(pool): State<SqlitePool>,
    State(connections): State<ConnectionMap>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<WebRtcCandidateRequest>,
) -> impl IntoResponse {
    if !ensure_webrtc_rate_limit("candidate", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many signaling requests");
    }
    if let Err(msg) = validate_common(&payload.session_token, &payload.target_username) {
        return error_response(StatusCode::BAD_REQUEST, msg);
    }
    if payload.candidate.is_empty() || payload.candidate.len() > MAX_CANDIDATE_BYTES {
        return error_response(StatusCode::BAD_REQUEST, "Invalid ICE candidate size");
    }

    relay_webrtc_message(
        pool,
        connections,
        payload.session_token,
        payload.target_username,
        |from_username| SignalMessage::Candidate {
            target_username: from_username,
            candidate: payload.candidate,
        },
    )
    .await
}

async fn relay_webrtc_message<F>(
    pool: SqlitePool,
    connections: ConnectionMap,
    session_token: String,
    target_username: String,
    build_message: F,
) -> axum::response::Response
where
    F: FnOnce(String) -> SignalMessage,
{
    let from_username = match db::get_username_by_session_token(&pool, &session_token).await {
        Ok(Some(username)) => username,
        Ok(None) => {
            return error_response(StatusCode::UNAUTHORIZED, "Invalid or expired session token")
        }
        Err(err) => {
            tracing::error!("session token lookup failed: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed");
        }
    };

    let message = build_message(from_username.clone());
    let relayed = relay_message(&connections, &target_username, message).await;
    if !relayed {
        return error_response(
            StatusCode::NOT_FOUND,
            format!("Target user '{}' is not connected", target_username),
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(WebRtcRelayResponse {
            from_username,
            target_username,
            relayed: true,
        }),
    )
        .into_response()
}

async fn relay_message(
    connections: &ConnectionMap,
    target_username: &str,
    msg: SignalMessage,
) -> bool {
    let signaler = {
        let guard = connections.read().await;
        guard.get(target_username).cloned()
    };
    let Some(signaler) = signaler else {
        return false;
    };

    signaler.try_send(msg)
}

#[cfg(feature = "webtransport-runtime")]
pub struct GatewayWebTransportHandler {
    pub pool: sqlx::SqlitePool,
    pub connections: ConnectionMap,
    pub active_sessions: Arc<RwLock<HashMap<String, String>>>, // peer_addr -> username
    pub active_targets: Arc<RwLock<HashMap<String, String>>>,  // session_id -> target_username
    pub session_senders:
        Arc<RwLock<HashMap<String, mpsc::Sender<web_transport::ControlStreamFrame>>>>,
}

#[cfg(feature = "webtransport-runtime")]
impl web_transport::WebTransportSessionHandler for GatewayWebTransportHandler {
    fn on_session_started(&self, session: web_transport::WebTransportSession) {
        tracing::info!("webtransport session started: {}", session.session_id);
        let senders = self.session_senders.clone();
        tokio::spawn(async move {
            senders.write().await.insert(session.session_id, session.tx);
        });
    }

    fn on_input_datagram(&self, session_id: &str, datagram: web_transport::InputDatagram) {
        let active_targets = self.active_targets.clone();
        let connections = self.connections.clone();
        let session_id = session_id.to_string();

        tokio::spawn(async move {
            let target = active_targets.read().await.get(&session_id).cloned();
            if let Some(target_user) = target {
                let bytes = datagram.encode();
                let vec = bytes.to_vec();

                let guard = connections.read().await;
                if let Some(signaler) = guard.get(&target_user) {
                    signaler.try_send_binary(vec);
                }
            }
        });
    }

    fn on_control_frame(&self, session_id: &str, frame: web_transport::ControlStreamFrame) {
        let pool = self.pool.clone();
        let connections = self.connections.clone();
        let active_sessions = self.active_sessions.clone();
        let active_targets = self.active_targets.clone();
        let session_senders = self.session_senders.clone();
        let session_id = session_id.to_string();

        tokio::spawn(async move {
            match frame {
                web_transport::ControlStreamFrame::Control(msg) => {
                    match msg {
                        web_transport::ControlMessage::Connect { session_token, .. } => {
                            if let Ok(Some(username)) =
                                db::get_username_by_session_token(&pool, &session_token).await
                            {
                                let maybe_tx =
                                    session_senders.read().await.get(&session_id).cloned();
                                if let Some(tx) = maybe_tx {
                                    tracing::info!(
                                        "WebTransport client {} bound to user {}",
                                        session_id,
                                        username
                                    );
                                    active_sessions
                                        .write()
                                        .await
                                        .insert(session_id.clone(), username.clone());
                                    connections.write().await.insert(
                                        username,
                                        crate::signal::Signaler::WebTransport(tx),
                                    );
                                }
                            }
                        }
                        _ => {
                            // Handle signaling
                            if let Some(from_user) =
                                active_sessions.read().await.get(&session_id).cloned()
                            {
                                let signal = match msg {
                                    web_transport::ControlMessage::WebRtcOffer {
                                        target_username,
                                        sdp,
                                    } => {
                                        active_targets
                                            .write()
                                            .await
                                            .insert(session_id.clone(), target_username.clone());
                                        Some((
                                            target_username,
                                            crate::signal::SignalMessage::Offer {
                                                target_username: from_user,
                                                sdp,
                                            },
                                        ))
                                    }
                                    web_transport::ControlMessage::WebRtcAnswer {
                                        target_username,
                                        sdp,
                                    } => Some((
                                        target_username,
                                        crate::signal::SignalMessage::Answer {
                                            target_username: from_user,
                                            sdp,
                                        },
                                    )),
                                    web_transport::ControlMessage::WebRtcCandidate {
                                        target_username,
                                        candidate,
                                    } => Some((
                                        target_username,
                                        crate::signal::SignalMessage::Candidate {
                                            target_username: from_user,
                                            candidate,
                                        },
                                    )),
                                    _ => None,
                                };

                                if let Some((target, relayed_signal)) = signal {
                                    let guard = connections.read().await;
                                    if let Some(target_signaler) = guard.get(&target) {
                                        target_signaler.try_send(relayed_signal);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }
}

#[cfg(feature = "webtransport-runtime")]
pub async fn run_webtransport_runtime(
    bind_addr: &str,
    pool: sqlx::SqlitePool,
    connections: ConnectionMap,
) -> anyhow::Result<()> {
    let server = web_transport::WebTransportServer::bind(bind_addr).await?;
    server
        .run(GatewayWebTransportHandler {
            pool,
            connections,
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_targets: Arc::new(RwLock::new(HashMap::new())),
            session_senders: Arc::new(RwLock::new(HashMap::new())),
        })
        .await
}

#[derive(Debug, Deserialize)]
pub struct RelayReportRequest {
    pub session_token: String,
    pub relay_id: String,
    pub success: bool,
    pub bytes_sent: i64,
    pub duration_secs: i64,
}

pub async fn handle_relay_report(
    State(pool): State<SqlitePool>,
    Json(payload): Json<RelayReportRequest>,
) -> impl IntoResponse {
    // Basic verification: does the token exist?
    let _user_id = match db::get_username_by_session_token(&pool, &payload.session_token).await {
        Ok(Some(id)) => id,
        _ => return error_response(StatusCode::UNAUTHORIZED, "Invalid session token"),
    };

    if payload.success {
        db::record_relay_success(&pool, &payload.relay_id)
            .await
            .ok();
    } else {
        db::record_relay_failure(&pool, &payload.relay_id)
            .await
            .ok();
    }

    db::record_relay_usage(
        &pool,
        &payload.relay_id,
        "web-session", // Placeholder for actual session ID
        payload.bytes_sent,
        payload.duration_secs,
    )
    .await
    .ok();

    StatusCode::OK.into_response()
}

pub async fn handle_relay_reputation(State(_pool): State<SqlitePool>) -> impl IntoResponse {
    // This would ideally return a list of all known relay reputations
    // For now, return a placeholder or implement list in db.rs
    StatusCode::NOT_IMPLEMENTED.into_response()
}
