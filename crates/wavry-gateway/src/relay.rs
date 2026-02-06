use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rift_core::relay::{
    LeaseAckPayload, LeasePresentPayload, LeaseRejectPayload, LeaseRejectReason, RelayHeader,
    RelayPacketType, RELAY_HEADER_SIZE, RELAY_MAX_PACKET_SIZE,
};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::security;

pub struct RelaySession {
    pub host_email: String,
    pub client_email: String,
    pub session_id: Uuid,
    pub host_addr: Option<SocketAddr>,
    pub client_addr: Option<SocketAddr>,
    pub created_at: Instant,
    pub bytes_sent: usize,
    pub last_tick: Instant,
}

const BANDWIDTH_LIMIT_BPS: usize = 500 * 1024;
const ROUTE_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(300);

pub type RelayMap = Arc<RwLock<HashMap<String, RelaySession>>>;

#[derive(Clone)]
enum RouteSide {
    Host,
    Client,
}

#[derive(Clone)]
struct RouteEntry {
    token: String,
    session_id: Uuid,
    side: RouteSide,
    dest: SocketAddr,
    last_seen: Instant,
}

pub async fn run_relay_server(port: u16, state: RelayMap) -> Result<()> {
    let addr = std::env::var("WAVRY_GATEWAY_RELAY_BIND_ADDR")
        .unwrap_or_else(|_| format!("127.0.0.1:{}", port));
    if let Ok(parsed) = addr.parse::<SocketAddr>() {
        let allow_public = std::env::var("WAVRY_GATEWAY_RELAY_ALLOW_PUBLIC_BIND")
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        if !parsed.ip().is_loopback() && !allow_public {
            return Err(anyhow::anyhow!(
                "refusing non-loopback gateway relay bind without WAVRY_GATEWAY_RELAY_ALLOW_PUBLIC_BIND=1"
            ));
        }
    }
    let socket = UdpSocket::bind(&addr).await?;
    info!("relay server listening on udp://{}", addr);

    let mut buf = [0u8; RELAY_MAX_PACKET_SIZE];
    let mut routes: HashMap<SocketAddr, RouteEntry> = HashMap::new();

    loop {
        let (len, src_addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(err) => {
                warn!("relay recv error: {}", err);
                continue;
            }
        };

        if len < RELAY_HEADER_SIZE {
            continue;
        }

        let packet = &buf[..len];
        let header = match RelayHeader::decode(packet) {
            Ok(header) => header,
            Err(err) => {
                debug!("dropping invalid relay header from {}: {}", src_addr, err);
                continue;
            }
        };

        let payload = &packet[RELAY_HEADER_SIZE..];
        match header.packet_type {
            RelayPacketType::LeasePresent => {
                handle_lease_present(&state, &mut routes, src_addr, &header, payload, &socket)
                    .await;
            }
            RelayPacketType::Forward => {
                handle_forward(&state, &mut routes, src_addr, &header, packet, &socket).await;
            }
            RelayPacketType::LeaseRenew => {
                send_lease_ack(&socket, header.session_id, src_addr, DEFAULT_LEASE_TTL).await;
            }
            _ => {
                debug!("ignoring unexpected relay packet type from {}", src_addr);
            }
        }

        cleanup_routes(&mut routes);
    }
}

async fn handle_lease_present(
    state: &RelayMap,
    routes: &mut HashMap<SocketAddr, RouteEntry>,
    src: SocketAddr,
    header: &RelayHeader,
    payload: &[u8],
    socket: &UdpSocket,
) {
    let lease = match LeasePresentPayload::decode(payload) {
        Ok(v) => v,
        Err(_) => {
            send_lease_reject(
                socket,
                header.session_id,
                src,
                LeaseRejectReason::InvalidSignature,
            )
            .await;
            return;
        }
    };

    let token = match String::from_utf8(lease.lease_token) {
        Ok(token) if security::is_valid_session_token(&token) => token,
        _ => {
            send_lease_reject(
                socket,
                header.session_id,
                src,
                LeaseRejectReason::InvalidSignature,
            )
            .await;
            return;
        }
    };

    let mut sessions = state.write().await;
    let Some(session) = sessions.get_mut(&token) else {
        send_lease_reject(
            socket,
            header.session_id,
            src,
            LeaseRejectReason::InvalidSignature,
        )
        .await;
        return;
    };

    if session.session_id != header.session_id {
        send_lease_reject(
            socket,
            header.session_id,
            src,
            LeaseRejectReason::WrongRelay,
        )
        .await;
        return;
    }

    let now = Instant::now();
    let mut host_bound = false;
    let mut client_bound = false;

    match lease.peer_role {
        rift_core::relay::PeerRole::Server => {
            host_bound = bind_host(session, src);
            if !host_bound && session.client_addr.is_none() {
                client_bound = bind_client(session, src);
            }
        }
        rift_core::relay::PeerRole::Client => {
            client_bound = bind_client(session, src);
            if !client_bound && session.host_addr.is_none() {
                host_bound = bind_host(session, src);
            }
        }
    }

    if !host_bound && !client_bound {
        send_lease_reject(
            socket,
            header.session_id,
            src,
            LeaseRejectReason::SessionFull,
        )
        .await;
        return;
    }

    if let (Some(host), Some(client)) = (session.host_addr, session.client_addr) {
        routes.insert(
            host,
            RouteEntry {
                token: token.clone(),
                session_id: session.session_id,
                side: RouteSide::Host,
                dest: client,
                last_seen: now,
            },
        );
        routes.insert(
            client,
            RouteEntry {
                token: token.clone(),
                session_id: session.session_id,
                side: RouteSide::Client,
                dest: host,
                last_seen: now,
            },
        );
    }

    send_lease_ack(socket, session.session_id, src, DEFAULT_LEASE_TTL).await;
}

