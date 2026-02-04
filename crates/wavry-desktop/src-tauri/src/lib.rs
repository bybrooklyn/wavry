use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::{oneshot, mpsc};
use wavry_client::{run_client, ClientConfig};
use serde::{Deserialize, Serialize};

/// Global session state for the desktop app
struct SessionState {
    stop_tx: Option<oneshot::Sender<()>>,
    cc_config_tx: Option<mpsc::UnboundedSender<rift_core::cc::DeltaConfig>>,
    current_bitrate: Arc<AtomicU32>,
    cc_state: Arc<Mutex<String>>,
}

static SESSION_STATE: Mutex<Option<SessionState>> = Mutex::new(None);
static AUTH_STATE: Mutex<Option<AuthState>> = Mutex::new(None);

struct AuthState {
    token: String,
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
async fn register(email: String, password: String, display_name: String, username: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let res = client.post("https://auth.wavry.dev/auth/register")
        .json(&serde_json::json!({
            "email": email,
            "password": password,
            "display_name": display_name,
            "username": username,
            "public_key": "TODO" // Phase 2: Crypto
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok("Registration successful. Please login.".into())
    } else {
        Err(res.text().await.unwrap_or_else(|_| "Registration failed".into()))
    }
}

#[tauri::command]
async fn login_full(email: String, password: String) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let res = client.post("https://auth.wavry.dev/auth/login")
        .json(&serde_json::json!({
            "email": email,
            "password": password
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
        Err("Login failed".into())
    }
}

#[tauri::command]
async fn set_signaling_token(token: Option<String>) -> Result<(), String> {
    let mut auth = AUTH_STATE.lock().unwrap();
    if let Some(t) = token {
        *auth = Some(AuthState {
            token: t,
        });
        log::info!("Signaling token re-hydrated from frontend");
    } else {
        *auth = None;
    }
    Ok(())
}

#[tauri::command]
async fn start_session(addr: String) -> Result<String, String> {
    let socket_addr = if let Ok(s) = SocketAddr::from_str(&addr) {
        Some(s)
    } else if addr.is_empty() {
        None // Discovery mode
    } else {
        return Err("Invalid IP address".into());
    };

    let config = ClientConfig {
        connect_addr: socket_addr,
        client_name: "wavry-desktop".to_string(),
        no_encrypt: false,
        identity_key: None,
    };

    // Spawn client in background
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_client(config, None).await {
            log::error!("Client error: {}", e);
        }
    });

    Ok("Session started".into())
}

#[tauri::command]
async fn connect_via_id(target_username: String) -> Result<String, String> {
    use wavry_client::signaling::{SignalingClient, SignalMessage};
    
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
    let mut sig = SignalingClient::connect("wss://auth.wavry.dev/ws", &token).await
        .map_err(|e| format!("Signaling error: {}", e))?;

    // 2. Discover STUN address if possible
    let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    let public_addr = if let Some(ref s) = udp {
        let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
        if let Some(tu) = tokio_u {
            wavry_client::discover_public_addr(&tu).await.ok().map(|a| a.to_string())
        } else { None }
    } else { None };

    log::info!("Discovered public addr: {:?}", public_addr);

    // 3. Send Offer (MVP: stubbed SDI/setup)
    sig.send(SignalMessage::OFFER { 
        target_username: target_username.clone(), 
        sdp: "RIFT_V1_SETUP".into(),
        public_addr
    }).await.map_err(|e| e.to_string())?;

    // 3. Wait for Answer
    loop {
        match sig.recv().await {
            Ok(SignalMessage::ANSWER { sdp, .. }) => {
                log::info!("Received answer from {}: {}", target_username, sdp);
                // Start RIFT session with info from SDP (e.g. IP/Port or Relay Token)
                // For MVP, we presume the Gateway relays RIFT or provides direct IP
                return Ok("Connected".into());
            }
            Ok(SignalMessage::ERROR { message, .. }) => return Err(message),
            Ok(_) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(target_os = "linux")]
#[tauri::command]
async fn start_host(port: u16) -> Result<String, String> {
    use wavry_media::{Codec, EncodeConfig, Resolution, PipewireEncoder};
    use std::net::UdpSocket;
    use std::thread;
    use wavry_client::signaling::{SignalingClient, SignalMessage};

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
        resolution: Resolution { width: 1920, height: 1080 },
        fps: 60,
        bitrate_kbps: 8000,
        keyframe_interval_ms: 2000,
    };
    
    let video_encoder = PipewireEncoder::new(config).await.map_err(|e| e.to_string())?;
    let mut audio_capturer = PipewireAudioCapturer::new().await.map_err(|e| e.to_string())?;

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
        let socket_audio = socket.try_clone().expect("Failed to clone socket");
        let (audio_stop_tx, mut audio_stop_rx) = oneshot::channel::<()>();
        let shared_client_addr_audio = shared_client_addr.clone();
        
        thread::spawn(move || {
            let mut sequence: u64 = 0;
            loop {
                if audio_stop_rx.try_recv().is_ok() { break; }
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
                            content: Some(rift_core::message::Content::Media(rift_core::MediaMessage {
                                content: Some(rift_core::media_message::Content::Audio(audio)),
                            })),
                        };

                        let phys = rift_core::PhysicalPacket {
                            version: rift_core::RIFT_VERSION,
                            session_id: None,
                            session_alias: None,
                            packet_id: 0, // Simplified
                            payload: rift_core::encode_msg(&msg),
                        };

                        let _ = socket_audio.send_to(&phys.encode(), addr);
                        sequence = sequence.wrapping_add(1);
                    }
                }
            }
            log::info!("Audio thread exiting");
        });

