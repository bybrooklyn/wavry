use crate::audit::{log_security_event, FailureReason, SecurityEventType};
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
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
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
    /// Existing TOTP code, required when the user already has TOTP enabled.
    /// Without this, an attacker with only the password could replace 2FA.
    pub existing_totp_code: Option<String>,
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
    // Compatibility fields for clients that still parse a flat auth payload.
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub username: String,
    pub totp_required: bool,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct LogoutResponse {
    pub revoked: bool,
}

struct AuthMetrics {
    register_attempts: AtomicU64,
    register_success: AtomicU64,
    login_attempts: AtomicU64,
    login_success: AtomicU64,
    totp_setup_attempts: AtomicU64,
    totp_setup_success: AtomicU64,
    totp_enable_attempts: AtomicU64,
    totp_enable_success: AtomicU64,
    logout_attempts: AtomicU64,
    logout_success: AtomicU64,
    rate_limited: AtomicU64,
    validation_errors: AtomicU64,
    auth_failures: AtomicU64,
    db_errors: AtomicU64,
}

impl Default for AuthMetrics {
    fn default() -> Self {
        Self {
            register_attempts: AtomicU64::new(0),
            register_success: AtomicU64::new(0),
            login_attempts: AtomicU64::new(0),
            login_success: AtomicU64::new(0),
            totp_setup_attempts: AtomicU64::new(0),
            totp_setup_success: AtomicU64::new(0),
            totp_enable_attempts: AtomicU64::new(0),
            totp_enable_success: AtomicU64::new(0),
            logout_attempts: AtomicU64::new(0),
            logout_success: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            validation_errors: AtomicU64::new(0),
            auth_failures: AtomicU64::new(0),
            db_errors: AtomicU64::new(0),
        }
    }
}

#[derive(Serialize)]
pub struct AuthMetricsSnapshot {
    pub register_attempts: u64,
    pub register_success: u64,
    pub login_attempts: u64,
    pub login_success: u64,
    pub totp_setup_attempts: u64,
    pub totp_setup_success: u64,
    pub totp_enable_attempts: u64,
    pub totp_enable_success: u64,
    pub logout_attempts: u64,
    pub logout_success: u64,
    pub rate_limited: u64,
    pub validation_errors: u64,
    pub auth_failures: u64,
    pub db_errors: u64,
}

static AUTH_METRICS: Lazy<AuthMetrics> = Lazy::new(AuthMetrics::default);

fn metrics_snapshot() -> AuthMetricsSnapshot {
    AuthMetricsSnapshot {
        register_attempts: AUTH_METRICS.register_attempts.load(Ordering::Relaxed),
        register_success: AUTH_METRICS.register_success.load(Ordering::Relaxed),
        login_attempts: AUTH_METRICS.login_attempts.load(Ordering::Relaxed),
        login_success: AUTH_METRICS.login_success.load(Ordering::Relaxed),
        totp_setup_attempts: AUTH_METRICS.totp_setup_attempts.load(Ordering::Relaxed),
        totp_setup_success: AUTH_METRICS.totp_setup_success.load(Ordering::Relaxed),
        totp_enable_attempts: AUTH_METRICS.totp_enable_attempts.load(Ordering::Relaxed),
        totp_enable_success: AUTH_METRICS.totp_enable_success.load(Ordering::Relaxed),
        logout_attempts: AUTH_METRICS.logout_attempts.load(Ordering::Relaxed),
        logout_success: AUTH_METRICS.logout_success.load(Ordering::Relaxed),
        rate_limited: AUTH_METRICS.rate_limited.load(Ordering::Relaxed),
        validation_errors: AUTH_METRICS.validation_errors.load(Ordering::Relaxed),
        auth_failures: AUTH_METRICS.auth_failures.load(Ordering::Relaxed),
        db_errors: AUTH_METRICS.db_errors.load(Ordering::Relaxed),
    }
}

