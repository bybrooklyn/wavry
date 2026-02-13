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
use std::time::{Instant, SystemTime};
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
    #[serde(rename = "rid")]
    relay_id: String,
    #[serde(rename = "kid")]
    key_id: String,
    #[serde(rename = "iat_rfc3339")]
    issued_at: String,
    #[serde(rename = "nbf_rfc3339")]
    not_before: String,
    #[serde(rename = "exp_rfc3339")]
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
    relay_id: &str,
    signing_key_id: &str,
    lease_ttl: Duration,
    key: &pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4>,
) -> Result<String> {
    use pasetors::claims::Claims;
    let mut claims = Claims::new().map_err(|e| anyhow!("pasetors error: {}", e))?;
    let now = chrono::Utc::now();
    let ttl =
        chrono::Duration::from_std(lease_ttl).unwrap_or_else(|_| chrono::Duration::minutes(15));
    let exp = (now + ttl).to_rfc3339();

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
    claims
        .add_additional("exp_rfc3339", exp)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("rid", relay_id)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("kid", signing_key_id)
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional("iat_rfc3339", now.to_rfc3339())
        .map_err(|e| anyhow!("pasetors error: {}", e))?;
    claims
        .add_additional(
            "nbf_rfc3339",
            (now - chrono::Duration::seconds(5)).to_rfc3339(),
        )
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
    signing_key_id: String,
    lease_ttl: Duration,
    provisioned_signing_key: bool,
    started_at: Instant,
}

const LEASE_LIMIT_PER_MINUTE: usize = 10;
const DEFAULT_LEASE_TTL_SECS: u64 = 900;

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

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn public_key_from_signing_key(
    key: &pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4>,
) -> pasetors::keys::AsymmetricPublicKey<pasetors::version4::V4> {
    pasetors::keys::AsymmetricPublicKey::<pasetors::version4::V4>::from(&key.as_bytes()[32..])
        .expect("failed to convert pubkey")
}

fn derive_default_key_id(
    key: &pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4>,
) -> String {
    let pub_key = public_key_from_signing_key(key);
    let bytes = pub_key.as_bytes();
    let suffix = bytes.len().min(8);
    format!("k{}", hex::encode(&bytes[..suffix]))
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

    let (signing_key, provisioned_signing_key) =
        if let Ok(key_hex) = std::env::var("WAVRY_MASTER_SIGNING_KEY") {
            info!("using provisioned signing key from environment");
            let key_bytes = hex::decode(key_hex).expect("invalid WAVRY_MASTER_SIGNING_KEY hex");
            (
                pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&key_bytes)
                    .expect("failed to load signing key from env"),
                true,
            )
        } else if let Ok(path) = std::env::var("WAVRY_MASTER_KEY_FILE") {
            info!("loading signing key from {}", path);
            let key_hex = std::fs::read_to_string(path).expect("failed to read master key file");
            let key_bytes = hex::decode(key_hex.trim()).expect("invalid master key file hex");
            (
                pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&key_bytes)
                    .expect("failed to load signing key from file"),
                true,
            )
        } else {
            warn!("WAVRY_MASTER_KEY_FILE or WAVRY_MASTER_SIGNING_KEY not provided");
            warn!("generating temporary random signing key (INSECURE)");
            use ed25519_dalek::SigningKey;
            let mut seed = [0u8; 32];
            rand::thread_rng().fill(&mut seed);
            let sk = SigningKey::from_bytes(&seed);
            (
                pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(
                    &sk.to_keypair_bytes(),
                )
                .expect("failed to init signing key"),
                false,
            )
        };

    let signing_key_id = std::env::var("WAVRY_MASTER_KEY_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_default_key_id(&signing_key));
    let lease_ttl_secs = env_u64("WAVRY_MASTER_LEASE_TTL_SECS", DEFAULT_LEASE_TTL_SECS);
    let lease_ttl = Duration::from_secs(lease_ttl_secs.clamp(60, 3600));
    info!(
        "master signing key id={} lease_ttl_secs={} provisioned_key={}",
        signing_key_id,
        lease_ttl.as_secs(),
        provisioned_signing_key
    );

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
        signing_key_id,
        lease_ttl,
        provisioned_signing_key,
        started_at: Instant::now(),
    });

    let relay_registry = state.relays.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        let quarantine_after = std::time::Duration::from_secs(120);
        let purge_after = std::time::Duration::from_secs(600);
        loop {
            interval.tick().await;
            let now = Instant::now();
            let mut relays = relay_registry.write().await;
            for relay in relays.values_mut() {
                let age = now.duration_since(relay.last_seen);
                if age > quarantine_after
                    && !matches!(relay.state, RelayState::Draining | RelayState::Banned)
                {
                    relay.state = RelayState::Quarantined;
                }
            }
            relays.retain(|_, relay| now.duration_since(relay.last_seen) <= purge_after);
        }
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(ready_check))
        .route("/.well-known/wavry-id", get(handle_well_known_id))
        .route("/v1/relays/register", post(handle_relay_register))
        .route("/v1/relays/heartbeat", post(handle_relay_heartbeat))
        .route("/v1/relays", get(handle_relay_list))
        .route("/v1/feedback", post(handle_feedback))
        .route("/admin/api/sessions/revoke", post(handle_revoke_session))
        .route(
            "/admin/api/relays/update_state",
            post(handle_relay_update_state),
        )
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

