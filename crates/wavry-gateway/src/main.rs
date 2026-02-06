use axum::{
    routing::{get, post},
    Router,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod db;
mod relay;
mod signal;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Impl FromRef for Axum sub-state extraction
#[derive(Clone)]
struct AppState {
    pool: sqlx::SqlitePool,
    connections: signal::ConnectionMap,
    relay_sessions: relay::RelayMap,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "wavry_gateway=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. Load Env
    dotenv::dotenv().ok();
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:gateway.db".to_string());

    // 3. Connect DB
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    tracing::info!("Connected to database: {}", database_url);

    // 4. Run Migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    // 4.5 Init Shared State
    let connections = Arc::new(RwLock::new(HashMap::new()));
    let relay_sessions = Arc::new(RwLock::new(HashMap::new()));

    let app_state = AppState {
        pool,
        connections,
        relay_sessions: relay_sessions.clone(),
    };

    // 5. Spawn Relay Server
    let relay_port = 3478; // Standard STUN/TURN port
    tokio::spawn(async move {
        if let Err(e) = relay::run_relay_server(relay_port, relay_sessions).await {
            tracing::error!("Relay server crashed: {}", e);
        }
    });

    // 5.5 Setup Router
    let app = Router::new()
        .route("/", get(|| async { "Wavry Gateway Online 🚀" }))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/2fa/setup", post(auth::setup_totp))
        .route("/auth/2fa/enable", post(auth::enable_totp))
        .route("/ws", get(signal::ws_handler))
        .with_state(app_state);

    // 6. Start Server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Gateway listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