pub async fn metrics() -> impl IntoResponse {
    (StatusCode::OK, Json(metrics_snapshot())).into_response()
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

fn auth_response(user: User, session: Session) -> AuthResponse {
    AuthResponse {
        token: session.token.clone(),
        user_id: user.id.clone(),
        email: user.email.clone(),
        username: user.username.clone(),
        totp_required: false,
        user,
        session,
    }
}

fn get_client_ip(headers: &HeaderMap, direct_addr: SocketAddr) -> IpAddr {
    security::effective_client_ip(headers, direct_addr)
}

fn rate_limit_key(scope: &str, ip: IpAddr) -> String {
    format!("{scope}:{}", ip)
}

fn ensure_auth_rate_limit(scope: &str, ip: IpAddr) -> bool {
    security::allow_auth_request(&rate_limit_key(scope, ip))
}

fn is_reasonable_password_input(password: &str) -> bool {
    !password.is_empty() && password.len() <= 128
}

fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

fn normalize_display_name(display_name: &str) -> String {
    display_name.trim().to_string()
}

fn normalize_public_key(public_key: &str) -> String {
    public_key.trim().to_string()
}

fn normalize_totp_secret(secret: &str) -> String {
    secret.chars().filter(|c| !c.is_whitespace()).collect()
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
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    AUTH_METRICS
        .register_attempts
        .fetch_add(1, Ordering::Relaxed);
    let client_ip = get_client_ip(&headers, addr);
    if !ensure_auth_rate_limit("register", client_ip) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let email = normalize_email(&payload.email);
    let username = normalize_username(&payload.username);
    let display_name = normalize_display_name(&payload.display_name);
    let public_key = normalize_public_key(&payload.public_key);

    if !security::is_valid_email(&email)
        || !security::is_valid_password(&payload.password)
        || !security::is_valid_display_name(&display_name)
        || !security::is_valid_username(&username)
        || !security::is_valid_public_key_hex(&public_key)
    {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid registration payload");
    }

    if let Ok(Some(_)) = db::get_user_by_email(&pool, &email).await {
        AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
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
        &email,
        &password_hash,
        &display_name,
        &username,
        &public_key,
    )
    .await
    {
        Ok(user) => user,
        Err(err) => {
            let lower = err.to_string().to_ascii_lowercase();
            if lower.contains("unique")
                && (lower.contains("users.email") || lower.contains("users.username"))
            {
                AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
                return error_response(StatusCode::CONFLICT, "Account already exists");
            }
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create user: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error");
        }
    };

    let session = match db::create_session(&pool, &user.id, Some(client_ip.to_string())).await {
        Ok(session) => session,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    AUTH_METRICS
        .register_success
        .fetch_add(1, Ordering::Relaxed);
    log_security_event(
        SecurityEventType::Registration,
        Some(client_ip),
        Some(&user.id),
        Some(&email),
        None,
        None,
    );
    (StatusCode::CREATED, Json(auth_response(user, session))).into_response()
}

const MAX_LOGIN_ATTEMPTS: i64 = 5;
const LOCKOUT_DURATION_MINUTES: i64 = 15;

pub async fn login(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    AUTH_METRICS.login_attempts.fetch_add(1, Ordering::Relaxed);
    let client_ip = get_client_ip(&headers, addr);
    if !ensure_auth_rate_limit("login", client_ip) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let email = normalize_email(&payload.email);
    let failure_key = format!("email:{}", email);
    let ip_failure_key = format!("ip:{}", client_ip);

    // 1. Check Account Lockout
    if let Ok(Some((count, last_failure))) = db::get_login_failures(&pool, &failure_key).await {
        if count >= MAX_LOGIN_ATTEMPTS {
            let lockout_until = last_failure + chrono::Duration::minutes(LOCKOUT_DURATION_MINUTES);
            if Utc::now() < lockout_until {
                AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    client_ip = %client_ip,
                    email = %email,
                    "login rejected: account locked"
                );
                return error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Account locked due to too many failed attempts. Try again later.",
                );
            }
        }
    }

    // 2. Check IP Lockout
    if let Ok(Some((count, last_failure))) = db::get_login_failures(&pool, &ip_failure_key).await {
        if count >= MAX_LOGIN_ATTEMPTS {
            let lockout_until = last_failure + chrono::Duration::minutes(LOCKOUT_DURATION_MINUTES);
            if Utc::now() < lockout_until {
                AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
                log_security_event(
                    SecurityEventType::RateLimitExceeded,
                    Some(client_ip),
                    None,
                    Some(&email),
                    None,
                    Some(&format!("{} failed login attempts", count)),
                );
                return error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many failed attempts from this IP. Try again later.",
                );
            }
        }
    }

    if !security::is_valid_email(&email) || !is_reasonable_password_input(&payload.password) {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid login payload");
    }

    let user = match db::get_user_by_email(&pool, &email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            db::record_login_failure(&pool, &ip_failure_key).await.ok();
            log_security_event(
                SecurityEventType::LoginFailure,
                Some(client_ip),
                None,
                Some(&email),
                Some(FailureReason::UserNotFound),
                None,
            );
            return error_response(StatusCode::UNAUTHORIZED, "Invalid credentials");
        }
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            log_security_event(
                SecurityEventType::DatabaseError,
                Some(client_ip),
                None,
                Some(&email),
                None,
                Some(&err.to_string()),
            );
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Database error");
        }
    };

    // 2. Check User Ban
    if let Ok(Some(reason)) = db::check_ban_status(&pool, &user.id).await {
        tracing::warn!(
            client_ip = %client_ip,
            user_id = %user.id,
            "login rejected: user banned"
        );
        return error_response(
            StatusCode::FORBIDDEN,
            format!("Account suspended: {}", reason),
        );
    }

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(hash) => hash,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
        db::record_login_failure(&pool, &failure_key).await.ok();
        db::record_login_failure(&pool, &ip_failure_key).await.ok();
        log_security_event(
            SecurityEventType::LoginFailure,
            Some(client_ip),
            Some(&user.id),
            Some(&email),
            Some(FailureReason::InvalidPassword),
            None,
        );
        return error_response(StatusCode::UNAUTHORIZED, "Invalid credentials");
    }

    if let Some(stored_secret) = &user.totp_secret {
        let Some(code) = payload.totp_code.as_deref() else {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            log_security_event(
                SecurityEventType::LoginFailure,
                Some(client_ip),
                Some(&user.id),
                Some(&email),
                Some(FailureReason::TotpRequired),
                None,
            );
            return error_response(StatusCode::UNAUTHORIZED, "2FA required");
        };

        if !security::is_valid_totp_code(code) {
            AUTH_METRICS
                .validation_errors
                .fetch_add(1, Ordering::Relaxed);
            db::record_login_failure(&pool, &failure_key).await.ok();
            return error_response(StatusCode::UNAUTHORIZED, "Invalid 2FA code");
        }

        let secret = match security::decrypt_totp_secret(stored_secret) {
            Ok(secret) => secret,
            Err(err) => {
                AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
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
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            db::record_login_failure(&pool, &failure_key).await.ok();
            db::record_login_failure(&pool, &ip_failure_key).await.ok();
            tracing::warn!(
                client_ip = %client_ip,
                user_id = %user.id,
                "login failed: invalid 2FA code"
            );
            return error_response(StatusCode::UNAUTHORIZED, "Invalid 2FA code");
        }
    }

    // Success - reset failures
    db::reset_login_failure(&pool, &failure_key).await.ok();
    db::reset_login_failure(&pool, &ip_failure_key).await.ok();

    let session = match db::create_session(&pool, &user.id, Some(client_ip.to_string())).await {
        Ok(session) => session,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    AUTH_METRICS.login_success.fetch_add(1, Ordering::Relaxed);
    log_security_event(
        SecurityEventType::LoginSuccess,
        Some(client_ip),
        Some(&user.id),
        Some(&email),
        None,
        None,
    );
    (StatusCode::OK, Json(auth_response(user, session))).into_response()
}

pub async fn setup_totp(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    AUTH_METRICS
        .totp_setup_attempts
        .fetch_add(1, Ordering::Relaxed);
    let client_ip = get_client_ip(&headers, addr);
    if !ensure_auth_rate_limit("totp_setup", client_ip) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let email = normalize_email(&payload.email);

    if !security::is_valid_email(&email) || !is_reasonable_password_input(&payload.password) {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid 2FA setup payload");
    }

    let user = match db::get_user_by_email(&pool, &email).await {
        Ok(Some(user)) => user,
        _ => {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
        }
    };

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
    }

    // If the user already has TOTP configured, require the current TOTP code.
    // Without this check, an attacker with only the password could replace 2FA.
    if let Some(stored_secret) = &user.totp_secret {
        let existing_code = match payload.totp_code.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => {
                AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "Current 2FA code required to change 2FA settings",
                );
            }
        };
        if !security::is_valid_totp_code(existing_code) {
            AUTH_METRICS
                .validation_errors
                .fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::BAD_REQUEST, "Invalid 2FA code format");
        }
        let existing_secret = match security::decrypt_totp_secret(stored_secret) {
            Ok(s) => s,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                )
            }
        };
        let existing_totp = match totp_from_secret(&existing_secret) {
            Ok(t) => t,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                )
            }
        };
        if !existing_totp.check_current(existing_code).unwrap_or(false) {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::UNAUTHORIZED, "Invalid current 2FA code");
        }
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
        email.clone(),
    ) {
        Ok(totp) => totp,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create TOTP secret: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "2FA setup failed");
        }
    };

    let secret_encoded = totp.get_secret_base32();
    let qr_png_base64 = match totp.get_qr_base64() {
        Ok(qr) => qr,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create TOTP QR: {}", err);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "2FA QR generation failed",
            );
        }
    };

    AUTH_METRICS
        .totp_setup_success
        .fetch_add(1, Ordering::Relaxed);
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
    headers: HeaderMap,
    Json(payload): Json<EnableTotpRequest>,
) -> impl IntoResponse {
    AUTH_METRICS
        .totp_enable_attempts
        .fetch_add(1, Ordering::Relaxed);
    let client_ip = get_client_ip(&headers, addr);
    if !ensure_auth_rate_limit("totp_enable", client_ip) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let email = normalize_email(&payload.email);
    let secret = normalize_totp_secret(&payload.secret);

    if !security::is_valid_email(&email)
        || !is_reasonable_password_input(&payload.password)
        || !security::is_valid_totp_code(&payload.code)
    {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid 2FA enable payload");
    }

    let user = match db::get_user_by_email(&pool, &email).await {
        Ok(Some(user)) => user,
        _ => {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
        }
    };

    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid stored hash"),
    };

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::UNAUTHORIZED, "Auth failed");
    }

    // If the user already has TOTP configured, require the existing TOTP code.
    // Without this, an attacker with only the password could replace the 2FA secret.
    if let Some(stored_secret) = &user.totp_secret {
        let existing_code = match payload.existing_totp_code.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => {
                AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "Current 2FA code required to replace 2FA settings",
                );
            }
        };
        if !security::is_valid_totp_code(existing_code) {
            AUTH_METRICS
                .validation_errors
                .fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::BAD_REQUEST, "Invalid existing 2FA code format");
        }
        let existing_secret = match security::decrypt_totp_secret(stored_secret) {
            Ok(s) => s,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                )
            }
        };
        let existing_totp = match totp_from_secret(&existing_secret) {
            Ok(t) => t,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "2FA verification unavailable",
                )
            }
        };
        if !existing_totp.check_current(existing_code).unwrap_or(false) {
            AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::UNAUTHORIZED, "Invalid existing 2FA code");
        }
    }

    let totp = match totp_from_secret(&secret) {
        Ok(totp) => totp,
        Err(_) => {
            AUTH_METRICS
                .validation_errors
                .fetch_add(1, Ordering::Relaxed);
            return error_response(StatusCode::BAD_REQUEST, "Invalid TOTP secret");
        }
    };

    if !totp.check_current(&payload.code).unwrap_or(false) {
        AUTH_METRICS.auth_failures.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid code");
    }

    let encrypted_secret = match security::encrypt_totp_secret(&secret) {
        Ok(secret) => secret,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to encrypt TOTP secret: {}", err);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "2FA secret encryption failed",
            );
        }
    };

    if let Err(err) = db::enable_totp(&pool, &user.id, &encrypted_secret).await {
        AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
        tracing::error!("failed to enable TOTP in DB: {}", err);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "DB error");
    }

    let refreshed_user = match db::get_user_by_email(&pool, &email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "User missing after 2FA update",
            )
        }
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to fetch refreshed user: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "DB error");
        }
    };

    let session = match db::create_session(&pool, &user.id, Some(client_ip.to_string())).await {
        Ok(session) => session,
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            tracing::error!("failed to create session: {}", err);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed");
        }
    };

    AUTH_METRICS
        .totp_enable_success
        .fetch_add(1, Ordering::Relaxed);
    log_security_event(
        SecurityEventType::TotpEnabled,
        Some(client_ip),
        Some(&user.id),
        Some(&email),
        None,
        None,
    );
    (StatusCode::OK, Json(auth_response(refreshed_user, session))).into_response()
}

