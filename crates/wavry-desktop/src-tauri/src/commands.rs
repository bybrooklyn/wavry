use crate::auth::{
    get_or_create_identity, normalize_auth_server, parse_login_payload, signaling_ws_url_for_server,
};
use crate::client_manager::spawn_client_session;
use crate::secure_storage;
use crate::state::{AuthState, AUTH_STATE, CLIENT_SESSION_STATE, SESSION_STATE};
use std::net::SocketAddr;
use std::str::FromStr;
use wavry_client::{ClientConfig, FileTransferAction, FileTransferCommand};
use wavry_media::CapabilityProbe;

#[cfg(target_os = "macos")]
use wavry_media::MacProbe;
#[cfg(target_os = "windows")]
use wavry_media::WindowsProbe;
#[cfg(target_os = "linux")]
use wavry_media::{linux_runtime_diagnostics, LinuxProbe, PipewireAudioCapturer, PipewireEncoder};

#[cfg(target_os = "linux")]
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::Ordering;
#[cfg(target_os = "linux")]
use tokio::sync::{mpsc, oneshot};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Serialize)]
pub struct LinuxHostPreflight {
    pub requested_display_id: Option<u32>,
    pub selected_display_id: u32,
    pub selected_display_name: String,
    pub selected_resolution: wavry_media::Resolution,
    pub diagnostics: wavry_media::LinuxRuntimeDiagnostics,
}

#[cfg(target_os = "linux")]
fn sanitize_linux_capture_resolution(
    resolution: wavry_media::Resolution,
) -> wavry_media::Resolution {
    // H.264 paths generally expect even dimensions; clamp to safe non-zero values.
    let mut width = resolution.width.max(2);
    let mut height = resolution.height.max(2);
    if !width.is_multiple_of(2) {
        width = width.saturating_sub(1).max(2);
    }
    if !height.is_multiple_of(2) {
        height = height.saturating_sub(1).max(2);
    }
    wavry_media::Resolution { width, height }
}

#[cfg(target_os = "linux")]
fn select_linux_display(
    displays: &[wavry_media::DisplayInfo],
    requested_id: Option<u32>,
) -> Result<wavry_media::DisplayInfo, String> {
    if displays.is_empty() {
        return Err(
            "No Linux displays are available for host capture. On Wayland, ensure portal permission is granted."
                .to_string(),
        );
    }

    if let Some(requested_id) = requested_id {
        if let Some(display) = displays.iter().find(|display| display.id == requested_id) {
            return Ok(display.clone());
        }
        let fallback = displays[0].clone();
        log::warn!(
            "Requested Linux display id {} unavailable; falling back to id {} ({})",
            requested_id,
            fallback.id,
            fallback.name
        );
        return Ok(fallback);
    }

    Ok(displays[0].clone())
}

#[cfg(target_os = "linux")]
fn linux_host_preflight_impl(
    requested_display_id: Option<u32>,
) -> Result<LinuxHostPreflight, String> {
    let diagnostics = linux_runtime_diagnostics().map_err(|e| {
        format!(
            "Linux runtime diagnostics failed before host start: {}. Run ./scripts/linux-display-smoke.sh",
            e
        )
    })?;

    if !diagnostics.required_video_source_available {
        let missing = if diagnostics.missing_gstreamer_elements.is_empty() {
            "unknown".to_string()
        } else {
            diagnostics.missing_gstreamer_elements.join(", ")
        };
        return Err(format!(
            "Missing Linux video capture backend '{}' (missing elements: {}). {}",
            diagnostics.required_video_source,
            missing,
            diagnostics
                .recommendations
                .first()
                .cloned()
                .unwrap_or_else(
                    || "Install Linux desktop capture dependencies and retry.".to_string()
                )
        ));
    }

    if diagnostics.available_h264_encoders.is_empty() {
        return Err(
            "No Linux H264 encoders detected. Install x264enc/openh264enc or hardware encoders (VAAPI/NVENC/V4L2)."
                .to_string(),
        );
    }

    let probe = LinuxProbe;
    let displays = probe
        .enumerate_displays()
        .map_err(|e| format!("Linux display enumeration failed: {}", e))?;
    let selected_display = select_linux_display(&displays, requested_display_id)?;

    let selected_resolution = sanitize_linux_capture_resolution(selected_display.resolution);
    if selected_resolution != selected_display.resolution {
        log::warn!(
            "Adjusted Linux display resolution {}x{} -> {}x{} for encoder compatibility",
            selected_display.resolution.width,
            selected_display.resolution.height,
            selected_resolution.width,
            selected_resolution.height
        );
    }

    Ok(LinuxHostPreflight {
        requested_display_id,
        selected_display_id: selected_display.id,
        selected_display_name: selected_display.name,
        selected_resolution,
        diagnostics,
    })
}