#[derive(Serialize)]
struct MasterHealthResponse {
    status: &'static str,
    ready: bool,
    uptime_secs: u64,
    peers_connected: usize,
    relays_registered: usize,
    relays_assignable: usize,
    signing_key_id: String,
    provisioned_signing_key: bool,
    lease_ttl_secs: u64,
}

async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let peers_connected = state.peers.read().await.len();
    let relays = state.relays.read().await;
    let now = Instant::now();
    let relays_registered = relays.len();
    let relays_assignable = relays
        .values()
        .filter(|relay| relay_is_assignable(relay, now))
        .count();
    let ready = state.provisioned_signing_key && relays_assignable > 0;
    (
        StatusCode::OK,
        Json(MasterHealthResponse {
            status: "ok",
            ready,
            uptime_secs: state.started_at.elapsed().as_secs(),
            peers_connected,
            relays_registered,
            relays_assignable,
            signing_key_id: state.signing_key_id.clone(),
            provisioned_signing_key: state.provisioned_signing_key,
            lease_ttl_secs: state.lease_ttl.as_secs(),
        }),
    )
        .into_response()
}

async fn ready_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let relays = state.relays.read().await;
    let now = Instant::now();
    let assignable = relays
        .values()
        .filter(|relay| relay_is_assignable(relay, now))
        .count();
    let ready = state.provisioned_signing_key && assignable > 0;
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(serde_json::json!({
            "ready": ready,
            "relays_assignable": assignable,
            "provisioned_signing_key": state.provisioned_signing_key,
            "signing_key_id": state.signing_key_id.clone()
        })),
    )
        .into_response()
}

fn relay_is_assignable(relay: &RelayRegistration, now: Instant) -> bool {
    let fresh = now.duration_since(relay.last_seen) <= Duration::from_secs(120);
    let state_ok = matches!(
        relay.state,
        RelayState::Active | RelayState::Probation | RelayState::Degraded | RelayState::New
    );
    fresh && state_ok
}

