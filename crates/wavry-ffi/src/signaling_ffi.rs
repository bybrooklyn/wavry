use crate::RUNTIME;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc;
use wavry_client::signaling::{SignalMessage, SignalingClient};

// Global Signaling State
pub struct SignalingState {
    is_connected: AtomicBool,
    is_hosting: AtomicBool,
    host_port: Mutex<u16>,
    // Channel for outgoing signaling messages
    outgoing_tx: Mutex<Option<mpsc::UnboundedSender<SignalMessage>>>,
}

pub static SIGNALING: Lazy<SignalingState> = Lazy::new(|| SignalingState {
    is_connected: AtomicBool::new(false),
    is_hosting: AtomicBool::new(false),
    host_port: Mutex::new(4444),
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

/// Send a Connect request to a target user by their username
pub fn send_offer(target_username: &str) {
    let tx_guard = SIGNALING.outgoing_tx.lock().unwrap();
    if let Some(tx) = tx_guard.as_ref() {
        let port = *SIGNALING.host_port.lock().unwrap();
        // SDP contains our advertised port (host mode) for now
        let sdp = format!("{{\"type\":\"connect_request\",\"port\":{}}}", port);

        let tx: mpsc::UnboundedSender<SignalMessage> = tx.clone();
        let target = target_username.to_string();

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
            let _ = tx.send(msg);
        });
    } else {
        warn!("Cannot send OFFER: Signaling not connected");
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
        }
        Err(e) => {
            error!("Failed to connect to signaling: {}", e);
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

            // TODO: Automatically trigger start_client with the peer_addr if available
            if let Some(addr) = peer_addr {
                info!("P2P potential: Host is reachable at {}", addr);
            }
        }
        SignalMessage::RELAY_CREDENTIALS { token, addr, .. } => {
            info!("Received RELAY credentials: {} @ {}", token, addr);
            // TODO: Store and use for relay connection
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
        }
        _ => {
            info!("Received signal: {:?}", msg);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wavry_connect_signaling(
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
        return -1;
    }
    let c_str = CStr::from_ptr(username_ptr);
    let username = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };

    send_offer(username);
    0
}