#[tauri::command]
pub async fn set_cc_config(config: rift_core::cc::DeltaConfig) -> Result<(), String> {
    if let Ok(state) = SESSION_STATE.lock() {
        if let Some(ref s) = *state {
            if let Some(ref tx) = s.cc_config_tx {
                tx.send(config)
                    .map_err(|e: tokio::sync::mpsc::error::SendError<_>| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_cc_stats() -> Result<serde_json::Value, String> {
    if let Ok(state) = SESSION_STATE.lock() {
        if let Some(ref s) = *state {
            return Ok(json!({
                "bitrate_kbps": s.current_bitrate.load(Ordering::Relaxed),
                "state": s.cc_state.lock().unwrap().clone(),
            }));
        }
    }
    Err("No active session".into())
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn get_pcvr_status() -> String {
    wavry_client::pcvr_status()
}

#[tauri::command]
pub async fn register(
    app_handle: tauri::AppHandle,
    email: String,
    password: String,
    display_name: String,
    username: String,
    server: Option<String>,
) -> Result<String, String> {
    let identity = get_or_create_identity(&app_handle)?;
    let wavry_id = identity.wavry_id().to_string();
    let auth_server = normalize_auth_server(server);

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/auth/register", auth_server))
        .json(&json!({
            "email": email,
            "password": password,
            "display_name": display_name,
            "username": username,
            "public_key": wavry_id
        }))
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    if res.status().is_success() {
        Ok("Registration successful. Please login.".into())
    } else {
        let body: serde_json::Value = res.json().await.unwrap_or_default();
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Registration failed");
        Err(err.to_string())
    }
}

#[tauri::command]
pub async fn login_full(
    app_handle: tauri::AppHandle,
    email: String,
    password: String,
    server: Option<String>,
) -> Result<serde_json::Value, String> {
    let identity = get_or_create_identity(&app_handle)?;
    let client = reqwest::Client::new();
    let auth_server = normalize_auth_server(server);
    let signaling_url = signaling_ws_url_for_server(&auth_server);

    let challenge_res = client
        .post(format!("{}/auth/challenge", auth_server))
        .json(&json!({
            "email": email,
            "wavry_id": identity.wavry_id().to_string()
        }))
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    let login_payload = if challenge_res.status().is_success() {
        let challenge_resp: serde_json::Value = challenge_res
            .json()
            .await
            .map_err(|e: reqwest::Error| e.to_string())?;
        let challenge_hex = challenge_resp["challenge"]
            .as_str()
            .ok_or("Missing challenge")?;
        let challenge = hex::decode(challenge_hex).map_err(|e: hex::FromHexError| e.to_string())?;
        let signature = identity.sign(&challenge);
        let signature_hex = hex::encode(signature);
        json!({
            "email": email,
            "password": password,
            "signature": signature_hex
        })
    } else {
        json!({
            "email": email,
            "password": password
        })
    };

    let res = client
        .post(format!("{}/auth/login", auth_server))
        .json(&login_payload)
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    if res.status().is_success() {
        let payload: serde_json::Value = res
            .json()
            .await
            .map_err(|e: reqwest::Error| e.to_string())?;
        let (username, token) = parse_login_payload(payload)?;

        // Save token and username securely
        let _ = secure_storage::save_token(&token);
        let _ = secure_storage::save_data("username", &username);

        let mut auth = AUTH_STATE.lock().unwrap();
        *auth = Some(AuthState {
            token: token.clone(),
            signaling_url: signaling_url.clone(),
        });
        Ok(json!({
            "username": username,
            "token": token,
            "signaling_url": signaling_url
        }))
    } else {
        let body: serde_json::Value = res.json().await.unwrap_or_default();
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Login failed");
        Err(err.to_string())
    }
}

#[tauri::command]
pub async fn set_signaling_token(
    token: Option<String>,
    server: Option<String>,
) -> Result<(), String> {
    let mut auth = AUTH_STATE.lock().unwrap();
    if let Some(t) = token {
        let _ = secure_storage::save_token(&t);
        let auth_server = normalize_auth_server(server);
        *auth = Some(AuthState {
            token: t,
            signaling_url: signaling_ws_url_for_server(&auth_server),
        });
        log::info!("Signaling token re-hydrated from frontend");
    } else {
        let _ = secure_storage::delete_token();
        let _ = secure_storage::delete_data("username");
        *auth = None;
    }
    Ok(())
}

#[tauri::command]
pub fn save_secure_token(token: String) -> Result<(), String> {
    secure_storage::save_token(&token)
}

#[tauri::command]
pub fn load_secure_token() -> Result<Option<String>, String> {
    secure_storage::get_token()
}

#[tauri::command]
pub fn delete_secure_token() -> Result<(), String> {
    secure_storage::delete_token()
}

#[tauri::command]
pub fn save_secure_data(key: String, value: String) -> Result<(), String> {
    secure_storage::save_data(&key, &value)
}

#[tauri::command]
pub fn load_secure_data(key: String) -> Result<Option<String>, String> {
    secure_storage::get_data(&key)
}

#[tauri::command]
pub fn delete_secure_data(key: String) -> Result<(), String> {
    secure_storage::delete_data(&key)
}

#[tauri::command]
pub async fn start_session(
    addr: String,
    resolution_mode: String,
    width: Option<u32>,
    height: Option<u32>,
    gamepad_enabled: Option<bool>,
    gamepad_deadzone: Option<f32>,
) -> Result<String, String> {
    let socket_addr = if let Ok(s) = SocketAddr::from_str(&addr) {
        Some(s)
    } else if addr.is_empty() {
        None
    } else {
        return Err("Invalid IP address".into());
    };

    let max_resolution = match resolution_mode.as_str() {
        "native" => None,
        "client" | "custom" => {
            if let (Some(w), Some(h)) = (width, height) {
                Some(wavry_media::Resolution {
                    width: w as u16,
                    height: h as u16,
                })
            } else {
                None
            }
        }
        _ => None,
    };

    let config = ClientConfig {
        connect_addr: socket_addr,
        client_name: "wavry-desktop".to_string(),
        no_encrypt: false,
        identity_key: None,
        relay_info: None,
        master_url: None, // Direct IP sessions don't usually need master feedback
        max_resolution,
        gamepad_enabled: gamepad_enabled.unwrap_or(true),
        gamepad_deadzone: gamepad_deadzone.unwrap_or(0.1).clamp(0.0, 0.95),
        vr_adapter: None,
        runtime_stats: None,
        recorder_config: None,
        send_files: Vec::new(),
        file_out_dir: std::path::PathBuf::from("received-files"),
        file_max_bytes: 1_073_741_824,
        file_command_bus: None,
    };

    spawn_client_session(config)?;

    Ok("Session started".into())
}

#[tauri::command]
pub async fn stop_session() -> Result<String, String> {
    let stop_tx = {
        let mut state = CLIENT_SESSION_STATE.lock().unwrap();
        state.as_mut().and_then(|s| s.stop_tx.take())
    };

    if let Some(tx) = stop_tx {
        let _ = tx.send(());
        Ok("Stopping client session".into())
    } else {
        Err("No active client session".into())
    }
}

#[tauri::command]
pub fn send_file_transfer_command(file_id: u64, action: String) -> Result<String, String> {
    let action = action
        .parse::<FileTransferAction>()
        .map_err(|e| e.to_string())?;
    let tx = {
        let state = CLIENT_SESSION_STATE.lock().unwrap();
        state.as_ref().and_then(|s| s.file_command_tx.clone())
    };

    let Some(tx) = tx else {
        return Err("No active client session".into());
    };

    tx.send(FileTransferCommand { file_id, action })
        .map_err(|e| format!("failed to enqueue file transfer command: {}", e))?;

    Ok(format!(
        "queued file transfer command: file_id={} action={}",
        file_id, action
    ))
}

#[tauri::command]
pub async fn stop_host() -> Result<String, String> {
    let stop_tx = {
        let mut state = SESSION_STATE.lock().unwrap();
        state.as_mut().and_then(|s| s.stop_tx.take())
    };

    if let Some(tx) = stop_tx {
        let _ = tx.send(());
        Ok("Stopping host".into())
    } else {
        Err("No active host session".into())
    }
}

#[tauri::command]
pub async fn list_monitors() -> Result<Vec<wavry_media::DisplayInfo>, String> {
    #[cfg(target_os = "macos")]
    let probe = MacProbe;
    #[cfg(target_os = "windows")]
    let probe = WindowsProbe;
    #[cfg(target_os = "linux")]
    let probe = LinuxProbe;

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    probe
        .enumerate_displays()
        .map_err(|e: anyhow::Error| e.to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn linux_runtime_health() -> Result<wavry_media::LinuxRuntimeDiagnostics, String> {
    linux_runtime_diagnostics().map_err(|e: anyhow::Error| e.to_string())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn linux_runtime_health() -> Result<serde_json::Value, String> {
    Err("Linux runtime health is only available on Linux builds".to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn linux_host_preflight(display_id: Option<u32>) -> Result<LinuxHostPreflight, String> {
    linux_host_preflight_impl(display_id)
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn linux_host_preflight(_display_id: Option<u32>) -> Result<serde_json::Value, String> {
    Err("Linux host preflight is only available on Linux builds".to_string())
}

#[tauri::command]
pub async fn connect_via_id(target_username: String) -> Result<String, String> {
    use wavry_client::signaling::{SignalMessage, SignalingClient};

    let (token, signaling_url) = {
        let auth = AUTH_STATE.lock().unwrap();
        if let Some(ref a) = *auth {
            (a.token.clone(), a.signaling_url.clone())
        } else {
            return Err("Not logged in".into());
        }
    };

    log::info!("Connecting to {} via signaling", target_username);

    let mut sig = SignalingClient::connect(&signaling_url, &token)
        .await
        .map_err(|e: anyhow::Error| format!("Signaling error: {}", e))?;

    let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    let public_addr = if let Some(ref s) = udp {
        let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
        if let Some(tu) = tokio_u {
            wavry_client::discover_public_addr(&tu)
                .await
                .ok()
                .map(|a: SocketAddr| a.to_string())
        } else {
            None
        }
    } else {
        None
    };

    log::info!("Discovered public addr: {:?}", public_addr);

    let hello_b64 = wavry_client::create_hello_base64("wavry-desktop".into(), public_addr)
        .map_err(|e: anyhow::Error| e.to_string())?;
    sig.send(SignalMessage::OFFER_RIFT {
        target_username: target_username.clone(),
        hello_base64: hello_b64,
    })
    .await
    .map_err(|e: anyhow::Error| e.to_string())?;

    let wait_target = target_username.clone();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut relay_info: Option<wavry_client::RelayInfo> = None;

        loop {
            match sig.recv().await {
                Ok(SignalMessage::ANSWER_RIFT { ack_base64, .. }) => {
                    let ack = wavry_client::decode_hello_ack_base64(&ack_base64)
                        .map_err(|e: anyhow::Error| e.to_string())?;
                    log::info!(
                        "Received RIFT answer from {}: accepted={}",
                        target_username,
                        ack.accepted
                    );

                    if !ack.accepted {
                        return Err("Connection rejected by host".into());
                    }

                    let connect_addr = if !ack.public_addr.is_empty() {
                        ack.public_addr.parse::<std::net::SocketAddr>().ok()
                    } else {
                        None
                    };

                    if connect_addr.is_none() && relay_info.is_none() {
                        log::info!(
                            "Host {} did not provide direct endpoint; requesting relay fallback",
                            target_username
                        );
                        sig.send(SignalMessage::REQUEST_RELAY {
                            target_username: target_username.clone(),
                            region: None,
                        })
                        .await
                        .map_err(|e: anyhow::Error| format!("Failed to request relay: {}", e))?;

                        let relay =
                            tokio::time::timeout(std::time::Duration::from_secs(8), async {
                                loop {
                                    match sig.recv().await {
                                        Ok(SignalMessage::RELAY_CREDENTIALS {
                                            relay_id,
                                            token,
                                            addr,
                                            session_id,
                                        }) => {
                                            let relay_addr = addr
                                                .parse::<std::net::SocketAddr>()
                                                .map_err(|_| {
                                                "Relay credentials contained invalid relay address"
                                                    .to_string()
                                            })?;
                                            break Ok(wavry_client::RelayInfo {
                                                relay_id,
                                                addr: relay_addr,
                                                token,
                                                session_id,
                                            });
                                        }
                                        Ok(SignalMessage::ERROR { message, .. }) => {
                                            break Err(message)
                                        }
                                        Ok(_) => continue,
                                        Err(e) => break Err(e.to_string()),
                                    }
                                }
                            })
                            .await
                            .map_err(|_| "Timed out waiting for relay credentials".to_string())??;

                        relay_info = Some(relay);
                    }

                    if connect_addr.is_none() && relay_info.is_none() {
                        return Err(
                            "Host acknowledged request but no direct or relay route was provided"
                                .into(),
                        );
                    }

                    let master_url = if signaling_url.contains("/ws") {
                        Some(signaling_url.replace("/ws", ""))
                    } else {
                        None
                    };

                    let config = wavry_client::ClientConfig {
                        connect_addr,
                        client_name: "wavry-desktop".into(),
                        no_encrypt: false,
                        identity_key: None,
                        relay_info,
                        master_url,
                        max_resolution: None,
                        gamepad_enabled: true,
                        gamepad_deadzone: 0.1,
                        vr_adapter: None,
                        runtime_stats: None,
                        recorder_config: None,
                        send_files: Vec::new(),
                        file_out_dir: std::path::PathBuf::from("received-files"),
                        file_max_bytes: 1_073_741_824,
                        file_command_bus: None,
                    };

                    spawn_client_session(config)?;

                    return Ok("Connected".into());
                }
                Ok(SignalMessage::RELAY_CREDENTIALS {
                    relay_id,
                    token,
                    addr,
                    session_id,
                }) => {
                    log::info!("Received relay credentials: {} (id={})", addr, relay_id);
                    if let Ok(relay_addr) = addr.parse::<std::net::SocketAddr>() {
                        relay_info = Some(wavry_client::RelayInfo {
                            relay_id,
                            addr: relay_addr,
                            token,
                            session_id,
                        });
                    }
                }
                Ok(SignalMessage::ERROR { message, .. }) => return Err(message),
                Ok(_) => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
    })
    .await
    .map_err(|_| format!("Timed out waiting for {} to respond", wait_target))?
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn start_host(
    app_handle: tauri::AppHandle,
    port: u16,
    display_id: Option<u32>,
) -> Result<String, String> {
    use crate::media_utils::choose_rift_codec;
    use crate::state::SessionState;
    use bytes::Bytes;
    use std::net::UdpSocket;
    use std::sync::atomic::AtomicU32;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use wavry_client::signaling::{SignalMessage, SignalingClient};
    use wavry_media::{Codec, EncodeConfig, MediaError};

    {
        let state = SESSION_STATE.lock().unwrap();
        if state.is_some() {
            return Err("Already hosting".into());
        }
    }

    let preflight = linux_host_preflight_impl(display_id)?;

    log::info!(
        "Linux host capture using display id {} '{}' at {}x{}",
        preflight.selected_display_id,
        preflight.selected_display_name,
        preflight.selected_resolution.width,
        preflight.selected_resolution.height
    );

    let (cc_tx, mut cc_rx) = mpsc::unbounded_channel::<rift_core::cc::DeltaConfig>();
    let current_bitrate = Arc::new(AtomicU32::new(8000));
    let cc_state_shared = Arc::new(Mutex::new("Stable".to_string()));

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    {
        let mut state = SESSION_STATE.lock().unwrap();
        *state = Some(SessionState {
            stop_tx: Some(stop_tx),
            cc_config_tx: Some(cc_tx),
            current_bitrate: current_bitrate.clone(),
            cc_state: cc_state_shared.clone(),
        });
    }

    let config = EncodeConfig {
        codec: Codec::H264,
        resolution: preflight.selected_resolution,
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        display_id: Some(preflight.selected_display_id),
        enable_10bit: false,
        enable_hdr: false,
    };

    let mut signaling_token: Option<String> = None;
    let mut signaling_url = "wss://auth.wavry.dev/ws".to_string();
    {
        let auth = AUTH_STATE.lock().unwrap();
        if let Some(ref a) = *auth {
            signaling_token = Some(a.token.clone());
            signaling_url = a.signaling_url.clone();
        }
    }

    let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
        Ok(socket) => socket,
        Err(e) => {
            if let Ok(mut state) = SESSION_STATE.lock() {
                *state = None;
            }
            return Err(format!("Failed to bind UDP socket: {}", e));
        }
    };
    socket.set_nonblocking(true).ok();
    let bound_port = socket.local_addr().map(|addr| addr.port()).unwrap_or(port);

    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        let app_handle = app_handle_clone;
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 10;
        let mut last_error_time = std::time::Instant::now();

        log::info!(
            "Host task started (requested port {}, bound port {})",
            port,
            bound_port
        );

        let shared_client_addr = Arc::new(std::sync::Mutex::new(None));
        let (audio_stop_tx, mut audio_stop_rx) = oneshot::channel::<()>();

        if let Some(token) = signaling_token {
            let signaling_url = signaling_url.clone();
            tokio::spawn(async move {
                if let Ok(mut sig) = SignalingClient::connect(&signaling_url, &token).await {
                    log::info!("Host registered with signaling gateway");
                    while let Ok(msg) = sig.recv().await {
                        if let SignalMessage::OFFER_RIFT {
                            target_username,
                            hello_base64,
                        } = msg
                        {
                            if let Ok(hello) = wavry_client::decode_hello_base64(&hello_base64) {
                                let session_id = uuid::Uuid::new_v4().into_bytes();
                                let session_alias = 1;

                                let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
                                let my_public_addr = if let Some(ref s) = udp {
                                    let tokio_u =
                                        tokio::net::UdpSocket::from_std(s.try_clone().unwrap())
                                            .ok();
                                    if let Some(tu) = tokio_u {
                                        wavry_client::discover_public_addr(&tu)
                                            .await
                                            .ok()
                                            .map(|a: SocketAddr| a.to_string())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                                let (w, h) = if let Some(res) = hello.max_resolution {
                                    (res.width, res.height)
                                } else {
                                    (1920, 1080)
                                };

                                let selected_codec = choose_rift_codec(&hello);
                                let ack_b64 = wavry_client::create_hello_ack_base64(
                                    true,
                                    session_id,
                                    session_alias,
                                    my_public_addr,
                                    w,
                                    h,
                                    selected_codec,
                                )
                                .unwrap_or_default();

                                let _ = sig
                                    .send(SignalMessage::ANSWER_RIFT {
                                        target_username,
                                        ack_base64: ack_b64,
                                    })
                                    .await;
                            }
                        }
                    }
                }
            });
        }

        'outer: loop {
            let mut video_encoder = match PipewireEncoder::new(config.clone()).await {
                Ok(e) => {
                    retry_count = 0;
                    e
                }
                Err(e) => {
                    log::error!("Failed to initialize video encoder: {}", e);
                    let can_retry = retry_count < MAX_RETRIES;

                    #[derive(Clone, serde::Serialize)]
                    struct HostErrorEvent {
                        error_type: String,
                        message: String,
                        can_retry: bool,
                    }

                    let error_type = match &e {
                        MediaError::ProtocolViolation(_) => "ProtocolViolation",
                        MediaError::PortalUnavailable(_) => "PortalUnavailable",
                        MediaError::StreamNodeLoss(_) => "StreamNodeLoss",
                        MediaError::CompositorDisconnect(_) => "CompositorDisconnect",
                        _ => "Other",
                    };

                    let _ = tauri::Emitter::emit(
                        &app_handle,
                        "host-error",
                        HostErrorEvent {
                            error_type: error_type.to_string(),
                            message: e.to_string(),
                            can_retry,
                        },
                    );

                    if !can_retry {
                        break 'outer;
                    }

                    retry_count += 1;
                    let delay = std::time::Duration::from_millis(1000 * (1 << retry_count).min(30));
                    log::info!("Retrying video encoder initialization in {:?}", delay);

                    tokio::select! {
                        _ = tokio::time::sleep(delay) => continue 'outer,
                        _ = &mut stop_rx => break 'outer,
                    }
                }
            };

            let mut audio_capturer = match PipewireAudioCapturer::new().await {
                Ok(a) => a,
                Err(e) => {
                    log::error!("Failed to initialize audio capturer: {}", e);
                    // Audio failure might be non-fatal but let's report it
                    #[derive(Clone, serde::Serialize)]
                    struct HostErrorEvent {
                        error_type: String,
                        message: String,
                        can_retry: bool,
                    }
                    let _ = tauri::Emitter::emit(
                        &app_handle,
                        "host-error",
                        HostErrorEvent {
                            error_type: "AudioCaptureFailure".to_string(),
                            message: e.to_string(),
                            can_retry: true,
                        },
                    );
                    // Fallback or just continue without audio
                    // For now, we'll try to continue.
                    // We need a dummy or option for audio_capturer.
                    // Let's just break for now to avoid complexity in this step.
                    break 'outer;
                }
            };

            let socket_clone = socket.try_clone().expect("Failed to clone socket");
            let shared_client_addr_audio = shared_client_addr.clone();

            // Audio loop in a separate task
            let mut audio_task_stop_rx = audio_stop_rx;
            let audio_handle = tokio::spawn(async move {
                let mut packet_id_counter: u64 = 1;
                let mut audio_capturer = audio_capturer;
                loop {
                    if audio_task_stop_rx.try_recv().is_ok() {
                        break;
                    }

                    // PipewireAudioCapturer::next_packet is blocking, so we use spawn_blocking if needed,
                    // but since this is a dedicated task it's okay for now if we don't have many sessions.
                    // Better: use a thread for the blocking parts.
                    match audio_capturer.next_packet() {
                        Ok(frame) => {
                            let addr = {
                                let addr_lock = shared_client_addr_audio.lock().unwrap();
                                *addr_lock
                            };

                            if let Some(addr) = addr {
                                let audio = rift_core::AudioPacket {
                                    timestamp_us: frame.timestamp_us,
                                    payload: frame.data,
                                };

                                let msg = rift_core::Message {
                                    content: Some(rift_core::message::Content::Media(
                                        rift_core::MediaMessage {
                                            content: Some(
                                                rift_core::media_message::Content::Audio(audio),
                                            ),
                                        },
                                    )),
                                };

                                let phys = rift_core::PhysicalPacket {
                                    version: rift_core::RIFT_VERSION,
                                    session_id: None,
                                    session_alias: None,
                                    packet_id: {
                                        let id = packet_id_counter;
                                        packet_id_counter += 1;
                                        id
                                    },
                                    payload: Bytes::from(rift_core::encode_msg(&msg)),
                                };

                                let _ = socket_clone.send_to(&phys.encode(), addr);
                            }
                        }
                        Err(e) => {
                            log::error!("Audio capture error: {}", e);
                            break;
                        }
                    }
                }
                log::info!("Audio task exiting");
            });

            let mut sequence: u64 = 0;
            let mut packet_id_counter: u64 = 1;
            let mut delta_cc = rift_core::cc::DeltaCC::new(
                rift_core::cc::DeltaConfig::default(),
                config.bitrate_kbps,
                config.fps as u32,
            );
            let mut fec_builder = rift_core::FecBuilder::new(20).unwrap();
            let mut last_fec_ratio = 0.05f32;

            loop {
                if stop_rx.try_recv().is_ok() {
                    let _ = audio_stop_tx.send(());
                    break 'outer;
                }

                if let Ok(new_config) = cc_rx.try_recv() {
                    delta_cc = rift_core::cc::DeltaCC::new(
                        new_config,
                        delta_cc.target_bitrate_kbps(),
                        delta_cc.target_fps(),
                    );
                }

                let mut buf = [0u8; 2048];
                if let Ok((len, src)) = socket.recv_from(&mut buf) {
                    let mut addr_lock = shared_client_addr.lock().unwrap();
                    if addr_lock.is_none() {
                        log::info!("Client connected from {}", src);
                        *addr_lock = Some(src);
                    }

                    if let Ok(phys) =
                        rift_core::PhysicalPacket::decode(Bytes::copy_from_slice(&buf[..len]))
                    {
                        if let Ok(msg) = rift_core::decode_msg(&phys.payload) {
                            if let Some(rift_core::message::Content::Control(ctrl)) = msg.content {
                                if let Some(rift_core::control_message::Content::Stats(stats)) =
                                    ctrl.content
                                {
                                    let loss = if stats.received_packets > 0 {
                                        stats.lost_packets as f32
                                            / (stats.received_packets + stats.lost_packets) as f32
                                    } else {
                                        0.0
                                    };
                                    delta_cc.on_rtt_sample(stats.rtt_us, loss, stats.jitter_us);

                                    let new_bitrate = delta_cc.target_bitrate_kbps();
                                    if let Err(e) = video_encoder.set_bitrate(new_bitrate) {
                                        log::error!("Failed to update bitrate: {}", e);
                                    }

                                    current_bitrate.store(new_bitrate, Ordering::Relaxed);
                                    let state_str = format!("{:?}", delta_cc.state());
                                    *cc_state_shared.lock().unwrap() = state_str;
                                }
                            }
                        }
                    }
                }

                match video_encoder.next_frame() {
                    Ok(frame) => {
                        let addr = {
                            let addr_lock = shared_client_addr.lock().unwrap();
                            *addr_lock
                        };

                        if let Some(addr) = addr {
                            let max_payload = 1300;
                            let data = frame.data;
                            let total_chunks = data.len().div_ceil(max_payload) as u32;
                            let frame_id = sequence;

                            for i in 0..total_chunks {
                                let start = (i as usize) * max_payload;
                                let end = std::cmp::min(start + max_payload, data.len());
                                let chunk_data = data[start..end].to_vec();

                                let chunk = rift_core::VideoChunk {
                                    frame_id,
                                    chunk_index: i,
                                    chunk_count: total_chunks,
                                    timestamp_us: frame.timestamp_us,
                                    keyframe: frame.keyframe,
                                    payload: chunk_data,
                                };

                                let msg = rift_core::Message {
                                    content: Some(rift_core::message::Content::Media(
                                        rift_core::MediaMessage {
                                            content: Some(
                                                rift_core::media_message::Content::Video(chunk),
                                            ),
                                        },
                                    )),
                                };

                                let phys = rift_core::PhysicalPacket {
                                    version: rift_core::RIFT_VERSION,
                                    session_id: None,
                                    session_alias: None,
                                    packet_id: {
                                        let id = packet_id_counter;
                                        packet_id_counter += 1;
                                        id
                                    },
                                    payload: Bytes::from(rift_core::encode_msg(&msg)),
                                };

                                let _ = socket.send_to(&phys.encode(), addr);
                                if let Some(fec) =
                                    fec_builder.push(packet_id_counter - 1, &phys.payload)
                                {
                                    let fec_msg = rift_core::Message {
                                        content: Some(rift_core::message::Content::Media(
                                            rift_core::MediaMessage {
                                                content: Some(
                                                    rift_core::media_message::Content::Fec(fec),
                                                ),
                                            },
                                        )),
                                    };
                                    let fec_phys = rift_core::PhysicalPacket {
                                        version: rift_core::RIFT_VERSION,
                                        session_id: None,
                                        session_alias: None,
                                        packet_id: 0,
                                        payload: bytes::Bytes::from(rift_core::encode_msg(
                                            &fec_msg,
                                        )),
                                    };
                                    let _ = socket.send_to(&fec_phys.encode(), addr);
                                }
                            }
                            sequence = sequence.wrapping_add(1);

                            let current_fec = delta_cc.fec_ratio();
                            if (current_fec - last_fec_ratio).abs() > 0.01 {
                                let shards = (1.0 / current_fec).clamp(4.0, 30.0) as u32;
                                if let Ok(new_fec) = rift_core::FecBuilder::new(shards) {
                                    fec_builder = new_fec;
                                    last_fec_ratio = current_fec;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Video capture error: {}", e);

                        // If it's a protocol error or compositor disconnect, we might want to retry
                        let can_retry = match e {
                            MediaError::ProtocolViolation(_)
                            | MediaError::CompositorDisconnect(_)
                            | MediaError::StreamNodeLoss(_) => true,
                            _ => false,
                        };

                        #[derive(Clone, serde::Serialize)]
                        struct HostErrorEvent {
                            error_type: String,
                            message: String,
                            can_retry: bool,
                        }

                        let error_type = match &e {
                            MediaError::ProtocolViolation(_) => "ProtocolViolation",
                            MediaError::PortalUnavailable(_) => "PortalUnavailable",
                            MediaError::StreamNodeLoss(_) => "StreamNodeLoss",
                            MediaError::CompositorDisconnect(_) => "CompositorDisconnect",
                            _ => "Other",
                        };

                        let _ = tauri::Emitter::emit(
                            &app_handle,
                            "host-error",
                            HostErrorEvent {
                                error_type: error_type.to_string(),
                                message: e.to_string(),
                                can_retry,
                            },
                        );

                        if can_retry && retry_count < MAX_RETRIES {
                            retry_count += 1;
                            // Cool down before retry
                            let delay = std::time::Duration::from_millis(2000);
                            log::info!("Retrying capture in {:?}", delay);
                            tokio::time::sleep(delay).await;
                            continue 'outer;
                        } else {
                            break 'outer;
                        }
                    }
                }
            }
        }

        if let Ok(mut state) = SESSION_STATE.lock() {
            *state = None;
        }
    });

    Ok(format!("Hosting on UDP {}", bound_port))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tauri::command]
pub async fn start_host(_port: u16, _display_id: Option<u32>) -> Result<String, String> {
    Err("Host not fully implemented for this platform in refactored version yet".into())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{sanitize_linux_capture_resolution, select_linux_display};

    fn display(id: u32, name: &str, width: u16, height: u16) -> wavry_media::DisplayInfo {
        wavry_media::DisplayInfo {
            id,
            name: name.to_string(),
            resolution: wavry_media::Resolution { width, height },
        }
    }

    #[test]
    fn sanitize_linux_capture_resolution_evenizes_and_clamps() {
        let sanitized = sanitize_linux_capture_resolution(wavry_media::Resolution {
            width: 1,
            height: 719,
        });
        assert_eq!(sanitized.width, 2);
        assert_eq!(sanitized.height, 718);
    }

    #[test]
    fn select_linux_display_prefers_requested_when_present() {
        let displays = vec![
            display(0, "Primary", 1920, 1080),
            display(2, "Secondary", 2560, 1440),
        ];
        let selected =
            select_linux_display(&displays, Some(2)).expect("display should be selected");
        assert_eq!(selected.id, 2);
        assert_eq!(selected.name, "Secondary");
    }

    #[test]
    fn select_linux_display_falls_back_to_first_when_missing() {
        let displays = vec![
            display(0, "Primary", 1920, 1080),
            display(1, "Secondary", 2560, 1440),
        ];
        let selected = select_linux_display(&displays, Some(99)).expect("fallback should succeed");
        assert_eq!(selected.id, 0);
        assert_eq!(selected.name, "Primary");
    }
}
