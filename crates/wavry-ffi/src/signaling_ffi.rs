use crate::RUNTIME;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use std::ffi::{c_char, CStr};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc;
use wavry_client::signaling::{SignalMessage, SignalingClient};

// Global Signaling State
pub struct SignalingState {
    is_connected: AtomicBool,
    is_hosting: AtomicBool,
    host_port: Mutex<u16>,
    pending_target: Mutex<Option<String>>,
    // Channel for outgoing signaling messages
    outgoing_tx: Mutex<Option<mpsc::UnboundedSender<SignalMessage>>>,
}

pub static SIGNALING: Lazy<SignalingState> = Lazy::new(|| SignalingState {
    is_connected: AtomicBool::new(false),
    is_hosting: AtomicBool::new(false),
    host_port: Mutex::new(4444),
    pending_target: Mutex::new(None),
    outgoing_tx: Mutex::new(None),
});

/// Called from session.rs when hosting starts
#[allow(dead_code)]
pub fn set_hosting(port: u16) {
    SIGNALING.is_hosting.store(true, Ordering::SeqCst);
    *SIGNALING.host_port.lock().unwrap() = port;
    info!("Signaling: Hosting enabled on port {}", port);
}

/// Called from session.rs when hosting stops
#[allow(dead_code)]
pub fn clear_hosting() {
    SIGNALING.is_hosting.store(false, Ordering::SeqCst);
    info!("Signaling: Hosting disabled");
}

/// Send a Connect request to a target user by their username.
pub fn send_offer(target_username: &str) -> Result<(), &'static str> {
    if target_username.trim().is_empty() {
        return Err("target username is required");
    }

    let tx_guard = SIGNALING.outgoing_tx.lock().unwrap();
    if let Some(tx) = tx_guard.as_ref() {
        let port = *SIGNALING.host_port.lock().unwrap();
        // SDP contains our advertised port (host mode) for now
        let sdp = format!("{{\"type\":\"connect_request\",\"port\":{}}}", port);

        let tx: mpsc::UnboundedSender<SignalMessage> = tx.clone();
        let target = target_username.to_string();
        *SIGNALING.pending_target.lock().unwrap() = Some(target.clone());
        crate::set_cloud_status("Request sent. Waiting for host acknowledgment...");

        // Use the runtime to do STUN discovery and then send
        RUNTIME.spawn(async move {
            let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
            let public_addr = if let Some(ref s) = udp {
                let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
                if let Some(tu) = tokio_u {
                    wavry_client::discover_public_addr(&tu)
                        .await
                        .ok()
                        .map(|a: std::net::SocketAddr| a.to_string())
                } else {
                    None
                }
            } else {
                None
            };

            let msg = SignalMessage::OFFER {
                target_username: target,
                sdp,
                public_addr,
            };
            if tx.send(msg).is_err() {
                warn!("Failed to queue OFFER on signaling channel");
                *SIGNALING.pending_target.lock().unwrap() = None;
            }
        });

        Ok(())
    } else {
        warn!("Cannot send OFFER: Signaling not connected");
        Err("signaling is not connected")
    }
}

fn request_relay_for_target(target_username: &str) -> Result<(), &'static str> {
    let tx_guard = SIGNALING.outgoing_tx.lock().unwrap();
    let Some(tx) = tx_guard.as_ref() else {
        return Err("signaling is not connected");
    };

    let msg = SignalMessage::REQUEST_RELAY {
        target_username: target_username.to_string(),
    };
    tx.send(msg).map_err(|_| "failed to send relay request")
}

fn parse_host_target(
    sdp: &str,
    public_addr: Option<String>,
) -> Result<(String, u16), &'static str> {
    let parsed_port = parse_port_from_sdp(sdp);

    let Some(raw_addr) = public_addr else {
        return Err("missing host public address in signaling answer");
    };
    let socket = SocketAddr::from_str(&raw_addr)
        .map_err(|_| "invalid host public address in signaling answer")?;

    let port = parsed_port.unwrap_or(socket.port());
    if port == 0 {
        return Err("invalid host port in signaling answer");
    }

    Ok((socket.ip().to_string(), port))
}

fn parse_port_from_sdp(sdp: &str) -> Option<u16> {
    let marker = "\"port\"";
    let marker_idx = sdp.find(marker)?;
    let after_marker = &sdp[(marker_idx + marker.len())..];
    let colon_idx = after_marker.find(':')?;
    let after_colon = &after_marker[(colon_idx + 1)..];

    let digits: String = after_colon
        .chars()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }

    let parsed = digits.parse::<u16>().ok()?;
    if parsed == 0 {
        return None;
    }
    Some(parsed)
}

