use axum::{
    extract::{ConnectInfo, Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::SocketAddr;

use crate::db;
use crate::security;

#[derive(Serialize)]
struct AdminError {
    error: String,
}

#[derive(Serialize)]
pub struct AdminOverview {
    users_total: i64,
    sessions_total: i64,
    sessions_active: i64,
    recent_users: Vec<db::AdminUserRow>,
    recent_sessions: Vec<db::AdminSessionRow>,
    active_bans: Vec<db::AdminBannedUserRow>,
}

#[derive(Deserialize)]
pub struct RevokeSessionRequest {
    token: String,
}

#[derive(Serialize)]
pub struct RevokeSessionResponse {
    revoked: bool,
}

#[derive(Deserialize)]
pub struct BanUserRequest {
    pub user_id: String,
    pub reason: String,
    pub duration_hours: Option<i64>,
}

#[derive(Deserialize)]
pub struct UnbanUserRequest {
    pub user_id: String,
}

#[derive(Serialize)]
pub struct SimpleAdminResponse {
    pub success: bool,
}

fn unauthorized(message: &str) -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(AdminError {
            error: message.to_string(),
        }),
    )
        .into_response()
}

fn extract_admin_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-admin-token") {
        if let Ok(token) = value.to_str() {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

enum AdminAuthError {
    Disabled,
    Invalid,
}

fn assert_admin(headers: &HeaderMap) -> Result<(), AdminAuthError> {
    let expected = std::env::var("ADMIN_PANEL_TOKEN").unwrap_or_default();
    if expected.len() < 32 {
        return Err(AdminAuthError::Disabled);
    }

    let got = extract_admin_token(headers);
    let Some(got) = got else {
        return Err(AdminAuthError::Invalid);
    };

    if !wavry_common::helpers::constant_time_eq(&got, &expected) {
        return Err(AdminAuthError::Invalid);
    }

    Ok(())
}

fn check_admin_rate_limit(addr: SocketAddr) -> bool {
    let key = format!("admin:{}", addr.ip());
    security::allow_auth_request(&key)
}

pub async fn admin_panel() -> impl IntoResponse {
    Html(ADMIN_HTML)
}

pub async fn admin_overview(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(pool): State<SqlitePool>,
) -> impl IntoResponse {
    if !check_admin_rate_limit(addr) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AdminError {
                error: "Too many admin requests".to_string(),
            }),
        )
            .into_response();
    }

    if let Err(err) = assert_admin(&headers) {
        return match err {
            AdminAuthError::Disabled => unauthorized("admin panel disabled: set ADMIN_PANEL_TOKEN"),
            AdminAuthError::Invalid => unauthorized("invalid admin token"),
        };
    }

    let users_total = match db::count_users(&pool).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to count users: {e}"),
                }),
            )
                .into_response();
        }
    };

    let sessions_total = match db::count_sessions(&pool).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to count sessions: {e}"),
                }),
            )
                .into_response();
        }
    };

    let sessions_active = match db::count_active_sessions(&pool).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to count active sessions: {e}"),
                }),
            )
                .into_response();
        }
    };

    let recent_users = match db::list_recent_users(&pool, 20).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to list users: {e}"),
                }),
            )
                .into_response();
        }
    };

    let recent_sessions = match db::list_recent_sessions(&pool, 20).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to list sessions: {e}"),
                }),
            )
                .into_response();
        }
    };

    let active_bans = match db::list_active_bans(&pool).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminError {
                    error: format!("failed to list bans: {e}"),
                }),
            )
                .into_response();
        }
    };

    let payload = AdminOverview {
        users_total,
        sessions_total,
        sessions_active,
        recent_users,
        recent_sessions,
        active_bans,
    };

    (StatusCode::OK, Json(payload)).into_response()
}

pub async fn admin_revoke_session(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(pool): State<SqlitePool>,
    Json(payload): Json<RevokeSessionRequest>,
) -> impl IntoResponse {
    if !check_admin_rate_limit(addr) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AdminError {
                error: "Too many admin requests".to_string(),
            }),
        )
            .into_response();
    }

    if let Err(err) = assert_admin(&headers) {
        return match err {
            AdminAuthError::Disabled => unauthorized("admin panel disabled: set ADMIN_PANEL_TOKEN"),
            AdminAuthError::Invalid => unauthorized("invalid admin token"),
        };
    }

    match db::revoke_session(&pool, &payload.token).await {
        Ok(revoked) => (StatusCode::OK, Json(RevokeSessionResponse { revoked })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminError {
                error: format!("failed to revoke session: {e}"),
            }),
        )
            .into_response(),
    }
}