async fn handle_well_known_id(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pub_key = public_key_from_signing_key(&state.signing_key);
    Json(serde_json::json!({
        "public_key": hex::encode(pub_key.as_bytes()),
        "key_id": state.signing_key_id.clone(),
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

    let pub_key = public_key_from_signing_key(&state.signing_key);

    Json(RelayRegisterResponse {
        heartbeat_interval_ms: 5_000,
        master_public_key: pub_key.as_bytes().to_vec(),
        master_key_id: Some(state.signing_key_id.clone()),
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
    if !matches!(entry.state, RelayState::Draining | RelayState::Banned) {
        entry.state = if payload.load_pct >= 95.0 {
            RelayState::Degraded
        } else if payload.load_pct >= 85.0 {
            RelayState::Probation
        } else {
            RelayState::Active
        };
    }
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
    headers: HeaderMap,
    Json(payload): Json<RevokeRequest>,
) -> impl IntoResponse {
    if !assert_admin(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
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
                                .filter_map(|(id, r)| {
                                    if matches!(
                                        r.state,
                                        RelayState::Draining
                                            | RelayState::Quarantined
                                            | RelayState::Banned
                                    ) {
                                        return None;
                                    }
                                    let rep = reps.get(id).cloned().unwrap_or_default();

                                    // Map legacy RelayReputation to new RelayMetrics
                                    let metrics = RelayMetrics {
                                        success_rate: rep.success_rate,
                                        ..Default::default()
                                    };

                                    let age = Instant::now().saturating_duration_since(r.last_seen);
                                    let seen_at = SystemTime::now()
                                        .checked_sub(age)
                                        .unwrap_or(SystemTime::UNIX_EPOCH);

                                    Some(RelayCandidate {
                                        _id: id.clone(),
                                        endpoints: r.endpoints.clone(),
                                        state: r.state.clone(),
                                        metrics,
                                        region: r.region.clone(),
                                        asn: r.asn,
                                        load_pct: r.load_pct,
                                        last_seen: seen_at,
                                    })
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
                            let Some(addr) = relay.endpoints.first().cloned() else {
                                warn!("selected relay {} has no endpoints", relay._id);
                                continue;
                            };
                            let relay_id = relay._id;
                            let session_id = Uuid::new_v4();
                            let host_lease = generate_lease(
                                src,
                                session_id,
                                "server",
                                &relay_id,
                                &state.signing_key_id,
                                state.lease_ttl,
                                &state.signing_key,
                            )
                            .unwrap();
                            let client_lease = generate_lease(
                                &target_username,
                                session_id,
                                "client",
                                &relay_id,
                                &state.signing_key_id,
                                state.lease_ttl,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn test_signing_key() -> pasetors::keys::AsymmetricSecretKey<pasetors::version4::V4> {
        let seed = [7u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        pasetors::keys::AsymmetricSecretKey::<pasetors::version4::V4>::from(&sk.to_keypair_bytes())
            .expect("test signing key")
    }

    #[test]
    fn relay_assignable_checks_state_and_freshness() {
        let now = Instant::now();
        let base = RelayRegistration {
            endpoints: vec!["127.0.0.1:4000".into()],
            load_pct: 10.0,
            last_seen: now,
            region: Some("us-east-1".into()),
            asn: Some(64512),
            max_bitrate_kbps: 20_000,
            state: RelayState::Active,
        };
        assert!(relay_is_assignable(&base, now));

        let mut draining = base.clone();
        draining.state = RelayState::Draining;
        assert!(!relay_is_assignable(&draining, now));

        let mut stale = base.clone();
        stale.last_seen = now - Duration::from_secs(180);
        assert!(!relay_is_assignable(&stale, now));
    }

    #[test]
    fn generate_lease_embeds_relay_and_key_id() {
        let key = test_signing_key();
        let key_id = "kid-test";
        let relay_id = "relay-test";
        let session_id = Uuid::new_v4();
        let token = generate_lease(
            "user-a",
            session_id,
            "client",
            relay_id,
            key_id,
            Duration::from_secs(300),
            &key,
        )
        .expect("generate lease");

        let pub_key = public_key_from_signing_key(&key);
        let validation_rules = pasetors::claims::ClaimsValidationRules::new();
        let untrusted_token = pasetors::token::UntrustedToken::<
            pasetors::token::Public,
            pasetors::version4::V4,
        >::try_from(token.as_str())
        .expect("parse token");
        let claims =
            pasetors::public::verify(&pub_key, &untrusted_token, &validation_rules, None, None)
                .expect("verify token");
        let payload_value: serde_json::Value = claims.payload().into();
        let payload: LeaseClaims = match payload_value {
            serde_json::Value::String(raw) => {
                serde_json::from_str(&raw).expect("decode claims json string")
            }
            other => serde_json::from_value(other).expect("decode claims object"),
        };

        assert_eq!(payload.relay_id, relay_id);
        assert_eq!(payload.key_id, key_id);
        assert_eq!(payload.session_id, session_id);
    }
}
