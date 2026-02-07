use tauri::Manager;
use crate::state::{AUTH_STATE, AuthState, IDENTITY_KEY};
use rift_crypto::identity::IdentityKeypair;

pub fn get_or_create_identity(
    app_handle: &tauri::AppHandle,
) -> Result<IdentityKeypair, String> {
    let mut id_lock = IDENTITY_KEY.lock().unwrap();
    if let Some(ref id) = *id_lock {
        return Ok(IdentityKeypair::from_bytes(
            &id.private_key_bytes(),
        ));
    }

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
    let key_path = app_dir.join("identity.key");

    if key_path.exists() {
        let id = IdentityKeypair::load(key_path.to_str().unwrap())
            .map_err(|e| format!("Failed to load identity: {}", e))?;
        *id_lock = Some(IdentityKeypair::from_bytes(
            &id.private_key_bytes(),
        ));
        Ok(id)
    } else {
        let id = IdentityKeypair::generate();
        id.save(
            key_path.to_str().unwrap(),
            app_dir.join("identity.pub").to_str().unwrap(),
        )
        .map_err(|e| format!("Failed to save identity: {}", e))?;
        *id_lock = Some(IdentityKeypair::from_bytes(
            &id.private_key_bytes(),
        ));
        Ok(id)
    }
}

pub fn normalize_auth_server(server: Option<String>) -> String {
    server
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://auth.wavry.dev".to_string())
}

pub fn signaling_ws_url_for_server(server: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(server) {
        let scheme = match url.scheme() {
            "ws" | "wss" => url.scheme().to_string(),
            "http" => "ws".to_string(),
            "https" => "wss".to_string(),
            _ => "wss".to_string(),
        };
        let host = url.host_str().unwrap_or("auth.wavry.dev");
        let port_part = url.port().map(|p| format!(":{p}")).unwrap_or_default();
        return format!("{scheme}://{host}{port_part}/ws");
    }
    "wss://auth.wavry.dev/ws".to_string()
}

pub fn parse_login_payload(value: serde_json::Value) -> Result<(String, String), String> {
    let token = value
        .get("token")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("session")
                .and_then(|v| v.get("token"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| "Login response missing session token".to_string())?;

    let username = value
        .get("username")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("user")
                .and_then(|v| v.get("username"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| "Login response missing username".to_string())?;

    Ok((username, token))
}

pub fn set_signaling_state(token: String, server: String) {
    let signaling_url = signaling_ws_url_for_server(&server);
    let mut auth = AUTH_STATE.lock().unwrap();
    *auth = Some(AuthState {
        token,
        signaling_url,
    });
}
