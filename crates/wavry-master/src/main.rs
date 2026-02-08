//! Wavry Master coordination server.
//!
//! Handles identity, relay registry, and lease issuance.

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
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{info, warn};
use uuid::Uuid;


use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};

mod selection;
use selection::{RelayCandidate, RelayMetrics, RelayState};

use wavry_common::protocol::{
    RegisterRequest, RelayFeedbackRequest, RelayHeartbeatRequest, RelayRegisterRequest,
    RelayRegisterResponse, SignalMessage, VerifyRequest,
};

/// Lease claims in PASETO token
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct LeaseClaims {
    #[serde(rename = "sub")]
    wavry_id: String,
    #[serde(rename = "sid")]
    session_id: Uuid,
    role: String, // "client" or "server"
    #[serde(rename = "exp")]
    expiration: String,
    #[serde(rename = "slimit")]
    soft_limit_kbps: Option<u32>,
    #[serde(rename = "hlimit")]
    hard_limit_kbps: Option<u32>,
}

fn generate_lease(
    wavry_id: &str,
    session_id: Uuid,
    role: &str,
    key: &pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4>,
) -> Result<String> {
    use pasetors::claims::Claims;
    let mut claims = Claims::new().map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .subject(wavry_id)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("sid", serde_json::json!(session_id))
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("role", role)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("wavry_id", wavry_id)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;

    let exp = (chrono::Utc::now() + chrono::Duration::minutes(15)).to_rfc3339();
    claims
        .add_additional("exp", exp)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;

    // Optional limits
    claims
        .add_additional("slimit", 50_000)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("hlimit", 100_000)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;

    let token = pasetors::public::sign(key, &claims, None, None)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    Ok(token)
}

#[derive(Parser, Debug)]
#[command(name = "wavry-master")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,
    #[arg(long, default_value = "info")]
    log_level: String,
    #[arg(long, default_value_t = false)]
    insecure_dev: bool,
}

type PeerMap = Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>;
type RelayMap = Arc<RwLock<HashMap<String, RelayRegistration>>>;

#[derive(Clone)]
struct RelayRegistration {
    endpoints: Vec<String>,
    load_pct: f32,
    last_seen: Instant,
    region: Option<String>,
    asn: Option<u32>,
    max_bitrate_kbps: u32,
    state: RelayState,
}

#[derive(Clone, Default)]
struct RelayReputation {
    success_rate: f32,
}

#[cfg(feature = "insecure-dev-auth")]
struct ChallengeEntry {
    challenge: [u8; 32],
    issued_at: Instant,
}

struct AppState {
    #[cfg(feature = "insecure-dev-auth")]
    challenges: Mutex<HashMap<String, ChallengeEntry>>,
    peers: PeerMap,
    relays: RelayMap,
    reputations: Arc<RwLock<HashMap<String, RelayReputation>>>,
    lease_rate_limiter: Mutex<HashMap<String, Vec<Instant>>>,
    banned_users: Arc<RwLock<HashSet<String>>>,
    #[cfg(feature = "insecure-dev-auth")]
    insecure_dev: bool,
    signing_key: pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4>,
}

const LEASE_LIMIT_PER_MINUTE: usize = 10;

fn check_lease_rate_limit(state: &AppState, username: &str) -> bool {
    let mut guard = state.lease_rate_limiter.lock().unwrap();
    let now = Instant::now();
    let entries = guard.entry(username.to_string()).or_default();

    // Remove expired entries (older than 1 min)
    entries.retain(|&t| now.duration_since(t) < Duration::from_secs(60));

    if entries.len() >= LEASE_LIMIT_PER_MINUTE {
        return false;
    }

    entries.push(now);
    true
}

#[derive(Serialize)]
struct RelayRegistryResponse {
    relay_id: String,
    endpoints: Vec<String>,
    load_pct: f32,
    last_seen_ms_ago: u64,
    max_bitrate_kbps: u32,
    state: RelayState,
}

#[derive(Deserialize)]
struct RelayUpdateStateRequest {
    relay_id: String,
    new_state: RelayState,
}

