use crate::db::{self, Session, User};
use crate::security;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Json, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::SocketAddr;

use rand::{thread_rng, Rng};
use totp_rs::{Algorithm, TOTP};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub username: String,
    pub public_key: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub totp_code: Option<String>,
}

#[derive(Deserialize)]
pub struct EnableTotpRequest {
    pub email: String,
    pub password: String,
    pub secret: String,
    pub code: String,
}

#[derive(Serialize)]
pub struct TotpSetupResponse {
    pub secret: String,
    pub qr_png_base64: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub user: User,
    pub session: Session,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct LogoutResponse {
    pub revoked: bool,
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

fn rate_limit_key(scope: &str, addr: SocketAddr) -> String {
    format!("{scope}:{}", addr.ip())
}

fn ensure_auth_rate_limit(scope: &str, addr: SocketAddr) -> bool {
    security::allow_auth_request(&rate_limit_key(scope, addr))
}

fn is_reasonable_password_input(password: &str) -> bool {
    !password.is_empty() && password.len() <= 128
}

fn decode_totp_secret(secret: &str) -> Result<Vec<u8>, &'static str> {
    base32::decode(base32::Alphabet::Rfc4648 { padding: false }, secret)
        .ok_or("Invalid TOTP secret encoding")
}

fn totp_from_secret(secret: &str) -> Result<TOTP, &'static str> {
    let decoded = decode_totp_secret(secret)?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        decoded,
        None,
        "wavry".to_string(),
    )
    .map_err(|_| "Failed to initialize TOTP")
}

fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-session-token") {
        if let Ok(token) = value.to_str() {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

pub async fn register(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    if !ensure_auth_rate_limit("register", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    if !security::is_valid_email(&payload.email)
        || !security::is_valid_password(&payload.password)
        || !security::is_valid_display_name(&payload.display_name)
        || !security::is_valid_username(&payload.username)
        || !security::is_valid_public_key_hex(&payload.public_key)
    {
        return error_response(StatusCode::BAD_REQUEST, "Invalid registration payload");
    }

    if let Ok(Some(_)) = db::get_user_by_email(&pool, &payload.email).await {
        return error_response(StatusCode::CONFLICT, "Email already exists");
    }

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = match argon2.hash_password(payload.password.as_bytes(), &salt) {
        Ok(hash) => hash.to_string(),
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Hashing failed"),
    };

    let user = match db::create_user(
        &pool,
        &payload.email,
        &password_hash,
        &payload.display_name,
        &payload.username,
        &payload.public_key,
    )
    .await
    {
        Ok(user) => user,
        Err(err) => {
            tracing::error!("failed to create user: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error");
        }
    };

    let session = match db::create_session(&pool, &user.id, Some(addr.ip().to_string())).await {
        Ok(session) => session,
        Err(err) => {
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    (StatusCode::CREATED, Json(AuthResponse { user, session })).into_response()
}

pub async fn login(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    if !ensure_auth_rate_limit("login", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    if !security::is_valid_email(&payload.email) || !is_reasonable_password_input(&payload.password)
    {
        return error_response(StatusCode::BAD_REQUEST, "Invalid login payload");
    }

    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(user)) => user,
        Ok(None) => return error_response(StatusCode::UNAUTHORIZED, "Invalid credentials"),
        Err(err) => {
            tracing::error!("database error during login: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error");
        }
    };

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(hash) => hash,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return error_response(StatusCode::UNAUTHORIZED, "Invalid credentials");
    }

    if let Some(stored_secret) = &user.totp_secret {
        let Some(code) = payload.totp_code.as_deref() else {
            return error_response(StatusCode::UNAUTHORIZED, "2FA required");
        };

        if !security::is_valid_totp_code(code) {
            return error_response(StatusCode::UNAUTHORIZED, "Invalid 2FA code");
        }

        let secret = match security::decrypt_totp_secret(stored_secret) {
            Ok(secret) => secret,
            Err(err) => {
                tracing::error!("unable to decrypt stored TOTP secret: {}", err);
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                );
            }
        };

        let totp = match totp_from_secret(&secret) {
            Ok(totp) => totp,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                )
            }
        };

        if !totp.check_current(code).unwrap_or(false) {
            return error_response(StatusCode::UNAUTHORIZED, "Invalid 2FA code");
        }
    }

    let session = match db::create_session(&pool, &user.id, Some(addr.ip().to_string())).await {
        Ok(session) => session,
        Err(err) => {
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    (StatusCode::OK, Json(AuthResponse { user, session })).into_response()
}

pub async fn setup_totp(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    if !ensure_auth_rate_limit("totp_setup", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    if !security::is_valid_email(&payload.email) || !is_reasonable_password_input(&payload.password)
    {
        return error_response(StatusCode::BAD_REQUEST, "Invalid 2FA setup payload");
    }

    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(user)) => user,
        _ => return error_response(StatusCode::UNAUTHORIZED, "Auth failed"),
    };

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
    }

    let mut secret_bytes = [0u8; 20];
    thread_rng().fill(&mut secret_bytes);
    let secret_vec = secret_bytes.to_vec();

    let totp = match TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_vec,
        Some("Wavry".to_string()),
        payload.email.clone(),
    ) {
        Ok(totp) => totp,
        Err(err) => {
            tracing::error!("failed to create TOTP secret: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "2FA setup failed");
        }
    };

    let secret_encoded = totp.get_secret_base32();
    let qr_png_base64 = match totp.get_qr_base64() {
        Ok(qr) => qr,
        Err(err) => {
            tracing::error!("failed to create TOTP QR: {}", err);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "2FA QR generation failed",
            );
        }
    };

    (
        StatusCode::OK,
        Json(TotpSetupResponse {
            secret: secret_encoded,
            qr_png_base64,
        }),
    )
        .into_response()
}

