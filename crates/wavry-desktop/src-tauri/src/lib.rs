#[cfg(target_os = "linux")]
use rift_core::Codec as RiftCodec;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use wavry_client::{run_client_with_shutdown, ClientConfig};
use wavry_media::{CapabilityProbe, Codec, EncodeConfig, Resolution};

#[cfg(target_os = "linux")]
use wavry_media::{LinuxProbe, PipewireAudioCapturer, PipewireEncoder};
#[cfg(target_os = "macos")]
use wavry_media::{MacAudioCapturer, MacProbe, MacScreenEncoder};
#[cfg(target_os = "windows")]
use wavry_media::{WindowsAudioCapturer, WindowsEncoder, WindowsProbe};

#[cfg(target_os = "linux")]
fn local_supported_encoders() -> Vec<Codec> {
    #[cfg(target_os = "windows")]
    {
        return WindowsProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "macos")]
    {
        MacProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264])
    }
    #[cfg(target_os = "linux")]
    {
        return wavry_media::LinuxProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        vec![Codec::H264]
    }
}

#[cfg(target_os = "linux")]
fn choose_rift_codec(hello: &rift_core::Hello) -> RiftCodec {
    let local_supported = local_supported_encoders();

    let remote_supported: Vec<RiftCodec> = hello
        .supported_codecs
        .iter()
        .filter_map(|c| RiftCodec::try_from(*c).ok())
        .collect();

    let supports = |codec: RiftCodec| {
        let local_ok = match codec {
            RiftCodec::Av1 => local_supported.contains(&Codec::Av1),
            RiftCodec::Hevc => local_supported.contains(&Codec::Hevc),
            RiftCodec::H264 => local_supported.contains(&Codec::H264),
        };
        let remote_ok = remote_supported.contains(&codec);
        local_ok && remote_ok
    };

    if supports(RiftCodec::Av1) {
        RiftCodec::Av1
    } else if supports(RiftCodec::Hevc) {
        RiftCodec::Hevc
    } else {
        RiftCodec::H264
    }
}

/// Global session state for the desktop app
struct SessionState {
    stop_tx: Option<oneshot::Sender<()>>,
    cc_config_tx: Option<mpsc::UnboundedSender<rift_core::cc::DeltaConfig>>,
    current_bitrate: Arc<AtomicU32>,
    cc_state: Arc<Mutex<String>>,
}

struct ClientSessionState {
    stop_tx: Option<oneshot::Sender<()>>,
}

static SESSION_STATE: Mutex<Option<SessionState>> = Mutex::new(None);
static CLIENT_SESSION_STATE: Mutex<Option<ClientSessionState>> = Mutex::new(None);
static AUTH_STATE: Mutex<Option<AuthState>> = Mutex::new(None);
static IDENTITY_KEY: Mutex<Option<rift_crypto::IdentityKeypair>> = Mutex::new(None);

struct AuthState {
    token: String,
}

fn register_client_session(stop_tx: oneshot::Sender<()>) -> Result<(), String> {
    let mut state = CLIENT_SESSION_STATE.lock().unwrap();
    if state.is_some() {
        return Err("Client session already active".into());
    }
    *state = Some(ClientSessionState {
        stop_tx: Some(stop_tx),
    });
    Ok(())
}

fn clear_client_session() {
    if let Ok(mut state) = CLIENT_SESSION_STATE.lock() {
        *state = None;
    }
}

fn spawn_client_session(config: ClientConfig) -> Result<(), String> {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    register_client_session(stop_tx)?;

    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_client_with_shutdown(config, None, stop_rx).await {
            log::error!("Client error: {}", e);
        }
        clear_client_session();
    });

    Ok(())
}