        // Background signaling task if needed
        if let Some(token) = signaling_token {
            tokio::spawn(async move {
                if let Ok(mut sig) = SignalingClient::connect("wss://auth.wavry.dev/ws", &token).await {
                    log::info!("Host registered with signaling gateway");
                    while let Ok(msg) = sig.recv().await {
                        match msg {
                            SignalMessage::OFFER { target_username: _, sdp: _, public_addr: peer_addr } => {
                                log::info!("Received connection offer from peer at {:?}", peer_addr);
                                // Discover own public address
                                let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
                                let my_public_addr = if let Some(ref s) = udp {
                                    let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
                                    if let Some(tu) = tokio_u {
                                        wavry_client::discover_public_addr(&tu).await.ok().map(|a| a.to_string())
                                    } else { None }
                                } else { None };

                                // Send Answer
                                let _ = sig.send(SignalMessage::ANSWER {
                                    target_username: "TODO_PEER".into(), // Gateway handles routing
                                    sdp: "RIFT_V1_READY".into(),
                                    public_addr: my_public_addr,
                                }).await;
                            }
                            _ => {}
                        }
                    }
                }
            });
        }

        let mut sequence: u64 = 0;
        let mut delta_cc = rift_core::cc::DeltaCC::new(
            rift_core::cc::DeltaConfig::default(),
            config.bitrate_kbps,
            config.fps as u32
        );
        
        let mut last_cc_update = std::time::Instant::now();

        loop {
            if stop_rx.try_recv().is_ok() { 
                let _ = audio_stop_tx.send(());
                break; 
            }

            // Check for CC config updates from UI
            if let Ok(new_config) = cc_rx.try_recv() {
                log::info!("Updating DELTA config from UI");
                // Reset CC with new config (or add a mutation method if needed)
                delta_cc = rift_core::cc::DeltaCC::new(new_config, delta_cc.target_bitrate_kbps(), delta_cc.target_fps());
            }
            
            let mut buf = [0u8; 2048];
            if let Ok((len, src)) = socket.recv_from(&mut buf) {
                let mut addr_lock = shared_client_addr.lock().unwrap();
                if addr_lock.is_none() {
                    log::info!("Client connected from {}", src);
                    *addr_lock = Some(src);
                }

                // Handle incoming control/stats for CC
                if let Ok(phys) = rift_core::PhysicalPacket::decode(&buf[..len]) {
                    if let Ok(msg) = rift_core::decode_msg(&phys.payload) {
                        if let Some(rift_core::message::Content::Control(ctrl)) = msg.content {
                            if let Some(rift_core::control_message::Content::Stats(stats)) = ctrl.content {
                                let loss = if stats.received_packets > 0 {
                                    stats.lost_packets as f32 / (stats.received_packets + stats.lost_packets) as f32
                                } else { 0.0 };
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
                            content: Some(rift_core::message::Content::Media(rift_core::MediaMessage {
                                content: Some(rift_core::media_message::Content::Video(chunk)),
                            })),
                        };

                        let phys = rift_core::PhysicalPacket {
                            version: rift_core::RIFT_VERSION,
                            session_id: None,
                            session_alias: None, // Fill if needed for signaling
                            packet_id: 0, // Simplified for now
                            payload: rift_core::encode_msg(&msg),
                        };

                        let _ = socket.send_to(&phys.encode(), addr);
                    }
                    sequence = sequence.wrapping_add(1);
                }
            }
        }
        
        if let Ok(mut state) = SESSION_STATE.lock() { *state = None; }
    });
    
    Ok(format!("Hosting on port {}", port))
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
async fn start_host(_port: u16) -> Result<String, String> {
    Err("Hosting only supported on Linux in desktop app".into())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet, 
            start_session, 
            connect_via_id,
            start_host, 
            stop_host,
            login_full,
            set_signaling_token,
            register,
            set_cc_config,
            get_cc_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
