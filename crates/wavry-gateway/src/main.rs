use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderName, Method, Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use sqlx::sqlite::SqlitePoolOptions;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod admin;
mod auth;
mod db;
mod relay;
mod security;
mod signal;
mod web;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct AppState {
    pool: sqlx::SqlitePool,
    connections: signal::ConnectionMap,
    relay_sessions: relay::RelayMap,
}

#[derive(Serialize)]
struct RuntimeMetrics {
    active_ws_connections: usize,
    active_relay_sessions: usize,
}

#[derive(Serialize)]
struct RateLimitError {
    error: String,
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let active_ws_connections = state.connections.read().await.len();
    let active_relay_sessions = state.relay_sessions.read().await.len();
    (
        axum::http::StatusCode::OK,
        Json(RuntimeMetrics {
            active_ws_connections,
            active_relay_sessions,
        }),
    )
        .into_response()
}

async fn prometheus_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let active_ws_connections = state.connections.read().await.len();
    let active_relay_sessions = state.relay_sessions.read().await.len();

    let prometheus_text = format!(
        r#"# HELP wavry_gateway_websocket_connections Active WebSocket connections
# TYPE wavry_gateway_websocket_connections gauge
wavry_gateway_websocket_connections {active_ws_connections}
# HELP wavry_gateway_relay_sessions Active relay sessions
# TYPE wavry_gateway_relay_sessions gauge
wavry_gateway_relay_sessions {active_relay_sessions}
"#,
        active_ws_connections = active_ws_connections,
        active_relay_sessions = active_relay_sessions,
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        prometheus_text,
    )
}

async fn global_api_rate_limit(req: Request<axum::body::Body>, next: middleware::Next) -> Response {
    let path = req.uri().path();
    if path == "/" || path == "/health" || path.starts_with("/metrics/") {
        return next.run(req).await;
    }

    let direct_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0)
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));
    let client_ip = security::effective_client_ip(req.headers(), direct_addr);
    let key = format!("{}:{}:{}", req.method(), path, client_ip);
    if !security::allow_global_api_request(&key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RateLimitError {
                error: "Too many requests".to_string(),
            }),
        )
            .into_response();
    }

    next.run(req).await
}

impl axum::extract::FromRef<AppState> for sqlx::SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState> for signal::ConnectionMap {
    fn from_ref(state: &AppState) -> Self {
        state.connections.clone()
    }
}

impl axum::extract::FromRef<AppState> for relay::RelayMap {
    fn from_ref(state: &AppState) -> Self {
        state.relay_sessions.clone()
    }
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

fn check_public_bind_allowed(addr: SocketAddr) -> anyhow::Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if env_bool("WAVRY_ALLOW_PUBLIC_BIND", false) {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "refusing non-loopback bind without WAVRY_ALLOW_PUBLIC_BIND=1"
    ))
}

fn ws_signaling_url_for_bound_addr(bound_addr: SocketAddr) -> String {
    let host = if bound_addr.ip().is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        bound_addr.ip().to_string()
    };
    format!("ws://{}:{}/ws", host, bound_addr.port())
}