pub async fn logout(
    State(pool): State<SqlitePool>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    AUTH_METRICS.logout_attempts.fetch_add(1, Ordering::Relaxed);
    let client_ip = get_client_ip(&headers, addr);
    if !ensure_auth_rate_limit("logout", client_ip) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
    }

    let Some(token) = extract_session_token(&headers) else {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Missing bearer token");
    };
    if !security::is_valid_session_token(&token) {
        AUTH_METRICS
            .validation_errors
            .fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::BAD_REQUEST, "Invalid session token");
    }

    let post_auth_key = format!("logout:{}:{}", client_ip, security::hash_token(&token));
    if !security::allow_post_auth_request(&post_auth_key) {
        AUTH_METRICS.rate_limited.fetch_add(1, Ordering::Relaxed);
        return error_response(StatusCode::TOO_MANY_REQUESTS, "Too many logout requests");
    }

    match db::revoke_session(&pool, &token).await {
        Ok(revoked) => {
            AUTH_METRICS.logout_success.fetch_add(1, Ordering::Relaxed);
            // Note: We don't have user_id here without querying the session first,
            // but logging the logout event with IP is still valuable for audit
            log_security_event(
                SecurityEventType::Logout,
                Some(client_ip),
                None, // user_id not readily available
                None, // email not readily available
                None,
                None,
            );
            (StatusCode::OK, Json(LogoutResponse { revoked })).into_response()
        }
        Err(err) => {
            AUTH_METRICS.db_errors.fetch_add(1, Ordering::Relaxed);
            log_security_event(
                SecurityEventType::DatabaseError,
                Some(client_ip),
                None,
                None,
                None,
                Some(&err.to_string()),
            );
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "Logout failed")
        }
    }
}