fn start_client_from_targets(
    direct_target: Option<(String, u16)>,
    relay_info: Option<wavry_client::RelayInfo>,
    stage_message: &'static str,
) {
    crate::set_cloud_status(stage_message);
    std::thread::spawn(move || {
        let rc = crate::start_client_with_targets(direct_target, relay_info);
        if rc == 0 {
            crate::set_cloud_status("Host acknowledged. Establishing secure session...");
        } else {
            crate::set_cloud_status("Cloud connect failed.");
        }
    });
}

fn auto_start_client_from_answer(
    target_username: String,
    sdp: String,
    public_addr: Option<String>,
) {
    let expected = SIGNALING.pending_target.lock().unwrap().clone();
    if expected.as_deref() != Some(target_username.as_str()) {
        warn!(
            "Ignoring ANSWER from {}: pending cloud target is {:?}",
            target_username, expected
        );
        return;
    }

    crate::set_cloud_status("Host acknowledged request.");

    match parse_host_target(&sdp, public_addr) {
        Ok((host_ip, port)) => {
            *SIGNALING.pending_target.lock().unwrap() = None;
            info!(
                "Cloud ANSWER resolved target {} -> {}:{}; starting direct client",
                target_username, host_ip, port
            );
            start_client_from_targets(
                Some((host_ip, port)),
                None,
                "Host acknowledged. Starting direct session...",
            );
        }
        Err(msg) => {
            warn!(
                "Cloud ANSWER missing direct endpoint for {}: {}. Requesting relay.",
                target_username, msg
            );
            crate::set_cloud_status("Direct route unavailable. Requesting relay...");
            match request_relay_for_target(&target_username) {
                Ok(()) => {}
                Err(relay_err) => {
                    error!(
                        "Relay request failed for {}: {}",
                        target_username, relay_err
                    );
                    crate::set_last_error(&format!(
                        "Cloud connect failed: {} (relay request failed: {})",
                        msg, relay_err
                    ));
                    crate::set_cloud_status("Relay request failed.");
                    *SIGNALING.pending_target.lock().unwrap() = None;
                }
            }
        }
    }
}