fn build_cors_layer() -> CorsLayer {
    let allow_origin = if security::cors_allow_any() {
        AllowOrigin::any()
    } else {
        let origins = security::cors_origin_values();
        if origins.is_empty() {
            tracing::warn!(
                "no valid CORS origins configured; cross-origin browser access will be blocked"
            );
            return CorsLayer::new()
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    HeaderName::from_static("x-session-token"),
                ]);
        } else {
            AllowOrigin::list(origins)
        }
    };

    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-session-token"),
        ])
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "wavry_gateway=info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenv::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:gateway.db".to_string());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to database");
    tracing::info!("connected to gateway database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    let connections = Arc::new(RwLock::new(HashMap::new()));
    let relay_sessions = Arc::new(RwLock::new(HashMap::new()));

    let app_state = AppState {
        pool: pool.clone(),
        connections: connections.clone(),
        relay_sessions: relay_sessions.clone(),
    };

    let relay_port: u16 = std::env::var("WAVRY_GATEWAY_RELAY_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0); // Default to 0 for random port
    tokio::spawn(async move {
        if let Err(err) = relay::run_relay_server(relay_port, relay_sessions).await {
            tracing::error!("relay server crashed: {}", err);
        }
    });

    let session_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            match db::delete_expired_sessions(&session_pool).await {
                Ok(count) if count > 0 => tracing::info!("cleaned {} expired auth sessions", count),
                Ok(_) => {}
                Err(err) => tracing::warn!("failed to clean expired sessions: {}", err),
            }
        }
    });

    let relay_cleanup = app_state.relay_sessions.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        let relay_ttl = Duration::from_secs(
            std::env::var("WAVRY_RELAY_SESSION_TTL_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(300)
                .max(30),
        );
        let relay_max = std::env::var("WAVRY_RELAY_SESSION_LIMIT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(4096);

        loop {
            interval.tick().await;
            let now = Instant::now();
            let mut guard = relay_cleanup.write().await;
            guard.retain(|_, session| now.duration_since(session.created_at) < relay_ttl);

            if guard.len() > relay_max {
                let remove_count = guard.len() - relay_max;
                let mut oldest: Vec<(String, Instant)> = guard
                    .iter()
                    .map(|(token, session)| (token.clone(), session.created_at))
                    .collect();
                oldest.sort_by_key(|(_, created_at)| *created_at);
                for (token, _) in oldest.into_iter().take(remove_count) {
                    guard.remove(&token);
                }
            }
        }
    });

    #[cfg(feature = "webtransport-runtime")]
    {
        if env_bool("WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME", false) {
            let bind_addr =
                std::env::var("WEBTRANSPORT_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:0".to_string());
            let socket_addr: SocketAddr =
                bind_addr.parse().expect("invalid WEBTRANSPORT_BIND_ADDR");
            check_public_bind_allowed(socket_addr).expect("WebTransport bind rejected");
            let pool_clone = pool.clone();
            let connections_clone = connections.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    web::run_webtransport_runtime(&bind_addr, pool_clone, connections_clone).await
                {
                    tracing::error!("WebTransport runtime crashed: {}", err);
                }
            });
        } else {
            tracing::warn!(
                "webtransport runtime disabled; set WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME=1 to enable (NOT FOR PRODUCTION)"
            );
        }
    }

    let app = Router::new()
        .route("/", get(|| async { "Wavry Gateway Online" }))
        .route("/health", get(health))
        .route("/metrics/runtime", get(health))
        .route("/metrics/auth", get(auth::metrics))
        .route("/metrics/prometheus", get(prometheus_metrics))
        .route("/admin", get(admin::admin_panel))
        .route("/admin/api/overview", get(admin::admin_overview))
        .route("/admin/api/audit", get(admin::admin_audit))
        .route(
            "/admin/api/sessions/revoke",
            post(admin::admin_revoke_session),
        )
        .route("/admin/api/ban", post(admin::admin_ban_user))
        .route("/admin/api/unban", post(admin::admin_unban_user))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/2fa/setup", post(auth::setup_totp))
        .route("/auth/2fa/enable", post(auth::enable_totp))
        .route("/webrtc/config", get(web::webrtc_config))
        .route("/webrtc/offer", post(web::webrtc_offer))
        .route("/webrtc/answer", post(web::webrtc_answer))
        .route("/webrtc/candidate", post(web::webrtc_candidate))
        .route("/v1/relays/report", post(web::handle_relay_report))
        .route("/v1/relays/reputation", get(web::handle_relay_reputation))
        .route("/ws", get(signal::ws_handler))
        .layer(middleware::from_fn(global_api_rate_limit))
        .layer(build_cors_layer())
        .with_state(app_state);

    let bind_addr =
        std::env::var("WAVRY_GATEWAY_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let addr: SocketAddr = bind_addr.parse()?;
    check_public_bind_allowed(addr)?;

    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            let fallback_addr = SocketAddr::new(addr.ip(), 0);
            tracing::warn!(
                "gateway bind {} is already in use, falling back to {}",
                addr,
                fallback_addr
            );
            TcpListener::bind(fallback_addr).await?
        }
        Err(err) => return Err(err.into()),
    };
    let bound_addr = listener.local_addr()?;
    let ws_signaling_url = std::env::var("WS_SIGNALING_URL")
        .unwrap_or_else(|_| ws_signaling_url_for_bound_addr(bound_addr));
    std::env::set_var("WAVRY_GATEWAY_BOUND_ADDR", bound_addr.to_string());
    std::env::set_var("WS_SIGNALING_URL", ws_signaling_url.clone());
    tracing::info!("gateway listening on {}", bound_addr);
    tracing::info!("gateway signaling url {}", ws_signaling_url);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
