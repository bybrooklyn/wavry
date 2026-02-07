//! Wavry Master server stub.
//!
//! This will be the coordination service for identity, relay registry,
//! lease issuance, and matchmaking.

#![forbid(unsafe_code)]

use anyhow::{anyhow, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{info, warn};

#[cfg(feature = "insecure-dev-auth")]
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
#[cfg(feature = "insecure-dev-auth")]
use std::sync::Mutex;
#[cfg(feature = "insecure-dev-auth")]
use std::time::Duration;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock}; // Required for socket.split() and receiver.next()

#[derive(Parser, Debug)]
#[command(name = "wavry-master")]
#[command(about = "Wavry Master coordination server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,
    #[arg(long, default_value = "info")]
    log_level: String,
    #[arg(long, default_value_t = false)]
    insecure_dev: bool,
}

use wavry_common::protocol::{
    RegisterRequest, RelayHeartbeatRequest, RelayRegisterRequest, RelayRegisterResponse,
    SignalMessage, VerifyRequest,
};

type PeerMap = Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>;
type RelayMap = Arc<RwLock<HashMap<String, RelayRegistration>>>;

#[derive(Clone)]
struct RelayRegistration {
    endpoints: Vec<String>,
    load_pct: f32,
    last_seen: Instant,
}

#[cfg(feature = "insecure-dev-auth")]
struct ChallengeEntry {
    challenge: [u8; 32],
    issued_at: Instant,
}

struct AppState {
    // wavry_id -> challenge
    #[cfg(feature = "insecure-dev-auth")]
    challenges: Mutex<HashMap<String, ChallengeEntry>>,
    peers: PeerMap,
    relays: RelayMap,
    #[cfg(feature = "insecure-dev-auth")]
    insecure_dev: bool,
}

#[derive(Serialize)]
struct RelayRegistryResponse {
    relay_id: String,
    endpoints: Vec<String>,
    load_pct: f32,
    last_seen_ms_ago: u64,
}

#[cfg(feature = "insecure-dev-auth")]
const CHALLENGE_TTL: Duration = Duration::from_secs(300);
#[cfg(feature = "insecure-dev-auth")]
const CHALLENGE_CAPACITY: usize = 10_000;

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn allowed_origins() -> Vec<HeaderValue> {
    let raw = std::env::var("WAVRY_MASTER_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:1420,http://127.0.0.1:1420,tauri://localhost".into());
    raw.split(',')
        .filter_map(|origin| HeaderValue::from_str(origin.trim()).ok())
        .collect()
}

fn build_cors() -> CorsLayer {
    if env_bool("WAVRY_MASTER_CORS_ALLOW_ANY", false) {
        return CorsLayer::permissive();
    }

    let origins = allowed_origins();
    if origins.is_empty() {
        return CorsLayer::new();
    }

    CorsLayer::new().allow_origin(AllowOrigin::list(origins))
}