pub async fn start_signaling_bg(url: String, token: String) {
    info!("Connecting to signaling server: {}", url);

    match SignalingClient::connect(&url, &token).await {
        Ok(client) => {
            info!("Signaling Connected!");
            SIGNALING.is_connected.store(true, Ordering::SeqCst);

            // Create outgoing channel
            let (tx, mut rx) = mpsc::unbounded_channel::<SignalMessage>();
            *SIGNALING.outgoing_tx.lock().unwrap() = Some(tx);

            // Spawn sender task
            let mut send_half = client; // SignalingClient owns the stream

            loop {
                tokio::select! {
                    // Outgoing messages
                    Some(msg) = rx.recv() => {
                        if let Err(e) = send_half.send(msg).await {
                            error!("Failed to send signaling message: {}", e);
                            break;
                        }
                    }
                    // Incoming messages
                    result = send_half.recv() => {
                        match result {
                            Ok(msg) => {
                                handle_signal_message(msg).await;
                            }
                            Err(e) => {
                                error!("Signaling connection lost: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            SIGNALING.is_connected.store(false, Ordering::SeqCst);
            *SIGNALING.outgoing_tx.lock().unwrap() = None;
            *SIGNALING.pending_target.lock().unwrap() = None;
        }
        Err(e) => {
            error!("Failed to connect to signaling: {}", e);
            *SIGNALING.pending_target.lock().unwrap() = None;
        }
    }
}

async fn handle_signal_message(msg: SignalMessage) {
    match msg {
        SignalMessage::OFFER {
            target_username,
            sdp,
            public_addr: peer_addr,
        } => {
            // Someone wants to connect to us!
            info!(
                "Received OFFER from {}: {}. Peer public addr: {:?}",
                target_username, sdp, peer_addr
            );

            // Check if we're hosting
            if SIGNALING.is_hosting.load(Ordering::SeqCst) {
                // Discover our own public address to respond
                let udp = std::net::UdpSocket::bind("0.0.0.0:0").ok();
                let my_public_addr = if let Some(ref s) = udp {
                    let tokio_u = tokio::net::UdpSocket::from_std(s.try_clone().unwrap()).ok();
                    if let Some(tu) = tokio_u {
                        wavry_client::discover_public_addr(&tu)
                            .await
                            .ok()
                            .map(|a: std::net::SocketAddr| a.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let port = *SIGNALING.host_port.lock().unwrap();
                let answer_sdp = format!("{{\"type\":\"host_ready\",\"port\":{}}}", port);

                let tx_guard = SIGNALING.outgoing_tx.lock().unwrap();
                if let Some(tx) = tx_guard.as_ref() {
                    let answer = SignalMessage::ANSWER {
                        target_username: target_username.clone(),
                        sdp: answer_sdp,
                        public_addr: my_public_addr,
                    };
                    let tx: mpsc::UnboundedSender<SignalMessage> = tx.clone();
                    let _ = tx.send(answer);
                    info!("Sent ANSWER to {}", target_username);
                }
            } else {
                warn!("Received OFFER but not hosting, ignoring");
            }
        }
        SignalMessage::ANSWER {
            target_username,
            sdp,
            public_addr: peer_addr,
        } => {
            // Host responded to our connection request!
            info!(
                "Received ANSWER from {}: {}. Host public addr: {:?}",
                target_username, sdp, peer_addr
            );

            if let Some(addr) = peer_addr.as_deref() {
                info!("P2P potential: Host is reachable at {}", addr);
            }

            auto_start_client_from_answer(target_username, sdp, peer_addr);
        }
        SignalMessage::RELAY_CREDENTIALS {
            token,
            addr,
            session_id,
        } => {
            let target = SIGNALING.pending_target.lock().unwrap().clone();
            if target.is_none() {
                info!(
                    "Received RELAY credentials with no pending cloud target; ignoring ({})",
                    addr
                );
                return;
            }
            let target = target.unwrap_or_default();
            info!(
                "Received RELAY credentials for pending target {} via {}",
                target, addr
            );

            let relay_addr = match SocketAddr::from_str(&addr) {
                Ok(v) => v,
                Err(_) => {
                    crate::set_last_error("Cloud relay failed: invalid relay address");
                    crate::set_cloud_status("Relay response invalid.");
                    *SIGNALING.pending_target.lock().unwrap() = None;
                    return;
                }
            };

            *SIGNALING.pending_target.lock().unwrap() = None;
            let relay_info = wavry_client::RelayInfo {
                addr: relay_addr,
                token,
                session_id,
            };
            start_client_from_targets(
                None,
                Some(relay_info),
                "Relay allocated. Starting session through relay...",
            );
        }
        SignalMessage::CANDIDATE {
            target_username,
            candidate,
        } => {
            info!(
                "Received ICE candidate from {}: {}",
                target_username, candidate
            );
            // Not used in Wavry's direct UDP model, but logged for completeness
        }
        SignalMessage::ERROR { code, message } => {
            error!("Received signal ERROR (code {:?}): {}", code, message);
            if SIGNALING.pending_target.lock().unwrap().is_some() {
                crate::set_last_error(&format!("Cloud request rejected: {}", message));
                crate::set_cloud_status("Cloud request rejected.");
                *SIGNALING.pending_target.lock().unwrap() = None;
            }
        }
        _ => {
            info!("Received signal: {:?}", msg);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_connect_signaling(token_ptr: *const c_char) -> i32 {
    if token_ptr.is_null() {
        return -1;
    }

    let c_token = CStr::from_ptr(token_ptr);
    let token = match c_token.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };

    let default_url = "wss://auth.wavry.dev/ws".to_string();
    RUNTIME.spawn(async move {
        start_signaling_bg(default_url, token).await;
    });

    0
}

#[no_mangle]
pub unsafe extern "C" fn wavry_connect_signaling_with_url(
    url_ptr: *const c_char,
    token_ptr: *const c_char,
) -> i32 {
    if url_ptr.is_null() || token_ptr.is_null() {
        return -1;
    }

    let c_url = CStr::from_ptr(url_ptr);
    let url = match c_url.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };

    let c_token = CStr::from_ptr(token_ptr);
    let token = match c_token.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };

    RUNTIME.spawn(async move {
        start_signaling_bg(url, token).await;
    });

    0
}

#[no_mangle]
pub unsafe extern "C" fn wavry_send_connect_request(username_ptr: *const c_char) -> i32 {
    if username_ptr.is_null() {
        crate::set_last_error("Cloud connect request failed: null username");
        return -1;
    }
    let c_str = CStr::from_ptr(username_ptr);
    let username = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::set_last_error("Cloud connect request failed: invalid UTF-8 username");
            return -2;
        }
    };

    match send_offer(username) {
        Ok(()) => {
            crate::clear_last_error();
            0
        }
        Err(msg) => {
            crate::set_last_error(&format!("Cloud connect request failed: {}", msg));
            -3
        }
    }
}
