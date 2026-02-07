use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rift_core::relay::{
    LeaseAckPayload, LeasePresentPayload, LeaseRejectPayload, LeaseRejectReason, RelayHeader,
    RelayPacketType, RELAY_HEADER_SIZE, RELAY_MAX_PACKET_SIZE,
};
use rift_crypto::seq_window::SequenceWindow;
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
    pub host_seq: SequenceWindow,
    pub client_seq: SequenceWindow,
}

const BANDWIDTH_LIMIT_BPS: usize = 500 * 1024;
const ROUTE_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(300);
const MAX_STRIKES: u32 = 5;
const BAN_DURATION: Duration = Duration::from_secs(300);

pub type RelayMap = Arc<RwLock<HashMap<String, RelaySession>>>;

struct IpRateLimiter {
    counts: HashMap<std::net::IpAddr, (u64, Instant)>,
    max_pps: u64,
}

impl IpRateLimiter {
    fn new(max_pps: u64) -> Self {
        Self {
            counts: HashMap::new(),
            max_pps,
        }
    }

    fn check(&mut self, ip: std::net::IpAddr) -> bool {
        let now = Instant::now();
        let entry = self.counts.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1) >= Duration::from_secs(1) {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.0 <= self.max_pps
    }

    fn cleanup(&mut self) {
        let now = Instant::now();
        self.counts
            .retain(|_, (_, start)| now.duration_since(*start) < Duration::from_secs(2));
    }
}

struct BannedIPs {
    strikes: HashMap<std::net::IpAddr, (u32, Instant)>,
}

impl BannedIPs {
    fn new() -> Self {
        Self {
            strikes: HashMap::new(),
        }
    }

    fn is_banned(&mut self, ip: std::net::IpAddr) -> bool {
        let now = Instant::now();
        if let Some((strikes, banned_at)) = self.strikes.get(&ip) {
            if *strikes >= MAX_STRIKES {
                if now.duration_since(*banned_at) < BAN_DURATION {
                    return true;
                } else {
                    self.strikes.remove(&ip);
                }
            }
        }
        false
    }

    fn add_strike(&mut self, ip: std::net::IpAddr) {
        let now = Instant::now();
        let entry = self.strikes.entry(ip).or_insert((0, now));
        entry.0 += 1;
        entry.1 = now;
        if entry.0 >= MAX_STRIKES {
            warn!(
                "IP {} banned from relay for {}s after {} strikes",
                ip,
                BAN_DURATION.as_secs(),
                MAX_STRIKES
            );
        }
    }

    fn cleanup(&mut self) {
        let now = Instant::now();
        self.strikes.retain(|_, (strikes, banned_at)| {
            if *strikes >= MAX_STRIKES {
                now.duration_since(*banned_at) < BAN_DURATION
            } else {
                now.duration_since(*banned_at) < Duration::from_secs(3600)
            }
        });
    }
}

#[derive(Clone, Debug)]
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
    let mut limiter = IpRateLimiter::new(2000);
    let mut banned = BannedIPs::new();
    let mut last_cleanup = Instant::now();

    loop {
        let (len, src_addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(err) => {
                warn!("relay recv error: {}", err);
                continue;
            }
        };

        let ip = src_addr.ip();
        if banned.is_banned(ip) {
            continue;
        }
        if !limiter.check(ip) {
            debug!("relay rate limit reached for {}", ip);
            continue;
        }

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
                if !handle_lease_present(&state, &mut routes, src_addr, &header, payload, &socket)
                    .await
                {
                    banned.add_strike(ip);
                }
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

        if last_cleanup.elapsed() >= Duration::from_secs(30) {
            cleanup_routes(&mut routes);
            limiter.cleanup();
            banned.cleanup();
            last_cleanup = Instant::now();
        }
    }
}

async fn handle_lease_present(
    state: &RelayMap,
    routes: &mut HashMap<SocketAddr, RouteEntry>,
    src: SocketAddr,
    header: &RelayHeader,
    payload: &[u8],
    socket: &UdpSocket,
) -> bool {
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
            return false;
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
            return false;
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
        return false;
    };

    if session.session_id != header.session_id {
        send_lease_reject(
            socket,
            header.session_id,
            src,
            LeaseRejectReason::WrongRelay,
        )
        .await;
        return false;
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
        return false;
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
    true
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

    // Replay protection
    let payload = &packet[RELAY_HEADER_SIZE..];
    let sequence = match extract_forward_sequence(payload) {
        Ok(seq) => seq,
        Err(err) => {
            debug!("failed to extract sequence from relayed payload: {}", err);
            return;
        }
    };

    let seq_window = match route.side {
        RouteSide::Host => &mut session.host_seq,
        RouteSide::Client => &mut session.client_seq,
    };

    if !seq_window.check_and_update(sequence) {
        debug!(
            "dropping replayed/out-of-window packet: seq={} side={:?}",
            sequence, route.side
        );
        return;
    }

    session.bytes_sent += packet.len();

    route.last_seen = now;
    let dest = route.dest;
    drop(sessions);

    let _ = socket.send_to(packet, dest).await;
}

fn extract_forward_sequence(payload: &[u8]) -> Result<u64, String> {
    use bytes::Bytes;
    use rift_core::relay::ForwardPayloadHeader;
    use rift_core::PhysicalPacket;

    if payload.starts_with(&rift_core::RIFT_MAGIC) {
        let packet = PhysicalPacket::decode(Bytes::copy_from_slice(payload))
            .map_err(|e| format!("RIFT decode error: {}", e))?;
        return Ok(packet.packet_id);
    }

    let header = ForwardPayloadHeader::decode(payload)
        .map_err(|e| format!("Forward header decode error: {}", e))?;
    Ok(header.sequence)
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
