use axum::{
    extract::{ws::Message, ConnectInfo, Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::SocketAddr;

use crate::signal::{ConnectionMap, SignalMessage};
use crate::{db, security};

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
    let tx = {
        let guard = connections.read().await;
        guard.get(target_username).cloned()
    };
    let Some(tx) = tx else {
        return false;
    };

    match serde_json::to_string(&msg) {
        Ok(json) => tx.try_send(Message::Text(json)).is_ok(),
        Err(err) => {
            tracing::warn!("failed to serialize relayed signaling message: {}", err);
            false
        }
    }
}

#[cfg(feature = "webtransport-runtime")]
pub struct GatewayWebTransportHandler;

#[cfg(feature = "webtransport-runtime")]
impl wavry_web::WebTransportSessionHandler for GatewayWebTransportHandler {
    fn on_input_datagram(&self, session_id: &str, datagram: wavry_web::InputDatagram) {
        tracing::debug!(
            "webtransport input datagram from {}: {:?}",
            session_id,
            datagram
        );
    }

    fn on_control_frame(&self, session_id: &str, frame: wavry_web::ControlStreamFrame) {
        tracing::debug!(
            "webtransport control frame from {}: {:?}",
            session_id,
            frame
        );
    }
}

#[cfg(feature = "webtransport-runtime")]
pub async fn run_webtransport_runtime(bind_addr: &str) -> anyhow::Result<()> {
    let server = wavry_web::WebTransportServer::bind(bind_addr).await?;
    server.run(GatewayWebTransportHandler).await
}
