//! Wavry Master server stub.
//!
//! This will be the coordination service for identity, relay registry,
//! lease issuance, and matchmaking.

#![forbid(unsafe_code)]

use anyhow::{anyhow, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::{mpsc, RwLock}; // Required for socket.split() and receiver.next()

#[derive(Parser, Debug)]
#[command(name = "wavry-master")]
#[command(about = "Wavry Master coordination server")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,
    #[arg(long, default_value = "info")]
    log_level: String,
}

use wavry_common::protocol::{RegisterRequest, SignalMessage, VerifyRequest};

type PeerMap = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Message>>>>;

struct AppState {
    // wavry_id -> challenge
    challenges: Mutex<HashMap<String, [u8; 32]>>,
    peers: PeerMap,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    wavry_common::init_tracing_with_default(&args.log_level);

    let state = Arc::new(AppState {
        challenges: Mutex::new(HashMap::new()),
        peers: Arc::new(RwLock::new(HashMap::new())),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/auth/register", post(handle_register))
        .route("/v1/auth/register/verify", post(handle_verify))
        .route("/v1/auth/login", post(handle_register)) // Same flow for now
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    info!("wavry-master starting on {}", args.listen);
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
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

async fn handle_register(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<RegisterRequest>,
) -> Json<serde_json::Value> {
    use rand::RngCore;
    let mut challenge = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut challenge);

    let mut lock = state.challenges.lock().unwrap();
    lock.insert(payload.wavry_id.clone(), challenge);

    Json(serde_json::json!({
        "status": "pending_challenge",
        "challenge": hex::encode(challenge)
    }))
}

async fn handle_verify(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<VerifyRequest>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let challenge = {
        let mut lock = state.challenges.lock().unwrap();
        lock.remove(&payload.wavry_id)
            .ok_or(axum::http::StatusCode::BAD_REQUEST)?
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
            "token": format!("MOCK_BEARER_{}", uuid::Uuid::new_v4()),
            "username": "verified_user"
        })))
    } else {
        Err(axum::http::StatusCode::UNAUTHORIZED)
    }
}

// --- WebSocket ---
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Outgoing loop
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

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
                    // MOCK: In prod, verify token against DB/auth service
                    let username = format!("user_{}", &token[..4]);
                    my_username = Some(username.clone());
                    state.peers.write().await.insert(username, tx.clone());
                    info!("Peer bound: {}", my_username.as_ref().unwrap());
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
        let _ = tx.send(Message::Text(serde_json::to_string(&msg).unwrap()));
    } else {
        info!(
            "Target peer '{}' not found for relaying message: {:?}",
            target, msg
        );
    }
}