fn bind_host(session: &mut RelaySession, src: SocketAddr) -> bool {
    match session.host_addr {
        None => {
            session.host_addr = Some(src);
            true
        }
        Some(addr) => addr == src,
    }
}

fn bind_client(session: &mut RelaySession, src: SocketAddr) -> bool {
    match session.client_addr {
        None => {
            session.client_addr = Some(src);
            true
        }
        Some(addr) => addr == src,
    }
}

async fn handle_forward(
    state: &RelayMap,
    routes: &mut HashMap<SocketAddr, RouteEntry>,
    src: SocketAddr,
    header: &RelayHeader,
    packet: &[u8],
    socket: &UdpSocket,
) {
    let Some(route) = routes.get_mut(&src) else {
        return;
    };

    if route.session_id != header.session_id {
        routes.remove(&src);
        return;
    }

    let now = Instant::now();
    let mut sessions = state.write().await;
    let Some(session) = sessions.get_mut(&route.token) else {
        routes.remove(&src);
        return;
    };

    if session.session_id != header.session_id {
        routes.remove(&src);
        return;
    }

    let source_matches = match route.side {
        RouteSide::Host => session.host_addr == Some(src),
        RouteSide::Client => session.client_addr == Some(src),
    };
    if !source_matches {
        routes.remove(&src);
        return;
    }

    if now.duration_since(session.last_tick).as_secs() >= 1 {
        session.bytes_sent = 0;
        session.last_tick = now;
    }
    if session.bytes_sent + packet.len() > BANDWIDTH_LIMIT_BPS {
        return;
    }
    session.bytes_sent += packet.len();

    route.last_seen = now;
    let dest = route.dest;
    drop(sessions);

    let _ = socket.send_to(packet, dest).await;
}

fn cleanup_routes(routes: &mut HashMap<SocketAddr, RouteEntry>) {
    let now = Instant::now();
    routes.retain(|_, route| now.duration_since(route.last_seen) <= ROUTE_IDLE_TIMEOUT);
}

async fn send_lease_ack(
    socket: &UdpSocket,
    session_id: Uuid,
    dest: SocketAddr,
    lease_ttl: Duration,
) {
    let header = RelayHeader::new(RelayPacketType::LeaseAck, session_id);
    let expires_ms = (chrono::Utc::now() + chrono::Duration::from_std(lease_ttl).unwrap())
        .timestamp_millis()
        .max(0) as u64;
    let payload = LeaseAckPayload {
        expires_ms,
        soft_limit_kbps: 10_000,
        hard_limit_kbps: 50_000,
    };

    let mut packet = [0u8; RELAY_HEADER_SIZE + LeaseAckPayload::SIZE];
    if header.encode(&mut packet).is_ok()
        && payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_ok()
    {
        let _ = socket.send_to(&packet, dest).await;
    }
}

async fn send_lease_reject(
    socket: &UdpSocket,
    session_id: Uuid,
    dest: SocketAddr,
    reason: LeaseRejectReason,
) {
    let header = RelayHeader::new(RelayPacketType::LeaseReject, session_id);
    let payload = LeaseRejectPayload { reason };
    let mut packet = [0u8; RELAY_HEADER_SIZE + LeaseRejectPayload::SIZE];
    if header.encode(&mut packet).is_ok()
        && payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_ok()
    {
        let _ = socket.send_to(&packet, dest).await;
    }
}
