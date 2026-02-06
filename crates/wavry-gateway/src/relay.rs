use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

// Shared state for Relay Tokens
// Token -> (HostEmail, ClientEmail, Expiration)
// Actual binding: Addr -> Token
pub struct RelaySession {
    pub host_email: String,
    pub client_email: String,
    pub host_addr: Option<SocketAddr>,
    pub client_addr: Option<SocketAddr>,
    pub created_at: std::time::Instant,
    // Rate Limiting
    pub bytes_sent: usize,
    pub last_tick: std::time::Instant,
}

const BANDWIDTH_LIMIT_BPS: usize = 500 * 1024; // 500 KB/s (~4 Mbps)

pub type RelayMap = Arc<RwLock<HashMap<String, RelaySession>>>; // Key: Token

pub async fn run_relay_server(port: u16, state: RelayMap) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&addr).await?;
    info!("Relay Server listening on udp://{}", addr);

    let mut buf = [0u8; 2048]; // Max MTU usually 1500, safe margin

    loop {
        let (len, src_addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                warn!("Relay recv error: {}", e);
                continue;
            }
        };

        let data = &buf[0..len];

        // Protocol:
        // 1. Handshake: 1 byte TYPE (0x01), 16 Bytes Token (UUID bytes)
        // 2. Data: Anything else is forwarded based on src_addr

        if len == 17 && data[0] == 0x01 {
            // Handshake
            handle_handshake(&state, src_addr, &data[1..], &socket).await;
        } else {
            // Forwarding
            handle_forward(&state, src_addr, data, &socket).await;
        }
    }
}

async fn handle_handshake(
    state: &RelayMap,
    src: SocketAddr,
    token_bytes: &[u8],
    socket: &UdpSocket,
) {
    let token_uuid = match Uuid::from_slice(token_bytes) {
        Ok(u) => u.to_string(),
        Err(_) => return,
    };

    let mut guard = state.write().await;

    // We need to find which session this token belongs to
    // Simplified: Both Host and Client send the SAME token.
    // The first one to bind is stored as "HostAddr" (or just Peer A)
    // The second is "ClientAddr" (Peer B)
    // Actually, we can just fill slots.

    if let Some(session) = guard.get_mut(&token_uuid) {
        if session.host_addr.is_none() {
            session.host_addr = Some(src);
            info!("Relay: Peer A bonded for session {}", token_uuid);
            let _ = socket.send_to(b"\x02OK", src).await; // 0x02 = Ack
        } else if session.client_addr.is_none() {
            // Avoid rebinding same addr
            if Some(src) != session.host_addr {
                session.client_addr = Some(src);
                info!(
                    "Relay: Peer B bonded for session {}. Ready to relay.",
                    token_uuid
                );
                let _ = socket.send_to(b"\x02OK", src).await;

                // Notify Peer A? Optional.
            }
        } else {
            // Re-binding logic? Or reject 3rd party?
            // If src matches existing, just ack.
            if Some(src) == session.host_addr || Some(src) == session.client_addr {
                let _ = socket.send_to(b"\x02OK", src).await;
            } else {
                warn!("Relay: Session full or invalid attempt from {}", src);
            }
        }
    } else {
        warn!("Relay: Unknown token from {}", src);
    }
}

async fn handle_forward(state: &RelayMap, src: SocketAddr, data: &[u8], socket: &UdpSocket) {
    // Reverse lookup: Src -> Target
    // This is O(N) if we iterate.
    // OPTIMIZATION: Maintain a separate `Addr -> TargetAddr` map for O(1) forwarding.
    // For MVP/Monolith, we can lock read and find.
    // Ideally `RelayMap` connects Token -> Session.
    // We need `AddrMap` for fast path.
    // Let's modify `run_relay_server` to keep a local fast-map `HashMap<SocketAddr, SocketAddr>`.
    // But `RelayMap` is modified by Signaling (to add tokens).
    // The shared state is tricky.

    // Better: `RelayMap` stores the authoritative session state.
    // The Relay Loop maintains a local `FastMap`.
    // Periodically (or on handshake), `FastMap` is updated.

    // Implementation for MVP: Write Lock + Find (Need write for rate limiting)
    let mut guard = state.write().await;

    // Find session containing src
    // Note: iterating values_mut() allows modification
    for session in guard.values_mut() {
        let is_host = session.host_addr == Some(src);
        let is_client = session.client_addr == Some(src);

        if is_host || is_client {
            // Check Rate Limit
            let now = std::time::Instant::now();
            if now.duration_since(session.last_tick).as_secs() >= 1 {
                session.bytes_sent = 0;
                session.last_tick = now;
            }

            if session.bytes_sent + data.len() > BANDWIDTH_LIMIT_BPS {
                // Drop packet implies congestion or abuse
                // debug!("Relay: Rate limit exceeded for session");
                return;
            }

            session.bytes_sent += data.len();

            // Forward
            if let Some(target) = if is_host {
                session.client_addr
            } else {
                session.host_addr
            } {
                let _ = socket.send_to(data, target).await;
            }
            return;
        }
    }

    // debug!("Relay: packet dropped from unbonded source {}", src);
}
