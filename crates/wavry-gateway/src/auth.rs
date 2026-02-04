use axum::{
    extract::{State, Json},
    response::IntoResponse,
    http::StatusCode,
};
use sqlx::SqlitePool;
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString
    },
    Argon2
};
use serde::{Deserialize, Serialize};
use crate::db::{self, User, Session};

use totp_rs::{Algorithm, TOTP};
use rand::{Rng, thread_rng};


// DTOs
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

// Routes

pub async fn register(
    State(pool): State<SqlitePool>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    // 1. Check if user exists
    if let Ok(Some(_)) = db::get_user_by_email(&pool, &payload.email).await {
        return (StatusCode::CONFLICT, Json(ErrorResponse { error: "Email already exists".into() })).into_response();
    }

    // 2. Hash Password
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = match argon2.hash_password(payload.password.as_bytes(), &salt) {
        Ok(hash) => hash.to_string(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Hashing failed".into() })).into_response(),
    };

    // 3. Create User
    let user = match db::create_user(
        &pool, 
        &payload.email, 
        &password_hash, 
        &payload.display_name,
        &payload.username,
        &payload.public_key
    ).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to create user: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Database error".into() })).into_response();
        }
    };

    // 4. Create Session
    let session = match db::create_session(&pool, &user.id, None).await { // TODO: Get IP from header
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create session: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Session creation failed".into() })).into_response();
        }
    };

    (StatusCode::CREATED, Json(AuthResponse { user, session })).into_response()
}

pub async fn login(
    State(pool): State<SqlitePool>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    // 1. Get User
    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid credentials".into() })).into_response(),
        Err(e) => {
             tracing::error!("Database error: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Database error".into() })).into_response();
        }
    };

    // 2. Verify Password
    let parsed_hash = match PasswordHash::new(&user.password_hash) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Invalid stored hash".into() })).into_response(),
    };

    let argon2 = Argon2::default();
    if argon2.verify_password(payload.password.as_bytes(), &parsed_hash).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid credentials".into() })).into_response();
    }

    // 2.5 Verify TOTP if enabled
    if let Some(secret) = &user.totp_secret {
        match payload.totp_code {
            Some(code) => {
                let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, secret.clone().into_bytes().to_vec(), None, "wavry".to_string()).unwrap();
                if !totp.check_current(&code).unwrap_or(false) {
                    return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid 2FA code".into() })).into_response();
                }
            },
            None => {
                 return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "2FA required".into() })).into_response();
            }
        }
    }

    // 3. Create Session
    let session = match db::create_session(&pool, &user.id, None).await {
        Ok(s) => s,
        Err(e) => {
             tracing::error!("Failed to create session: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Session creation failed".into() })).into_response();
        }
    };

    (StatusCode::OK, Json(AuthResponse { user, session })).into_response()
}

pub async fn setup_totp(
    State(pool): State<SqlitePool>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    // 1. Verify User credentials again
    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(u)) => u,
        _ => return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Auth failed".into() })).into_response(),
    };
    
    // Check password
    let parsed_hash = PasswordHash::new(&user.password_hash).unwrap();
    if Argon2::default().verify_password(payload.password.as_bytes(), &parsed_hash).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Auth failed".into() })).into_response();
    }

    // 2. Generate Secret
    let mut secret_bytes = [0u8; 20];
    thread_rng().fill(&mut secret_bytes);
    let secret_vec = secret_bytes.to_vec();
    
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_vec,
        Some("Wavry".to_string()),
        payload.email.clone(),
    ).unwrap();

    let secret_encoded = totp.get_secret_base32();

    let qr = totp.get_qr_base64().unwrap();
    
    (StatusCode::OK, Json(TotpSetupResponse { 
        secret: secret_encoded, 
        qr_png_base64: qr 
    })).into_response()
}

#[derive(Deserialize)]
pub struct EnableTotpRequest {
    pub email: String,
    pub password: String,
    pub secret: String,
    pub code: String,
}

pub async fn enable_totp(
    State(pool): State<SqlitePool>,
    Json(payload): Json<EnableTotpRequest>,
) -> impl IntoResponse {
     // 1. Verify User
    let user = match db::get_user_by_email(&pool, &payload.email).await {
        Ok(Some(u)) => u,
        _ => return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Auth failed".into() })).into_response(),
    };
    
    // Check password
    let parsed_hash = PasswordHash::new(&user.password_hash).unwrap();
    if Argon2::default().verify_password(payload.password.as_bytes(), &parsed_hash).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Auth failed".into() })).into_response();
    }

    // 2. Verify Code matches Secret
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        payload.secret.clone().into_bytes().to_vec(),
        None,
        "wavry".to_string()
    ).unwrap();

    if !totp.check_current(&payload.code).unwrap_or(false) {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid code".into() })).into_response();
    }

    // 3. Enable in DB
    if let Err(e) = db::enable_totp(&pool, &user.id, &payload.secret).await {
        tracing::error!("DB error: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "DB error".into() })).into_response();
    }

    (StatusCode::OK, Json(AuthResponse { 
        user: db::get_user_by_email(&pool, &payload.email).await.unwrap().unwrap(),
        session: db::create_session(&pool, &user.id, None).await.unwrap()
    })).into_response()
}
