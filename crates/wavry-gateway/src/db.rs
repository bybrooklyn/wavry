use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

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
    // Generate secure random token
    // Using a simple UUID for now, but in production should be high-entropy 32-byte hex
    let token =
        Uuid::new_v4().to_string().replace("-", "") + &Uuid::new_v4().to_string().replace("-", "");

    // Expires in 24 hours
    let expires_at = Utc::now() + chrono::Duration::hours(24);

    let session = sqlx::query_as::<_, Session>(
        r#"
        INSERT INTO sessions (token, user_id, expires_at, ip_address)
        VALUES (?, ?, ?, ?)
        RETURNING token, user_id, expires_at, ip_address, created_at
        "#,
    )
    .bind(&token)
    .bind(user_id)
    .bind(expires_at)
    .bind(ip_address)
    .fetch_one(pool)
    .await?;

    Ok(session)
}

pub async fn enable_totp(pool: &SqlitePool, user_id: &str, secret: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET totp_secret = ? WHERE id = ?")
        .bind(secret)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}