fn ws_origin_allowed(headers: &HeaderMap) -> bool {
    let require = env_bool("WAVRY_MASTER_WS_REQUIRE_ORIGIN", true);
    let allow_missing = env_bool("WAVRY_MASTER_WS_ALLOW_MISSING_ORIGIN", false);
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let Some(origin) = origin else {
        return !require || allow_missing;
    };

    let normalized = origin.trim().trim_end_matches('/').to_ascii_lowercase();
    allowed_origins().into_iter().any(|value| {
        value
            .to_str()
            .map(|s| {
                s.trim()
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(&normalized)
            })
            .unwrap_or(false)
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    wavry_common::init_tracing_with_default(&args.log_level);

    let listen_addr: std::net::SocketAddr = args
        .listen
        .parse()
        .map_err(|e| anyhow!("invalid --listen address: {e}"))?;
    if !listen_addr.ip().is_loopback() && !env_bool("WAVRY_MASTER_ALLOW_PUBLIC_BIND", false) {
        return Err(anyhow!(
            "refusing non-loopback master bind without WAVRY_MASTER_ALLOW_PUBLIC_BIND=1"
        ));
    }

    #[cfg(feature = "insecure-dev-auth")]
    let insecure_dev = {
        let requested = args.insecure_dev || env_bool("WAVRY_MASTER_INSECURE_DEV", false);
        if requested {
            info!("Insecure dev mode ENABLED via feature-gate and flag");
            true
        } else {
            false
        }
    };

    #[cfg(not(feature = "insecure-dev-auth"))]
    if args.insecure_dev || env_bool("WAVRY_MASTER_INSECURE_DEV", false) {
        warn!("Insecure dev mode requested but 'insecure-dev-auth' feature is NOT enabled. Staying in secure mode.");
    }

    let state = Arc::new(AppState {
        #[cfg(feature = "insecure-dev-auth")]
        challenges: Mutex::new(HashMap::new()),
        peers: Arc::new(RwLock::new(HashMap::new())),
        relays: Arc::new(RwLock::new(HashMap::new())),
        #[cfg(feature = "insecure-dev-auth")]
        insecure_dev,
    });

    let relay_registry = state.relays.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        let ttl = std::time::Duration::from_secs(120);
        loop {
            interval.tick().await;
            let now = Instant::now();
            let mut relays = relay_registry.write().await;
            relays.retain(|_, relay| now.duration_since(relay.last_seen) <= ttl);
        }
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/relays/register", post(handle_relay_register))
        .route("/v1/relays/heartbeat", post(handle_relay_heartbeat))
        .route("/v1/relays", get(handle_relay_list))
        .route("/v1/auth/register", post(handle_register))
        .route("/v1/auth/register/verify", post(handle_verify))
        .route("/v1/auth/login", post(handle_login))
        .route("/ws", get(ws_handler))
        .layer(build_cors())
        .with_state(state);

    info!("wavry-master starting on {}", listen_addr);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .map_err(|e| anyhow!(e))?;
    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}

async fn handle_relay_register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RelayRegisterRequest>,
) -> Result<Json<RelayRegisterResponse>, StatusCode> {
    if payload.relay_id.trim().is_empty() || payload.endpoints.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut relays = state.relays.write().await;
    relays.insert(
        payload.relay_id.clone(),
        RelayRegistration {
            endpoints: payload.endpoints,
            load_pct: 0.0,
            last_seen: Instant::now(),
        },
    );
    info!("relay registered: {}", payload.relay_id);

    Ok(Json(RelayRegisterResponse {
        heartbeat_interval_ms: 5_000,
    }))
}

async fn handle_relay_heartbeat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RelayHeartbeatRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !(0.0..=100.0).contains(&payload.load_pct) || payload.relay_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut relays = state.relays.write().await;
    let Some(entry) = relays.get_mut(&payload.relay_id) else {
        return Err(StatusCode::NOT_FOUND);
    };
    entry.load_pct = payload.load_pct;
    entry.last_seen = Instant::now();
    info!(
        "relay heartbeat: {} load={:.1}%",
        payload.relay_id, payload.load_pct
    );
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn handle_relay_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RelayRegistryResponse>>, StatusCode> {
    let now = Instant::now();
    let relays = state.relays.read().await;
    let mut out = Vec::with_capacity(relays.len());
    for (relay_id, relay) in relays.iter() {
        out.push(RelayRegistryResponse {
            relay_id: relay_id.clone(),
            endpoints: relay.endpoints.clone(),
            load_pct: relay.load_pct,
            last_seen_ms_ago: now
                .saturating_duration_since(relay.last_seen)
                .as_millis()
                .min(u64::MAX as u128) as u64,
        });
    }
    Ok(Json(out))
}

#[cfg(feature = "insecure-dev-auth")]
async fn handle_register(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !state.insecure_dev {
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    use rand::RngCore;
    let mut challenge = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut challenge);

    {
        let mut lock = state
            .challenges
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let now = Instant::now();
        lock.retain(|_, entry| now.duration_since(entry.issued_at) <= CHALLENGE_TTL);
        if lock.len() >= CHALLENGE_CAPACITY {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        lock.insert(
            payload.wavry_id.clone(),
            ChallengeEntry {
                challenge,
                issued_at: now,
            },
        );
    }

    Ok(Json(serde_json::json!({
        "status": "pending_challenge",
        "challenge": hex::encode(challenge)
    })))
}

#[cfg(not(feature = "insecure-dev-auth"))]
async fn handle_register(
    axum::extract::State(_): axum::extract::State<Arc<AppState>>,
    Json(_): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Err(StatusCode::NOT_IMPLEMENTED)
}

#[cfg(feature = "insecure-dev-auth")]
async fn handle_login(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    handle_register(axum::extract::State(state), Json(payload)).await
}

#[cfg(not(feature = "insecure-dev-auth"))]
async fn handle_login(
    axum::extract::State(_): axum::extract::State<Arc<AppState>>,
    Json(_): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Err(StatusCode::NOT_IMPLEMENTED)
}

#[cfg(feature = "insecure-dev-auth")]
async fn handle_verify(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    if !state.insecure_dev {
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    let challenge = {
        let mut lock = state
            .challenges
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let entry = lock
            .remove(&payload.wavry_id)
            .ok_or(axum::http::StatusCode::BAD_REQUEST)?;
        if Instant::now().duration_since(entry.issued_at) > CHALLENGE_TTL {
            return Err(StatusCode::BAD_REQUEST);
        }
        entry.challenge
    };

    // Verify Signature
    let public_key_bytes =
        hex::decode(&payload.wavry_id).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let sig_bytes =
        hex::decode(&payload.signature_hex).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    let verifying_key = VerifyingKey::from_bytes(
        public_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?,
    )
    .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let signature =
        Signature::from_slice(&sig_bytes).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    if verifying_key.verify(&challenge, &signature).is_ok() {
        Ok(Json(serde_json::json!({
            "status": "success",
            "token": format!("MOCK_BEARER_{}", uuid::Uuid::new_v4().as_simple()),
            "username": "verified_user"
        })))
    } else {
        Err(axum::http::StatusCode::UNAUTHORIZED)
    }
}

#[cfg(not(feature = "insecure-dev-auth"))]
async fn handle_verify(
    axum::extract::State(_): axum::extract::State<Arc<AppState>>,
    Json(_): Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Err(StatusCode::NOT_IMPLEMENTED)
}

// --- WebSocket ---
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ws_origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, state))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(128);

    // Outgoing loop
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    #[allow(unused_mut)]
    let mut my_username: Option<String> = None;

    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            let signal: SignalMessage = match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(_) => {
                    info!("Failed to parse signal message: {}", text);
                    continue;
                }
            };

            match signal {
                SignalMessage::OFFER_RIFT { .. } | SignalMessage::ANSWER_RIFT { .. } => {
                    // Master doesn't handle RIFT signaling
                }
                SignalMessage::BIND { token } => {
                    #[cfg(not(feature = "insecure-dev-auth"))]
                    {
                        let _ = token;
                        let _ = tx_clone.try_send(Message::Text(
                            serde_json::to_string(&SignalMessage::ERROR {
                                code: None,
                                message: "Master WS bind disabled (feature-gated)".into(),
                            })
                            .unwrap_or_else(|_| {
                                "{\"type\":\"ERROR\",\"payload\":{\"message\":\"disabled\"}}".into()
                            }),
                        ));
                        break;
                    }

                    #[cfg(feature = "insecure-dev-auth")]
                    {
                        if !state.insecure_dev {
                            let _ = tx_clone.try_send(Message::Text(
                                serde_json::to_string(&SignalMessage::ERROR {
                                    code: None,
                                    message: "Master WS bind disabled outside insecure dev mode"
                                        .into(),
                                })
                                .unwrap_or_else(|_| {
                                    "{\"type\":\"ERROR\",\"payload\":{\"message\":\"disabled\"}}"
                                        .into()
                                }),
                            ));
                            break;
                        }

                        if token.len() < 8 {
                            let _ = tx_clone.try_send(Message::Text(
                                serde_json::to_string(&SignalMessage::ERROR {
                                    code: None,
                                    message: "Invalid token".into(),
                                })
                                .unwrap_or_else(|_| {
                                    "{\"type\":\"ERROR\",\"payload\":{\"message\":\"invalid token\"}}"
                                        .into()
                                }),
                            ));
                            continue;
                        }

                        // MOCK: In prod, verify token against DB/auth service
                        let prefix: String = token.chars().take(8).collect();
                        let username = format!("user_{}", prefix);
                        my_username = Some(username.clone());
                        state.peers.write().await.insert(username, tx_clone.clone());
                        info!("Peer bound: {}", my_username.as_ref().unwrap());
                    }
                }
                SignalMessage::OFFER {
                    target_username,
                    sdp,
                    public_addr,
                } => {
                    if let Some(src) = &my_username {
                        relay_signal(
                            &state,
                            &target_username,
                            SignalMessage::OFFER {
                                target_username: src.clone(),
                                sdp,
                                public_addr,
                            },
                        )
                        .await;
                    }
                }
                SignalMessage::ANSWER {
                    target_username,
                    sdp,
                    public_addr,
                } => {
                    if let Some(src) = &my_username {
                        relay_signal(
                            &state,
                            &target_username,
                            SignalMessage::ANSWER {
                                target_username: src.clone(),
                                sdp,
                                public_addr,
                            },
                        )
                        .await;
                    }
                }
                SignalMessage::CANDIDATE {
                    target_username,
                    candidate,
                } => {
                    if let Some(src) = &my_username {
                        relay_signal(
                            &state,
                            &target_username,
                            SignalMessage::CANDIDATE {
                                target_username: src.clone(),
                                candidate,
                            },
                        )
                        .await;
                    }
                }
                SignalMessage::REQUEST_RELAY { target_username } => {
                    if let Some(src) = &my_username {
                        relay_signal(
                            &state,
                            &target_username,
                            SignalMessage::REQUEST_RELAY {
                                target_username: src.clone(),
                            },
                        )
                        .await;
                    }
                }
                SignalMessage::RELAY_CREDENTIALS { .. } => {
                    // Ensure we don't panic on received credentials without target
                    info!("Received RELAY_CREDENTIALS from client. Ignoring (no target).");
                }
                SignalMessage::ERROR { message, .. } => {
                    info!("Received ERROR message from client: {}", message);
                }
            }
        }
    }

    if let Some(u) = my_username {
        state.peers.write().await.remove(&u);
        info!("Peer unbound: {}", u);
    }
}

async fn relay_signal(state: &Arc<AppState>, target: &str, msg: SignalMessage) {
    let guard = state.peers.read().await;
    if let Some(tx) = guard.get(target) {
        if let Ok(text) = serde_json::to_string(&msg) {
            let _ = tx.try_send(Message::Text(text));
        }
    } else {
        info!(
            "Target peer '{}' not found for relaying message: {:?}",
            target, msg
        );
    }
}