pub async fn admin_ban_user(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(pool): State<SqlitePool>,
    Json(payload): Json<BanUserRequest>,
) -> impl IntoResponse {
    if !check_admin_rate_limit(addr) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AdminError {
                error: "Too many admin requests".to_string(),
            }),
        )
            .into_response();
    }

    if let Err(err) = assert_admin(&headers) {
        return match err {
            AdminAuthError::Disabled => unauthorized("admin panel disabled: set ADMIN_PANEL_TOKEN"),
            AdminAuthError::Invalid => unauthorized("invalid admin token"),
        };
    }

    let expires_at = payload
        .duration_hours
        .map(|h| chrono::Utc::now() + chrono::Duration::hours(h));

    match db::ban_user(&pool, &payload.user_id, &payload.reason, expires_at).await {
        Ok(_) => (StatusCode::OK, Json(SimpleAdminResponse { success: true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminError {
                error: format!("failed to ban user: {e}"),
            }),
        )
            .into_response(),
    }
}

pub async fn admin_unban_user(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(pool): State<SqlitePool>,
    Json(payload): Json<UnbanUserRequest>,
) -> impl IntoResponse {
    if !check_admin_rate_limit(addr) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AdminError {
                error: "Too many admin requests".to_string(),
            }),
        )
            .into_response();
    }

    if let Err(err) = assert_admin(&headers) {
        return match err {
            AdminAuthError::Disabled => unauthorized("admin panel disabled: set ADMIN_PANEL_TOKEN"),
            AdminAuthError::Invalid => unauthorized("invalid admin token"),
        };
    }

    match db::unban_user(&pool, &payload.user_id).await {
        Ok(_) => (StatusCode::OK, Json(SimpleAdminResponse { success: true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AdminError {
                error: format!("failed to unban user: {e}"),
            }),
        )
            .into_response(),
    }
}