pub async fn enable_totp(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<EnableTotpRequest>,
) -> impl IntoResponse {
    if !ensure_auth_rate_limit("totp_enable", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    if !security::is_valid_email(&payload.email)
        || !is_reasonable_password_input(&payload.password)
        || !security::is_valid_totp_code(&payload.code)
    {
        return error_response(StatusCode::BAD_REQUEST, "Invalid 2FA enable payload");
    }

    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(user)) => user,
        _ => return error_response(StatusCode::UNAUTHORIZED, "Auth failed"),
    };

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
    }

    let totp = match totp_from_secret(&payload.secret) {
        Ok(totp) => totp,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid TOTP secret"),
    };

    if !totp.check_current(&payload.code).unwrap_or(false) {
        return error_response(StatusCode::BAD_REQUEST, "Invalid code");
    }

    let encrypted_secret = match security::encrypt_totp_secret(&payload.secret) {
        Ok(secret) => secret,
        Err(err) => {
            tracing::error!("failed to encrypt TOTP secret: {}", err);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "2FA secret encryption failed",
            );
        }
    };

    if let Err(err) = db::enable_totp(&pool, &user.id, &encrypted_secret).await {
        tracing::error!("failed to enable TOTP in DB: {}", err);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "DB error");
    }

    let refreshed_user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "User missing after 2FA update",
            )
        }
        Err(err) => {
            tracing::error!("failed to fetch refreshed user: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "DB error");
        }
    };

    let session = match db::create_session(&pool, &user.id, Some(addr.ip().to_string())).await {
        Ok(session) => session,
        Err(err) => {
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    (
        StatusCode::OK,
        Json(AuthResponse {
            user: refreshed_user,
            session,
        }),
    )
        .into_response()
}

pub async fn logout(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_auth_rate_limit("logout", addr) {
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let Some(token) = extract_session_token(&headers) else {
        return error_response(StatusCode::BAD_REQUEST, "Missing bearer token");
    };
    if !security::is_valid_session_token(&token) {
        return error_response(StatusCode::BAD_REQUEST, "Invalid session token");
    }

    match db::revoke_session(&pool, &token).await {
        Ok(revoked) => (StatusCode::OK, Json(LogoutResponse { revoked })).into_response(),
        Err(err) => {
            tracing::error!("failed to revoke session: {}", err);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "Logout failed")
        }
    }
}
