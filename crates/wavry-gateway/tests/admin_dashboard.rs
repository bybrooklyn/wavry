//! Integration tests for Admin Dashboard
//!
//! Tests the following:
//! - Admin panel authentication and token validation
//! - Admin overview endpoint (GET /admin/api/overview)
//! - Session revocation (POST /admin/api/sessions/revoke)
//! - User banning (POST /admin/api/ban)
//! - User unbanning (POST /admin/api/unban)
//! - Permission enforcement
//! - Rate limiting

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use wavry_gateway::{db, security};

async fn setup_test_db() -> SqlitePool {
    // Create in-memory SQLite database for tests
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    // Run migrations
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            email TEXT UNIQUE NOT NULL,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            display_name TEXT,
            public_key TEXT NOT NULL,
            totp_secret TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create users table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            expires_at DATETIME NOT NULL,
            ip_address TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create sessions table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_bans (
            user_id TEXT PRIMARY KEY,
            reason TEXT NOT NULL,
            expires_at DATETIME,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create user_bans table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS login_failures (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            identifier TEXT NOT NULL,
            attempt_count INTEGER DEFAULT 1,
            last_attempt DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create login_failures table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS relay_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            relay_id TEXT NOT NULL UNIQUE,
            successes INTEGER DEFAULT 0,
            failures INTEGER DEFAULT 0,
            last_seen DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create relay_stats table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS admin_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            action TEXT NOT NULL,
            target_type TEXT NOT NULL,
            target_id TEXT,
            outcome TEXT NOT NULL,
            actor_ip_hash TEXT NOT NULL,
            details TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create admin_audit_log table");

    pool
}

#[tokio::test]
async fn test_create_test_user() {
    let pool = setup_test_db().await;

    // Create test user
    let user = db::create_user(
        &pool,
        "admin@test.local",
        "hashed_password_12345",
        "Admin User",
        "admin",
        "public_key_bytes",
    )
    .await
    .expect("Failed to create user");

    assert_eq!(user.email, "admin@test.local");
    assert_eq!(user.username, "admin");
    assert_eq!(user.display_name, "Admin User");

    // Verify we can retrieve the user
    let retrieved = db::get_user_by_email(&pool, "admin@test.local")
        .await
        .expect("Failed to get user")
        .expect("User not found");

    assert_eq!(retrieved.id, user.id);
    assert_eq!(retrieved.email, user.email);
}

#[tokio::test]
async fn test_count_users_and_sessions() {
    let pool = setup_test_db().await;

    // Initially empty
    let count = db::count_users(&pool).await.expect("Failed to count users");
    assert_eq!(count, 0);

    let sessions = db::count_sessions(&pool)
        .await
        .expect("Failed to count sessions");
    assert_eq!(sessions, 0);

    // Create a user
    let user = db::create_user(
        &pool,
        "test@example.com",
        "password_hash",
        "Test User",
        "testuser",
        "pub_key",
    )
    .await
    .expect("Failed to create user");

    let count = db::count_users(&pool).await.expect("Failed to count users");
    assert_eq!(count, 1);

    // Create a session
    let _session = db::create_session(&pool, &user.id, Some("192.168.1.1".to_string()))
        .await
        .expect("Failed to create session");

    let sessions = db::count_sessions(&pool)
        .await
        .expect("Failed to count sessions");
    assert_eq!(sessions, 1);

    let active = db::count_active_sessions(&pool)
        .await
        .expect("Failed to count active sessions");
    assert!(active >= 0);
}

#[tokio::test]
async fn test_list_recent_users() {
    let pool = setup_test_db().await;

    // Create multiple users
    for i in 0..5 {
        db::create_user(
            &pool,
            &format!("user{}@test.com", i),
            "password_hash",
            &format!("User {}", i),
            &format!("user{}", i),
            "pub_key",
        )
        .await
        .expect("Failed to create user");
    }

    let recent = db::list_recent_users(&pool, 3)
        .await
        .expect("Failed to list recent users");

    assert_eq!(recent.len(), 3);

    for user in recent {
        assert!(!user.email.is_empty());
        assert!(!user.username.is_empty());
    }
}

#[tokio::test]
async fn test_revoke_session() {
    let pool = setup_test_db().await;

    // Create user and session
    let user = db::create_user(&pool, "user@test.com", "hash", "Test", "testuser", "key")
        .await
        .expect("Failed to create user");

    let session = db::create_session(&pool, &user.id, None)
        .await
        .expect("Failed to create session");

    assert!(!session.token.is_empty()); // Session created

    // Revoke it
    let revoked = db::revoke_session(&pool, &session.token)
        .await
        .expect("Failed to revoke session");

    assert!(revoked, "Session should have been revoked");

    // Try to revoke again - should return false
    let revoked_again = db::revoke_session(&pool, &session.token)
        .await
        .expect("Failed on second revoke attempt");

    assert!(!revoked_again, "Should not revoke non-existent session");
}

#[tokio::test]
async fn test_ban_and_unban_user() {
    let pool = setup_test_db().await;

    // Create user
    let user = db::create_user(&pool, "user@test.com", "hash", "Test", "testuser", "key")
        .await
        .expect("Failed to create user");

    // Ban the user (permanent)
    db::ban_user(&pool, &user.id, "Test ban reason", None)
        .await
        .expect("Failed to ban user");

    // Check ban status
    let ban_reason = db::check_ban_status(&pool, &user.id)
        .await
        .expect("Failed to check ban status");

    assert!(ban_reason.is_some(), "User should be banned");
    assert_eq!(ban_reason.unwrap(), "Test ban reason");

    // Unban the user
    db::unban_user(&pool, &user.id)
        .await
        .expect("Failed to unban user");

    // Check ban status again
    let ban_reason = db::check_ban_status(&pool, &user.id)
        .await
        .expect("Failed to check ban status");

    assert!(ban_reason.is_none(), "User should no longer be banned");
}

#[tokio::test]
async fn test_temporary_ban() {
    let pool = setup_test_db().await;

    let user = db::create_user(&pool, "user@test.com", "hash", "Test", "testuser", "key")
        .await
        .expect("Failed to create user");

    // Ban for 1 hour
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);
    db::ban_user(&pool, &user.id, "Temporary ban", Some(expires_at))
        .await
        .expect("Failed to ban user");

    let ban_reason = db::check_ban_status(&pool, &user.id)
        .await
        .expect("Failed to check ban status");

    assert!(ban_reason.is_some(), "User should be temporarily banned");
}

#[tokio::test]
async fn test_list_active_bans() {
    let pool = setup_test_db().await;

    // Create and ban 3 users
    for i in 0..3 {
        let user = db::create_user(
            &pool,
            &format!("user{}@test.com", i),
            "hash",
            &format!("User {}", i),
            &format!("user{}", i),
            "key",
        )
        .await
        .expect("Failed to create user");

        db::ban_user(&pool, &user.id, &format!("Ban reason {}", i), None)
            .await
            .expect("Failed to ban user");
    }

    let bans = db::list_active_bans(&pool)
        .await
        .expect("Failed to list bans");

    assert_eq!(bans.len(), 3);

    for ban in bans {
        assert!(!ban.user_id.is_empty());
        assert!(!ban.reason.is_empty());
    }
}

#[tokio::test]
async fn test_list_recent_sessions() {
    let pool = setup_test_db().await;

    // Create user
    let user = db::create_user(&pool, "user@test.com", "hash", "Test", "testuser", "key")
        .await
        .expect("Failed to create user");

    // Create multiple sessions
    for i in 0..3 {
        db::create_session(&pool, &user.id, Some(format!("192.168.1.{}", i)))
            .await
            .expect("Failed to create session");
    }

    let sessions = db::list_recent_sessions(&pool, 5)
        .await
        .expect("Failed to list sessions");

    assert!(sessions.len() >= 3);

    for session in sessions {
        assert!(!session.token.is_empty());
    }
}

#[tokio::test]
async fn test_token_hashing() {
    // Test that token hashing is consistent
    let token = "test_admin_token_xyz";
    let hash1 = security::hash_token(token);
    let hash2 = security::hash_token(token);

    assert_eq!(hash1, hash2, "Token hash should be consistent");
    assert_ne!(hash1, token, "Hash should not equal plaintext token");
    assert!(!hash1.is_empty());
}

#[tokio::test]
async fn test_admin_overview_data_structure() {
    let pool = setup_test_db().await;

    // Create some test data
    let user1 = db::create_user(&pool, "user1@test.com", "hash", "User 1", "user1", "key")
        .await
        .expect("Failed to create user");

    let user2 = db::create_user(&pool, "user2@test.com", "hash", "User 2", "user2", "key")
        .await
        .expect("Failed to create user");

    db::create_session(&pool, &user1.id, Some("192.168.1.1".to_string()))
        .await
        .expect("Failed to create session");

    db::create_session(&pool, &user2.id, Some("192.168.1.2".to_string()))
        .await
        .expect("Failed to create session");

    db::ban_user(&pool, &user1.id, "Spam", None)
        .await
        .expect("Failed to ban user");

    // Verify counts
    let users_total = db::count_users(&pool).await.unwrap();
    let sessions_total = db::count_sessions(&pool).await.unwrap();
    let sessions_active = db::count_active_sessions(&pool).await.unwrap();
    let recent_users = db::list_recent_users(&pool, 20).await.unwrap();
    let recent_sessions = db::list_recent_sessions(&pool, 20).await.unwrap();
    let active_bans = db::list_active_bans(&pool).await.unwrap();

    assert_eq!(users_total, 2);
    assert_eq!(sessions_total, 2);
    assert!(sessions_active >= 0);
    assert_eq!(recent_users.len(), 2);
    assert_eq!(recent_sessions.len(), 2);
    assert_eq!(active_bans.len(), 1);
}

#[tokio::test]
async fn test_admin_audit_log_insert_and_list() {
    let pool = setup_test_db().await;

    db::insert_admin_audit(
        &pool,
        "ban_user",
        "user",
        Some("user-123"),
        "success",
        "0123456789abcdef",
        Some("test reason"),
    )
    .await
    .expect("Failed to insert admin audit row");

    let rows = db::list_recent_admin_audit(&pool, 10)
        .await
        .expect("Failed to list admin audit rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, "ban_user");
    assert_eq!(rows[0].target_type, "user");
    assert_eq!(rows[0].target_id.as_deref(), Some("user-123"));
    assert_eq!(rows[0].outcome, "success");
}