const ADMIN_HTML: &str = r#"<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
  <title>Wavry Admin</title>
  <style>
    body { font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif; margin: 24px; background: #0b0f16; color: #e6edf3; }
    h1 { margin: 0 0 16px; }
    .row { display: flex; gap: 12px; margin-bottom: 16px; }
    input, button { padding: 10px 12px; border-radius: 8px; border: 1px solid #2d3748; background: #111827; color: #e6edf3; }
    button { cursor: pointer; }
    .grid { display: grid; grid-template-columns: repeat(3, minmax(120px, 220px)); gap: 12px; margin: 16px 0; }
    .card { background: #111827; border: 1px solid #2d3748; border-radius: 10px; padding: 12px; }
    table { width: 100%; border-collapse: collapse; margin-top: 16px; }
    th, td { border-bottom: 1px solid #1f2937; text-align: left; padding: 8px; font-size: 13px; }
    .muted { color: #9ca3af; font-size: 12px; }
  </style>
</head>
<body>
  <h1>Wavry Auth Admin</h1>
  <div class=\"row\">
    <input id=\"token\" type=\"password\" placeholder=\"Admin token\" style=\"width: 340px\" />
    <button id=\"load\">Load</button>
  </div>

  <div class=\"grid\">
    <div class=\"card\"><div class=\"muted\">Users</div><div id=\"users\">-</div></div>
    <div class=\"card\"><div class=\"muted\">Sessions</div><div id=\"sessions\">-</div></div>
    <div class=\"card\"><div class=\"muted\">Active Sessions</div><div id=\"active\">-</div></div>
  </div>

  <h3>Recent Users</h3>
  <table id=\"usersTable\"><thead><tr><th>Email</th><th>Username</th><th>TOTP</th><th>Created</th><th>Actions</th></tr></thead><tbody></tbody></table>

  <h3>Recent Sessions</h3>
  <table id=\"sessionsTable\"><thead><tr><th>Username</th><th>Expires</th><th>IP</th><th>Token</th><th>Actions</th></tr></thead><tbody></tbody></table>

  <h3>Active Bans</h3>
  <table id=\"bansTable\"><thead><tr><th>Username</th><th>Reason</th><th>Expires</th><th>Actions</th></tr></thead><tbody></tbody></table>

  <script>
    function appendCell(row, value) {
      const cell = document.createElement('td');
      cell.textContent = value == null ? '' : String(value);
      row.appendChild(cell);
      return cell;
    }

    async function apiPost(path, body) {
      const token = document.getElementById('token').value.trim();
      const res = await fetch(path, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/json'
        },
        body: JSON.stringify(body)
      });
      if (!res.ok) {
        alert('Action failed: ' + (await res.text()));
        return false;
      }
      return true;
    }

    async function revokeSession(token) {
      if (confirm('Revoke this session?')) {
        if (await apiPost('/admin/api/sessions/revoke', { token })) {
          loadOverview();
        }
      }
    }

    async function banUser(userId) {
      const reason = prompt('Reason for ban?');
      if (reason === null) return;
      const hours = prompt('Duration in hours? (leave blank for permanent)');
      const duration_hours = hours ? parseInt(hours) : null;
      if (await apiPost('/admin/api/ban', { user_id: userId, reason, duration_hours })) {
        loadOverview();
      }
    }

    async function unbanUser(userId) {
      if (confirm('Unban this user?')) {
        if (await apiPost('/admin/api/unban', { user_id: userId })) {
          loadOverview();
        }
      }
    }

    async function loadOverview() {
      const token = document.getElementById('token').value.trim();
      const res = await fetch('/admin/api/overview', {
        headers: { 'Authorization': `Bearer ${token}` }
      });
      if (!res.ok) {
        alert('Failed: ' + (await res.text()));
        return;
      }
      const data = await res.json();
      document.getElementById('users').textContent = data.users_total;
      document.getElementById('sessions').textContent = data.sessions_total;
      document.getElementById('active').textContent = data.sessions_active;

      const usersBody = document.querySelector('#usersTable tbody');
      usersBody.innerHTML = '';
      for (const u of data.recent_users) {
        const row = document.createElement('tr');
        appendCell(row, u.email);
        appendCell(row, u.username);
        appendCell(row, u.has_totp ? 'yes' : 'no');
        appendCell(row, u.created_at);
        const actionCell = document.createElement('td');
        const banBtn = document.createElement('button');
        banBtn.textContent = 'Ban';
        banBtn.style.padding = '4px 8px';
        banBtn.onclick = () => banUser(u.id);
        actionCell.appendChild(banBtn);
        row.appendChild(actionCell);
        usersBody.appendChild(row);
      }

      const sessionsBody = document.querySelector('#sessionsTable tbody');
      sessionsBody.innerHTML = '';
      for (const s of data.recent_sessions) {
        const row = document.createElement('tr');
        appendCell(row, s.username);
        appendCell(row, s.expires_at);
        appendCell(row, s.ip_address || '');
        appendCell(row, `${(s.token || '').slice(0, 12)}...`);
        const actionCell = document.createElement('td');
        const revokeBtn = document.createElement('button');
        revokeBtn.textContent = 'Revoke';
        revokeBtn.style.padding = '4px 8px';
        revokeBtn.onclick = () => revokeSession(s.token);
        actionCell.appendChild(revokeBtn);
        row.appendChild(actionCell);
        sessionsBody.appendChild(row);
      }

      const bansBody = document.querySelector('#bansTable tbody');
      bansBody.innerHTML = '';
      for (const b of data.active_bans) {
        const row = document.createElement('tr');
        appendCell(row, b.username);
        appendCell(row, b.reason);
        appendCell(row, b.expires_at || 'Permanent');
        const actionCell = document.createElement('td');
        const unbanBtn = document.createElement('button');
        unbanBtn.textContent = 'Unban';
        unbanBtn.style.padding = '4px 8px';
        unbanBtn.onclick = () => unbanUser(b.user_id);
        actionCell.appendChild(unbanBtn);
        row.appendChild(actionCell);
        bansBody.appendChild(row);
      }
    }

    document.getElementById('load').addEventListener('click', loadOverview);
  </script>
</body>
</html>
"#;