fn get_or_create_identity(
    app_handle: &tauri::AppHandle,
) -> Result<rift_crypto::IdentityKeypair, String> {
    use tauri::Manager;

    let mut id_lock = IDENTITY_KEY.lock().unwrap();
    if let Some(ref id) = *id_lock {
        return Ok(rift_crypto::IdentityKeypair::from_bytes(
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
        let id = rift_crypto::IdentityKeypair::load(key_path.to_str().unwrap())
            .map_err(|e| format!("Failed to load identity: {}", e))?;
        *id_lock = Some(rift_crypto::IdentityKeypair::from_bytes(
            &id.private_key_bytes(),
        ));
        Ok(id)
    } else {
        let id = rift_crypto::IdentityKeypair::generate();
        id.save(
            key_path.to_str().unwrap(),
            app_dir.join("identity.pub").to_str().unwrap(),
        )
        .map_err(|e| format!("Failed to save identity: {}", e))?;
        *id_lock = Some(rift_crypto::IdentityKeypair::from_bytes(
            &id.private_key_bytes(),
        ));
        Ok(id)
    }
}

#[derive(Serialize, Deserialize)]
struct LoginResponse {
    user: User,
    session: Session,
}

#[derive(Serialize, Deserialize)]
struct User {
    id: i32,
    username: String,
    email: String,
}

#[derive(Serialize, Deserialize)]
struct Session {
    token: String,
}

#[tauri::command]
async fn set_cc_config(config: rift_core::cc::DeltaConfig) -> Result<(), String> {
    if let Ok(state) = SESSION_STATE.lock() {
        if let Some(ref s) = *state {
            if let Some(ref tx) = s.cc_config_tx {
                tx.send(config).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn get_cc_stats() -> Result<serde_json::Value, String> {
    if let Ok(state) = SESSION_STATE.lock() {
        if let Some(ref s) = *state {
            return Ok(serde_json::json!({
                "bitrate_kbps": s.current_bitrate.load(Ordering::Relaxed),
                "state": s.cc_state.lock().unwrap().clone(),
            }));
        }
    }
    Err("No active session".into())
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_pcvr_status() -> String {
    wavry_client::pcvr_status()
}

#[tauri::command]
async fn register(
    app_handle: tauri::AppHandle,
    email: String,
    password: String,
    display_name: String,
    username: String,
) -> Result<String, String> {
    let identity = get_or_create_identity(&app_handle)?;
    let wavry_id = identity.wavry_id().to_string();

    let client = reqwest::Client::new();
    let res = client
        .post("https://auth.wavry.dev/auth/register")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
            "display_name": display_name,
            "username": username,
            "public_key": wavry_id
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok("Registration successful. Please login.".into())
    } else {
        Err(res
            .text()
            .await
            .unwrap_or_else(|_| "Registration failed".into()))
    }
}

#[tauri::command]
async fn login_full(
    app_handle: tauri::AppHandle,
    email: String,
    password: String,
) -> Result<serde_json::Value, String> {
    let identity = get_or_create_identity(&app_handle)?;
    let client = reqwest::Client::new();

    // 1. Get auth challenge
    let res = client
        .post("https://auth.wavry.dev/auth/challenge")
        .json(&serde_json::json!({
            "email": email,
            "wavry_id": identity.wavry_id().to_string()
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err("Failed to get auth challenge".into());
    }

    let challenge_resp: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let challenge_hex = challenge_resp["challenge"]
        .as_str()
        .ok_or("Missing challenge")?;
    let challenge = hex::decode(challenge_hex).map_err(|e| e.to_string())?;

    // 2. Sign challenge
    let signature = identity.sign(&challenge);
    let signature_hex = hex::encode(signature);

    // 3. Complete login with signature + password
    let res = client
        .post("https://auth.wavry.dev/auth/login")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
            "signature": signature_hex
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        let resp: LoginResponse = res.json().await.map_err(|e| e.to_string())?;
        let mut auth = AUTH_STATE.lock().unwrap();
        *auth = Some(AuthState {
            token: resp.session.token.clone(),
        });
        Ok(serde_json::json!({
            "username": resp.user.username,
            "token": resp.session.token
        }))
    } else {
        Err("Login failed: invalid signature or password".into())
    }
}

#[tauri::command]
async fn set_signaling_token(token: Option<String>) -> Result<(), String> {
    let mut auth = AUTH_STATE.lock().unwrap();
    if let Some(t) = token {
        *auth = Some(AuthState { token: t });
        log::info!("Signaling token re-hydrated from frontend");
    } else {
        *auth = None;
    }
    Ok(())
}

#[tauri::command]
async fn start_session(
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
        None // Discovery mode
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
        max_resolution,
        gamepad_enabled: gamepad_enabled.unwrap_or(true),
        gamepad_deadzone: gamepad_deadzone.unwrap_or(0.1).clamp(0.0, 0.95),
        vr_adapter: None,
        runtime_stats: None,
    };

    spawn_client_session(config)?;

    Ok("Session started".into())
}

#[tauri::command]
async fn list_monitors() -> Result<Vec<wavry_media::DisplayInfo>, String> {
    #[cfg(target_os = "macos")]
    let probe = MacProbe;
    #[cfg(target_os = "windows")]
    let probe = WindowsProbe;
    #[cfg(target_os = "linux")]
    let probe = LinuxProbe;

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    probe.enumerate_displays().map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_via_id(target_username: String) -> Result<String, String> {
    use wavry_client::signaling::{SignalMessage, SignalingClient};

    let token = {
        let auth = AUTH_STATE.lock().unwrap();
        if let Some(ref a) = *auth {
            a.token.clone()
        } else {
            return Err("Not logged in".into());
        }
    };

    log::info!("Connecting to {} via signaling", target_username);

    // 1. Connect to signaling
    let mut sig = SignalingClient::connect("wss://auth.wavry.dev/ws", &token)
        .await
        .map_err(|e| format!("Signaling error: {}", e))?;

    // 2. Discover STUN address if possible
    let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    let public_addr = if let Some(ref s) = udp {
        let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
        if let Some(tu) = tokio_u {
            wavry_client::discover_public_addr(&tu)
                .await
                .ok()
                .map(|a| a.to_string())
        } else {
            None
        }
    } else {
        None
    };

    log::info!("Discovered public addr: {:?}", public_addr);

    // 3. Send OfferRift
    let hello_b64 = wavry_client::create_hello_base64("wavry-desktop".into(), public_addr)
        .map_err(|e| e.to_string())?;
    sig.send(SignalMessage::OFFER_RIFT {
        target_username: target_username.clone(),
        hello_base64: hello_b64,
    })
    .await
    .map_err(|e| e.to_string())?;

    // 4. Wait for AnswerRift
    let mut relay_info: Option<wavry_client::RelayInfo> = None;

    loop {
        match sig.recv().await {
            Ok(SignalMessage::ANSWER_RIFT { ack_base64, .. }) => {
                let ack = wavry_client::decode_hello_ack_base64(&ack_base64)
                    .map_err(|e| e.to_string())?;
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

                let config = wavry_client::ClientConfig {
                    connect_addr,
                    client_name: "wavry-desktop".into(),
                    no_encrypt: false,
                    identity_key: None,
                    relay_info,
                    max_resolution: None,
                    gamepad_enabled: true,
                    gamepad_deadzone: 0.1,
                    vr_adapter: None,
                    runtime_stats: None,
                };

                spawn_client_session(config)?;

                return Ok("Connected".into());
            }
            Ok(SignalMessage::RELAY_CREDENTIALS {
                token,
                addr,
                session_id,
            }) => {
                log::info!("Received relay credentials: {}", addr);
                if let Ok(relay_addr) = addr.parse::<std::net::SocketAddr>() {
                    relay_info = Some(wavry_client::RelayInfo {
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
}

#[cfg(target_os = "linux")]
#[tauri::command]
async fn start_host(port: u16, display_id: Option<u32>) -> Result<String, String> {
    use bytes::Bytes;
    use std::net::UdpSocket;
    use std::thread;
    use wavry_media::{Codec, EncodeConfig, PipewireAudioCapturer, PipewireEncoder, Resolution};

    // Check if already hosting
    {
        let state = SESSION_STATE.lock().unwrap();
        if state.is_some() {
            return Err("Already hosting".into());
        }
    }

    let (cc_tx, _cc_rx) = mpsc::unbounded_channel::<rift_core::cc::DeltaConfig>();
    let current_bitrate = Arc::new(AtomicU32::new(8000));
    let cc_state_shared = Arc::new(Mutex::new("Stable".to_string()));

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    // Store state
    {
        let mut state = SESSION_STATE.lock().unwrap();
        *state = Some(SessionState {
            stop_tx: Some(stop_tx),
            cc_config_tx: Some(cc_tx),
            current_bitrate: current_bitrate.clone(),
            cc_state: cc_state_shared.clone(),
        });
    }

    // Create encoder in async block
    let config = EncodeConfig {
        codec: Codec::H264,
        resolution: Resolution {
            width: 1920,
            height: 1080,
        },
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        display_id,
    };

    let video_encoder = PipewireEncoder::new(config)
        .await
        .map_err(|e| e.to_string())?;
    let mut audio_capturer = PipewireAudioCapturer::new()
        .await
        .map_err(|e| e.to_string())?;

    // Optional: Register with Gateway if logged in
    let mut signaling_token: Option<String> = None;
    {
        let auth = AUTH_STATE.lock().unwrap();
        if let Some(ref a) = *auth {
            signaling_token = Some(a.token.clone());
        }
    }

    // Spawn host threads
    thread::spawn(move || {
        let mut video_encoder = video_encoder;
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to bind socket: {}", e);
                return;
            }
        };
        socket.set_nonblocking(true).ok();

        log::info!("Host thread started on port {}", port);

        // Shared client address
        let shared_client_addr = std::sync::Arc::new(std::sync::Mutex::new(None));

        // Audio thread
        let mut audio_capturer = audio_capturer;
        let socket_audio = socket.try_clone().expect("Failed to clone socket");
        let (audio_stop_tx, mut audio_stop_rx) = oneshot::channel::<()>();
        let shared_client_addr_audio = shared_client_addr.clone();

        thread::spawn(move || {
            let mut sequence: u64 = 0;
            let mut packet_id_counter: u64 = 1;
            loop {
                if audio_stop_rx.try_recv().is_ok() {
                    break;
                }
                if let Ok(frame) = audio_capturer.next_packet() {
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
                                    content: Some(rift_core::media_message::Content::Audio(audio)),
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

                        let _ = socket_audio.send_to(&phys.encode(), addr);
                        sequence = sequence.wrapping_add(1);

                        // Adaptive FEC tuning
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
            }
            log::info!("Audio thread exiting");
        });

        // Background signaling task if needed
        if let Some(token) = signaling_token {
            tokio::spawn(async move {
                if let Ok(mut sig) =
                    SignalingClient::connect("wss://auth.wavry.dev/ws", &token).await
                {
                    log::info!("Host registered with signaling gateway");
                    while let Ok(msg) = sig.recv().await {
                        match msg {
                            SignalMessage::OFFER_RIFT {
                                target_username,
                                hello_base64,
                            } => {
                                if let Ok(hello) = wavry_client::decode_hello_base64(&hello_base64)
                                {
                                    log::info!(
                                        "Received RIFT offer from {} ({})",
                                        hello.client_name,
                                        target_username
                                    );

                                    // Generate session ID and alias
                                    let session_id = [0u8; 16]; // TODO: random
                                    let session_alias = 1;

                                    // Discover own public address for host
                                    let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
                                    let my_public_addr = if let Some(ref s) = udp {
                                        let tokio_u =
                                            tokio::net::UdpSocket::from_std(s.try_clone().unwrap())
                                                .ok();
                                        if let Some(tu) = tokio_u {
                                            wavry_client::discover_public_addr(&tu)
                                                .await
                                                .ok()
                                                .map(|a| a.to_string())
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };

                                    // Extract resolution from Hello or default
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
                            _ => {}
                        }
                    }
                }
            });
        }

        let mut sequence: u64 = 0;
        let mut packet_id_counter: u64 = 1;
        let mut delta_cc = rift_core::cc::DeltaCC::new(
            rift_core::cc::DeltaConfig::default(),
            config.bitrate_kbps,
            config.fps as u32,
        );
        let mut fec_builder = rift_core::FecBuilder::new(20).unwrap();
        let mut last_fec_ratio = 0.05f32;

        let _last_cc_update = std::time::Instant::now();

        loop {
            if stop_rx.try_recv().is_ok() {
                let _ = audio_stop_tx.send(());
                break;
            }

            // Check for CC config updates from UI
            if let Ok(new_config) = cc_rx.try_recv() {
                log::info!("Updating DELTA config from UI");
                // Reset CC with new config (or add a mutation method if needed)
                delta_cc = rift_core::cc::DeltaCC::new(
                    new_config,
                    delta_cc.target_bitrate_kbps(),
                    delta_cc.target_fps(),
                );
            }

            let mut buf = [0u8; 2048];
            if let Ok((_len, src)) = socket.recv_from(&mut buf) {
                let mut addr_lock = shared_client_addr.lock().unwrap();
                if addr_lock.is_none() {
                    log::info!("Client connected from {}", src);
                    *addr_lock = Some(src);
                }

                // Handle incoming control/stats for CC
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

                                // Apply CC output to encoder
                                let new_bitrate = delta_cc.target_bitrate_kbps();
                                if let Err(e) = video_encoder.set_bitrate(new_bitrate) {
                                    log::error!("Failed to update bitrate: {}", e);
                                }

                                // Update shared stats for UI
                                current_bitrate.store(new_bitrate, Ordering::Relaxed);
                                let state_str = format!("{:?}", delta_cc.state()); // Assuming we add a state() getter
                                *cc_state_shared.lock().unwrap() = state_str;
                            }
                        }
                    }
                }
            }

            if let Ok(frame) = video_encoder.next_frame() {
                let addr = {
                    let addr_lock = shared_client_addr.lock().unwrap();
                    *addr_lock
                };

                if let Some(addr) = addr {
                    let max_payload = 1300; // Leave room for PhysicalPacket header
                    let data = frame.data;
                    let total_chunks = data.len().div_ceil(max_payload) as u32;
                    let frame_id = sequence;

                    for i in 0..total_chunks {
                        let start = (i as usize) * max_payload;
                        let end = std::cmp::min(start + max_payload, data.len());
                        let chunk_data = data[start..end].to_vec();

                        let chunk = rift_core::VideoChunk {
                            frame_id,
                            chunk_index: i as u32,
                            chunk_count: total_chunks,
                            timestamp_us: frame.timestamp_us,
                            keyframe: frame.keyframe,
                            payload: chunk_data,
                        };

                        let msg = rift_core::Message {
                            content: Some(rift_core::message::Content::Media(
                                rift_core::MediaMessage {
                                    content: Some(rift_core::media_message::Content::Video(chunk)),
                                },
                            )),
                        };

                        let phys = rift_core::PhysicalPacket {
                            version: rift_core::RIFT_VERSION,
                            session_id: None,
                            session_alias: None, // Fill if needed for signaling
                            packet_id: {
                                let id = packet_id_counter;
                                packet_id_counter += 1;
                                id
                            },
                            payload: Bytes::from(rift_core::encode_msg(&msg)),
                        };

                        let _ = socket.send_to(&phys.encode(), addr);
                        // Push to FEC builder
                        if let Some(fec) = fec_builder.push(packet_id_counter - 1, &phys.payload) {
                            let fec_msg = rift_core::Message {
                                content: Some(rift_core::message::Content::Media(
                                    rift_core::MediaMessage {
                                        content: Some(rift_core::media_message::Content::Fec(fec)),
                                    },
                                )),
                            };
                            let fec_phys = rift_core::PhysicalPacket {
                                version: rift_core::RIFT_VERSION,
                                session_id: None,
                                session_alias: None,
                                packet_id: 0,
                                payload: bytes::Bytes::from(rift_core::encode_msg(&fec_msg)),
                            };
                            let _ = socket.send_to(&fec_phys.encode(), addr);
                        }
                    }
                    sequence = sequence.wrapping_add(1);

                    // Adaptive FEC tuning
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
        }

        if let Ok(mut state) = SESSION_STATE.lock() {
            *state = None;
        }
    });

    Ok(format!("Hosting on port {}", port))
}

// macOS hosting - uses MacScreenEncoder
#[cfg(target_os = "macos")]
#[tauri::command]
async fn start_host(port: u16, display_id: Option<u32>) -> Result<String, String> {
    use bytes::Bytes;
    use std::net::UdpSocket;
    use std::thread;

    // Check if already hosting
    {
        let state = SESSION_STATE.lock().unwrap();
        if state.is_some() {
            return Err("Already hosting".into());
        }
    }

    let (cc_tx, _cc_rx) = mpsc::unbounded_channel::<rift_core::cc::DeltaConfig>();
    let current_bitrate = Arc::new(AtomicU32::new(8000));
    let cc_state_shared = Arc::new(Mutex::new("Stable".to_string()));
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    // Create encoder
    let config = EncodeConfig {
        codec: Codec::H264,
        resolution: Resolution {
            width: 1920,
            height: 1080,
        },
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        display_id,
    };

    let video_encoder = MacScreenEncoder::new(config)
        .await
        .map_err(|e| e.to_string())?;
    let mut audio_capturer = MacAudioCapturer::new().await.map_err(|e| e.to_string())?;

    // Bind before spawning worker threads so start_host returns a real error on failure.
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", port))
        .map_err(|e| format!("Failed to bind socket on {}: {}", port, e))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("Failed to set nonblocking socket: {}", e))?;

    // Store state
    {
        let mut state = SESSION_STATE.lock().unwrap();
        *state = Some(SessionState {
            stop_tx: Some(stop_tx),
            cc_config_tx: Some(cc_tx),
            current_bitrate: current_bitrate.clone(),
            cc_state: cc_state_shared.clone(),
        });
    }

    // Spawn host thread
    thread::spawn(move || {
        let mut video_encoder = video_encoder;

        // Similar loop to Windows/Linux below...
        // For brevity and parity, this would ideally be factored out, but for now we follow the existing pattern.
        log::info!("macOS host thread started on port {}", port);

        let shared_client_addr = Arc::new(Mutex::new(None));

        let socket_audio = socket.try_clone().expect("Failed to clone socket");
        let (audio_stop_tx, mut audio_stop_rx) = oneshot::channel::<()>();
        let shared_client_addr_audio = shared_client_addr.clone();

        thread::spawn(move || loop {
            if audio_stop_rx.try_recv().is_ok() {
                break;
            }
            if let Ok(frame) = audio_capturer.next_packet() {
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
                                content: Some(rift_core::media_message::Content::Audio(audio)),
                            },
                        )),
                    };

                    let phys = rift_core::PhysicalPacket {
                        version: rift_core::RIFT_VERSION,
                        session_id: None,
                        session_alias: None,
                        packet_id: 0,
                        payload: Bytes::from(rift_core::encode_msg(&msg)),
                    };

                    let _ = socket_audio.send_to(&phys.encode(), addr);
                }
            }
        });

        let mut sequence: u64 = 0;
        let mut packet_id_counter: u64 = 1;

        loop {
            if stop_rx.try_recv().is_ok() {
                let _ = audio_stop_tx.send(());
                break;
            }

            // Recv control packets
            let mut buf = [0u8; 2048];
            if let Ok((_len, src)) = socket.recv_from(&mut buf) {
                let mut addr_lock = shared_client_addr.lock().unwrap();
                if addr_lock.is_none() {
                    *addr_lock = Some(src);
                }
            }

            // Encode & Send
            if let Ok(frame) = video_encoder.next_frame() {
                let addr = { *shared_client_addr.lock().unwrap() };
                if let Some(addr) = addr {
                    // Send frame chunks (Simplified)
                    let msg = rift_core::Message {
                        content: Some(rift_core::message::Content::Media(
                            rift_core::MediaMessage {
                                content: Some(rift_core::media_message::Content::Video(
                                    rift_core::VideoChunk {
                                        frame_id: sequence,
                                        chunk_index: 0,
                                        chunk_count: 1,
                                        timestamp_us: frame.timestamp_us,
                                        keyframe: frame.keyframe,
                                        payload: frame.data,
                                    },
                                )),
                            },
                        )),
                    };
                    let phys = rift_core::PhysicalPacket {
                        version: rift_core::RIFT_VERSION,
                        session_id: None,
                        session_alias: None,
                        packet_id: packet_id_counter,
                        payload: Bytes::from(rift_core::encode_msg(&msg)),
                    };
                    let _ = socket.send_to(&phys.encode(), addr);
                    sequence += 1;
                    packet_id_counter += 1;
                }
            }
        }
    });

    Ok(format!("Hosting on port {}", port))
}

// Windows hosting - implementation
#[cfg(target_os = "windows")]
#[tauri::command]
async fn start_host(port: u16, display_id: Option<u32>) -> Result<String, String> {
    use bytes::Bytes;
    use std::net::UdpSocket;
    use std::sync::atomic::Ordering;
    use std::thread;
    use tokio::sync::{mpsc, oneshot};
    use wavry_media::{Codec, EncodeConfig, Resolution, WindowsAudioCapturer, WindowsEncoder};

    // Check if already hosting
    {
        let state = SESSION_STATE.lock().unwrap();
        if state.is_some() {
            return Err("Already hosting".into());
        }
    }

    let (cc_tx, mut cc_rx) = mpsc::unbounded_channel::<rift_core::cc::DeltaConfig>();
    let current_bitrate = Arc::new(AtomicU32::new(8000));
    let cc_state_shared = Arc::new(Mutex::new("Stable".to_string()));

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    // Create encoder in async block
    let config = EncodeConfig {
        codec: Codec::H264,
        resolution: Resolution {
            width: 1920,
            height: 1080,
        },
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
        display_id,
    };

    let video_encoder = WindowsEncoder::new(config)
        .await
        .map_err(|e| e.to_string())?;
    let mut audio_capturer = WindowsAudioCapturer::new()
        .await
        .map_err(|e| e.to_string())?;

    // Optional: Register with Gateway if logged in
    let mut signaling_token: Option<String> = None;
    {
        let auth = AUTH_STATE.lock().unwrap();
        if let Some(ref a) = *auth {
            signaling_token = Some(a.token.clone());
        }
    }

    // Store state
    {
        let mut state = SESSION_STATE.lock().unwrap();
        *state = Some(SessionState {
            stop_tx: Some(stop_tx),
            cc_config_tx: Some(cc_tx),
            current_bitrate: current_bitrate.clone(),
            cc_state: cc_state_shared.clone(),
        });
    }

    // Spawn Windows host thread
    thread::spawn(move || {
        let mut video_encoder = video_encoder;
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to bind socket: {}", e);
                return;
            }
        };
        socket.set_nonblocking(true).ok();

        log::info!("Windows host thread started on port {}", port);

        // Background signaling task if needed
        if let Some(token) = signaling_token {
            use wavry_client::signaling::{SignalMessage, SignalingClient};
            tokio::spawn(async move {
                if let Ok(mut sig) =
                    SignalingClient::connect("wss://auth.wavry.dev/ws", &token).await
                {
                    log::info!("Windows host registered with signaling gateway");
                    while let Ok(msg) = sig.recv().await {
                        match msg {
                            SignalMessage::OFFER_RIFT {
                                target_username,
                                hello_base64,
                            } => {
                                if let Ok(hello) = wavry_client::decode_hello_base64(&hello_base64)
                                {
                                    log::info!(
                                        "Received RIFT offer from {} ({})",
                                        hello.client_name,
                                        target_username
                                    );

                                    let session_id = [0u8; 16];
                                    let session_alias = 1;

                                    // Discover own public address for host
                                    let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
                                    let my_public_addr = if let Some(ref s) = udp {
                                        let tokio_u =
                                            tokio::net::UdpSocket::from_std(s.try_clone().unwrap())
                                                .ok();
                                        if let Some(tu) = tokio_u {
                                            wavry_client::discover_public_addr(&tu)
                                                .await
                                                .ok()
                                                .map(|a| a.to_string())
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
                            _ => {}
                        }
                    }
                }
            });
        }

        let shared_client_addr = std::sync::Arc::new(std::sync::Mutex::new(None));

        // Audio thread
        let socket_audio = socket.try_clone().expect("Failed to clone socket");
        let (audio_stop_tx, mut audio_stop_rx) = oneshot::channel::<()>();
        let shared_client_addr_audio = shared_client_addr.clone();

        thread::spawn(move || loop {
            if audio_stop_rx.try_recv().is_ok() {
                break;
            }
            if let Ok(frame) = audio_capturer.next_frame() {
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
                                content: Some(rift_core::media_message::Content::Audio(audio)),
                            },
                        )),
                    };

                    let phys = rift_core::PhysicalPacket {
                        version: rift_core::RIFT_VERSION,
                        session_id: None,
                        session_alias: None,
                        packet_id: 0,
                        payload: Bytes::from(rift_core::encode_msg(&msg)),
                    };

                    let _ = socket_audio.send_to(&phys.encode(), addr);
                }
            }
        });

        let mut sequence: u64 = 0;
        let mut packet_id_counter: u64 = 1;
        let mut delta_cc =
            rift_core::cc::DeltaCC::new(rift_core::cc::DeltaConfig::default(), 8000, 60);
        let mut fec_builder = rift_core::FecBuilder::new(20).unwrap();
        let mut last_fec_ratio = 0.05f32;

        loop {
            if stop_rx.try_recv().is_ok() {
                let _ = audio_stop_tx.send(());
                break;
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
                    log::info!("Windows Client connected from {}", src);
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
                                video_encoder.set_bitrate(new_bitrate).ok();

                                current_bitrate.store(new_bitrate, Ordering::Relaxed);
                                *cc_state_shared.lock().unwrap() =
                                    format!("{:?}", delta_cc.state());
                            }
                        }
                    }
                }
            }

            if let Ok(frame) = video_encoder.next_frame() {
                let addr = {
                    let addr_lock = shared_client_addr.lock().unwrap();
                    *addr_lock
                };

                if let Some(addr) = addr {
                    let max_payload = 1300;
                    let data = frame.data;
                    let total_chunks = data.len().div_ceil(max_payload) as u32;

                    for i in 0..total_chunks {
                        let start = (i as usize) * max_payload;
                        let end = std::cmp::min(start + max_payload, data.len());
                        let chunk_data = data[start..end].to_vec();

                        let chunk = rift_core::VideoChunk {
                            frame_id: sequence,
                            chunk_index: i as u32,
                            chunk_count: total_chunks,
                            timestamp_us: frame.timestamp_us,
                            keyframe: frame.keyframe,
                            payload: chunk_data,
                        };

                        let msg = rift_core::Message {
                            content: Some(rift_core::message::Content::Media(
                                rift_core::MediaMessage {
                                    content: Some(rift_core::media_message::Content::Video(chunk)),
                                },
                            )),
                        };

                        let phys = rift_core::PhysicalPacket {
                            version: rift_core::RIFT_VERSION,
                            session_id: None,
                            session_alias: None,
                            packet_id: 0,
                            payload: Bytes::from(rift_core::encode_msg(&msg)),
                        };

                        let _ = socket.send_to(&phys.encode(), addr);
                        // Push to FEC builder
                        if let Some(fec) = fec_builder.push(packet_id_counter - 1, &phys.payload) {
                            let fec_msg = rift_core::Message {
                                content: Some(rift_core::message::Content::Media(
                                    rift_core::MediaMessage {
                                        content: Some(rift_core::media_message::Content::Fec(fec)),
                                    },
                                )),
                            };
                            let fec_phys = rift_core::PhysicalPacket {
                                version: rift_core::RIFT_VERSION,
                                session_id: None,
                                session_alias: None,
                                packet_id: 0,
                                payload: bytes::Bytes::from(rift_core::encode_msg(&fec_msg)),
                            };
                            let _ = socket.send_to(&fec_phys.encode(), addr);
                        }
                    }
                    sequence = sequence.wrapping_add(1);

                    // Adaptive FEC tuning
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
        }
    });

    Ok(format!("Windows hosting started on port {}", port))
}

#[tauri::command]
async fn stop_host() -> Result<(), String> {
    let stop_tx = {
        let mut state = SESSION_STATE.lock().unwrap();
        state.as_mut().and_then(|s| s.stop_tx.take())
    };

    if let Some(tx) = stop_tx {
        let _ = tx.send(());
        Ok(())
    } else {
        Err("Not currently hosting".into())
    }
}

#[tauri::command]
async fn stop_session() -> Result<(), String> {
    let stop_tx = {
        let mut state = CLIENT_SESSION_STATE.lock().unwrap();
        state.as_mut().and_then(|s| s.stop_tx.take())
    };

    if let Some(tx) = stop_tx {
        let _ = tx.send(());
        Ok(())
    } else {
        let state = CLIENT_SESSION_STATE.lock().unwrap();
        if state.is_some() {
            Err("Client session is already stopping".into())
        } else {
            Err("No active client session".into())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_pcvr_status,
            start_session,
            connect_via_id,
            start_host,
            stop_host,
            stop_session,
            login_full,
            set_signaling_token,
            register,
            set_cc_config,
            get_cc_stats,
            list_monitors
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