fn assert_admin(headers: &HeaderMap) -> bool {
    let expected = std::env::var("ADMIN_PANEL_TOKEN").unwrap_or_default();
    if expected.len() < 32 {
        return false;
    }

    let got = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string());

    if let Some(got) = got {
        return wavry_common::helpers::constant_time_eq(&got, &expected);
    }
    false
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
async fn main() -> anyhow::Result<()> {
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
    let insecure_dev = args.insecure_dev || env_bool("WAVRY_MASTER_INSECURE_DEV", false);

    let signing_key = if let Ok(key_hex) = std::env::var("WAVRY_MASTER_SIGNING_KEY") {
        info!("using provisioned signing key from environment");
        let key_bytes = hex::decode(key_hex).expect("invalid WAVRY_MASTER_SIGNING_KEY hex");
        pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&key_bytes)
            .expect("failed to load signing key from env")
    } else if let Ok(path) = std::env::var("WAVRY_MASTER_KEY_FILE") {
        info!("loading signing key from {}", path);
        let key_hex = std::fs::read_to_string(path).expect("failed to read master key file");
        let key_bytes = hex::decode(key_hex.trim()).expect("invalid master key file hex");
        pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&key_bytes)
            .expect("failed to load signing key from file")
    } else {
        warn!("WAVRY_MASTER_KEY_FILE or WAVRY_MASTER_SIGNING_KEY not provided");
        warn!("generating temporary random signing key (INSECURE)");
        use ed25519_dalek::SigningKey;
        let mut seed = [0u8; 32];
        rand::thread_rng().fill(&mut seed);
        let sk = SigningKey::from_bytes(&seed);
        pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&sk.to_keypair_bytes())
            .expect("failed to init signing key")
    };

    let state = Arc::new(AppState {
        #[cfg(feature = "insecure-dev-auth")]
        challenges: Mutex::new(HashMap::new()),
        peers: Arc::new(RwLock::new(HashMap::new())),
        relays: Arc::new(RwLock::new(HashMap::new())),
        reputations: Arc::new(RwLock::new(HashMap::new())),
        lease_rate_limiter: Mutex::new(HashMap::new()),
        banned_users: Arc::new(RwLock::new(HashSet::new())),
        #[cfg(feature = "insecure-dev-auth")]
        insecure_dev,
        signing_key,
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
        .route("/.well-known/wavry-id", get(handle_well_known_id))
        .route("/v1/relays/register", post(handle_relay_register))
        .route("/v1/relays/heartbeat", post(handle_relay_heartbeat))
        .route("/v1/relays", get(handle_relay_list))
        .route("/v1/feedback", post(handle_feedback))
        .route("/admin/api/sessions/revoke", post(handle_revoke_session))
        .route("/admin/api/relays/update_state", post(handle_relay_update_state))
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
    .await?;
    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}

async fn handle_well_known_id(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pub_key = pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(
        &state.signing_key.as_bytes()[32..],
    )
    .expect("failed to convert pubkey");
    Json(serde_json::json!({
        "public_key": hex::encode(pub_key.as_bytes()),
        "version": "1.0"
    }))
    .into_response()
}

async fn handle_relay_register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RelayRegisterRequest>,
) -> impl IntoResponse {
    if payload.relay_id.trim().is_empty() || payload.endpoints.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Minimum requirement: 10Mbps (10,000 kbps)
    let max_bitrate = payload.max_bitrate_kbps.unwrap_or(10_000);
    if max_bitrate < 10_000 {
        warn!(
            "relay {} rejected: max_bitrate {} kbps is below minimum 10000 kbps",
            payload.relay_id, max_bitrate
        );
        return (
            StatusCode::BAD_REQUEST,
            "Relay must support at least 10Mbps bandwidth",
        )
            .into_response();
    }

    // Sybil Check: Max 5 relays per IP
    if let Some(ip) = payload.endpoints.first().and_then(|e| e.split(':').next()) {
        let relays = state.relays.read().await;
        let count = relays
            .values()
            .filter(|r| r.endpoints.iter().any(|e| e.starts_with(ip)))
            .count();
        if count >= 5 {
            warn!("Sybil check failed for IP {}: {} relays", ip, count);
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    let mut relays = state.relays.write().await;
    relays.insert(
        payload.relay_id.clone(),
        RelayRegistration {
            endpoints: payload.endpoints,
            load_pct: 0.0,
            last_seen: Instant::now(),
            region: payload.region,
            asn: payload.asn,
            max_bitrate_kbps: max_bitrate,
            state: RelayState::New,
        },
    );
    info!("relay registered: {}", payload.relay_id);

    let pub_key = pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(
        &state.signing_key.as_bytes()[32..],
    )
    .expect("failed to convert pubkey");

    Json(RelayRegisterResponse {
        heartbeat_interval_ms: 5_000,
        master_public_key: pub_key.as_bytes().to_vec(),
    })
    .into_response()
}

async fn handle_relay_heartbeat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RelayHeartbeatRequest>,
) -> impl IntoResponse {
    if !(0.0..=100.0).contains(&payload.load_pct) || payload.relay_id.trim().is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut relays = state.relays.write().await;
    let Some(entry) = relays.get_mut(&payload.relay_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    entry.load_pct = payload.load_pct;
    entry.last_seen = Instant::now();
    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn handle_relay_list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let now = Instant::now();
    let relays = state.relays.read().await;
    let mut out = Vec::with_capacity(relays.len());
    for (relay_id, relay) in relays.iter() {
        out.push(RelayRegistryResponse {
            relay_id: relay_id.clone(),
            endpoints: relay.endpoints.clone(),
            load_pct: relay.load_pct,
            last_seen_ms_ago: now.saturating_duration_since(relay.last_seen).as_millis() as u64,
            max_bitrate_kbps: relay.max_bitrate_kbps,
            state: relay.state.clone(),
        });
    }
    Json(out).into_response()
}

async fn handle_relay_update_state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RelayUpdateStateRequest>,
) -> impl IntoResponse {
    if !assert_admin(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut relays = state.relays.write().await;
    if let Some(relay) = relays.get_mut(&payload.relay_id) {
        info!(
            "Admin updated relay {} state: {:?} -> {:?}",
            payload.relay_id, relay.state, payload.new_state
        );
        relay.state = payload.new_state;
        StatusCode::OK.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn handle_feedback(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RelayFeedbackRequest>,
) -> impl IntoResponse {
    let mut reputations = state.reputations.write().await;
    let entry = reputations.entry(payload.relay_id.clone()).or_default();

    // Simple moving average for success rate based on feedback quality
    let success = payload.quality_score > 50;
    let weight = 0.1;
    entry.success_rate =
        (1.0 - weight) * entry.success_rate + weight * (if success { 1.0 } else { 0.0 });

    info!(
        "feedback received for relay {}: score={}, success={}",
        payload.relay_id, payload.quality_score, success
    );

    Json(serde_json::json!({ "accepted": true })).into_response()
}

#[derive(Debug, Deserialize)]
struct RevokeRequest {
    wavry_id: String,
}

async fn handle_revoke_session(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RevokeRequest>,
) -> impl IntoResponse {
    let mut banned = state.banned_users.write().await;
    banned.insert(payload.wavry_id.clone());
    info!("Banned user {}", payload.wavry_id);
    Json(serde_json::json!({ "banned": true })).into_response()
}

async fn handle_register(
    State(_state): State<Arc<AppState>>,
    Json(_payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn handle_login(
    State(_state): State<Arc<AppState>>,
    Json(_payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn handle_verify(
    State(_state): State<Arc<AppState>>,
    Json(_payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ws_origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(128);

    let tx_clone = tx.clone();
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
                Err(_) => continue,
            };

            match signal {
                SignalMessage::BIND { token } => {
                    let prefix: String = token.chars().take(8).collect();
                    let username = format!("user_{}", prefix);
                    my_username = Some(username.clone());
                    state.peers.write().await.insert(username, tx_clone.clone());
                }
                SignalMessage::REQUEST_RELAY {
                    target_username,
                    region: client_region,
                } => {
                    if let Some(src) = &my_username {
                        if !check_lease_rate_limit(&state, src) {
                            let _ = tx_clone.try_send(Message::Text(
                                serde_json::to_string(&SignalMessage::ERROR {
                                    code: Some(429),
                                    message: "Lease rate limit exceeded. Please wait a moment."
                                        .into(),
                                })
                                .unwrap(),
                            ));
                            continue;
                        }

                        let selected_relay = {
                            let relays = state.relays.read().await;
                            let reps = state.reputations.read().await;

                            let candidates: Vec<RelayCandidate> = relays
                                .iter()
                                .map(|(id, r)| {
                                    let rep = reps.get(id).cloned().unwrap_or_default();

                                    // Map legacy RelayReputation to new RelayMetrics
                                    let metrics = RelayMetrics {
                                        success_rate: rep.success_rate,
                                        ..Default::default()
                                    };

                                    RelayCandidate {
                                        _id: id.clone(),
                                        endpoints: r.endpoints.clone(),
                                        state: r.state.clone(),
                                        metrics,
                                        region: r.region.clone(),
                                        asn: r.asn,
                                        load_pct: r.load_pct,
                                        last_seen: std::time::SystemTime::now(),
                                    }
                                })
                                .collect();

                            let filtered = selection::filter_by_geography(
                                candidates,
                                client_region.as_deref(),
                                None,
                                10,
                            );

                            selection::select_relay(&filtered).cloned()
                        };

                        if let Some(relay) = selected_relay {
                            let addr = relay.endpoints.first().cloned().unwrap();
                            let relay_id = relay._id;
                            let session_id = Uuid::new_v4();
                            let host_lease =
                                generate_lease(src, session_id, "server", &state.signing_key)
                                    .unwrap();
                            let client_lease = generate_lease(
                                &target_username,
                                session_id,
                                "client",
                                &state.signing_key,
                            )
                            .unwrap();

                            let _ = tx_clone.try_send(Message::Text(
                                serde_json::to_string(&SignalMessage::RELAY_CREDENTIALS {
                                    relay_id: relay_id.clone(),
                                    token: host_lease,
                                    addr: addr.clone(),
                                    session_id,
                                })
                                .unwrap(),
                            ));

                            relay_signal(
                                &state,
                                &target_username,
                                SignalMessage::RELAY_CREDENTIALS {
                                    relay_id,
                                    token: client_lease,
                                    addr,
                                    session_id,
                                },
                            )
                            .await;
                        }
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
                _ => {}
            }
        }
    }

    if let Some(u) = my_username {
        state.peers.write().await.remove(&u);
    }
}

async fn relay_signal(state: &Arc<AppState>, target: &str, msg: SignalMessage) {
    let guard = state.peers.read().await;
    if let Some(tx) = guard.get(target) {
        if let Ok(text) = serde_json::to_string(&msg) {
            let _ = tx.try_send(Message::Text(text));
        }
    }
}
