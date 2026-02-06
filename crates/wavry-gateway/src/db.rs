use chrono::{DateTime, Utc};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::security;

const SESSION_HASH_PREFIX: &str = "h1:";

fn storage_token_for_bearer(token: &str) -> String {
    format!("{}{}", SESSION_HASH_PREFIX, security::hash_token(token))
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub username: String,
    pub public_key: String,
    #[serde(skip)]
    pub password_hash: String,
    pub display_name: String,
    #[serde(skip)]
    pub totp_secret: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Session {
    pub token: String,
    pub user_id: String,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AdminUserRow {
    pub id: String,
    pub email: String,
    pub username: String,
    pub display_name: String,
    pub has_totp: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AdminSessionRow {
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub username: String,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

// DB Operations

pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    password_hash: &str,
    display_name: &str,
    username: &str,
    public_key: &str,
) -> anyhow::Result<User> {
    let id = Uuid::new_v4().to_string();
    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (id, email, password_hash, display_name, username, public_key)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id, email, password_hash, display_name, username, public_key, totp_secret, created_at
        "#
    )
    .bind(&id)
    .bind(email)
    .bind(password_hash)
    .bind(display_name)
    .bind(username)
    .bind(public_key)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn create_session(
    pool: &SqlitePool,
    user_id: &str,
    ip_address: Option<String>,
) -> anyhow::Result<Session> {
    // Generate high-entropy random token and store only a hash in DB.
    let mut token_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut token_bytes);
    let token = hex::encode(token_bytes);
    let stored_token = storage_token_for_bearer(&token);

    // Expires in 24 hours
    let expires_at = Utc::now() + chrono::Duration::hours(24);

    sqlx::query(
        r#"
        INSERT INTO sessions (token, user_id, expires_at, ip_address)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&stored_token)
    .bind(user_id)
    .bind(expires_at)
    .bind(ip_address.clone())
    .execute(pool)
    .await?;

    Ok(Session {
        token,
        user_id: user_id.to_string(),
        expires_at,
        ip_address,
        created_at: Utc::now(),
    })
}

pub async fn enable_totp(pool: &SqlitePool, user_id: &str, secret: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET totp_secret = ? WHERE id = ?")
        .bind(secret)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn count_users(pool: &SqlitePool) -> anyhow::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn count_sessions(pool: &SqlitePool) -> anyhow::Result<i64> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn count_active_sessions(pool: &SqlitePool) -> anyhow::Result<i64> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE expires_at > datetime('now')")
            .fetch_one(pool)
            .await?;
    Ok(count)
}

pub async fn list_recent_users(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<AdminUserRow>> {
    let rows = sqlx::query_as::<_, AdminUserRow>(
        r#"
        SELECT
            id,
            email,
            username,
            display_name,
            CASE WHEN totp_secret IS NULL OR totp_secret = '' THEN 0 ELSE 1 END as has_totp,
            created_at
        FROM users
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_recent_sessions(
    pool: &SqlitePool,
    limit: i64,
) -> anyhow::Result<Vec<AdminSessionRow>> {
    let rows = sqlx::query_as::<_, AdminSessionRow>(
        r#"
        SELECT
            s.token,
            s.user_id,
            u.email,
            u.username,
            s.expires_at,
            s.ip_address,
            s.created_at
        FROM sessions s
        JOIN users u ON s.user_id = u.id
        ORDER BY s.created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn revoke_session(pool: &SqlitePool, token: &str) -> anyhow::Result<bool> {
    let stored_token = storage_token_for_bearer(token);
    let result = sqlx::query("DELETE FROM sessions WHERE token = ?")
        .bind(stored_token)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_username_by_session_token(
    pool: &SqlitePool,
    token: &str,
) -> anyhow::Result<Option<String>> {
    let stored_token = storage_token_for_bearer(token);
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT u.username
        FROM sessions s
        JOIN users u ON s.user_id = u.id
        WHERE s.token = ? AND s.expires_at > datetime('now')
        "#,
    )
    .bind(stored_token)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|v| v.0))
}

pub async fn delete_expired_sessions(pool: &SqlitePool) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
