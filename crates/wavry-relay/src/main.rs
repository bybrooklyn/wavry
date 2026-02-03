#![forbid(unsafe_code)]

//! Wavry Relay - blind UDP forwarder for encrypted peer traffic.
//!
//! The relay:
//! 1. Validates leases presented by peers
//! 2. Forwards encrypted packets between peers
//! 3. Never decrypts traffic (E2E encryption between peers)

mod session;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use rift_core::relay::{
    LeaseAckPayload, LeaseRejectPayload, LeaseRejectReason, RelayHeader, RelayPacketType,
    RELAY_HEADER_SIZE, RELAY_MAX_PACKET_SIZE,
};
use session::{PeerRole, SessionError, SessionPool};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default configuration values
const DEFAULT_MAX_SESSIONS: usize = 100;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;
const DEFAULT_LEASE_DURATION_SECS: u64 = 300;
const CLEANUP_INTERVAL_SECS: u64 = 10;

#[derive(Parser, Debug)]
#[command(name = "wavry-relay")]
#[command(about = "Wavry relay node - forwards encrypted UDP traffic between peers")]
struct Args {
    /// UDP listen address
    #[arg(long, default_value = "0.0.0.0:4000")]
    listen: SocketAddr,

    /// Maximum concurrent sessions
    #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
    max_sessions: usize,

    /// Session idle timeout in seconds
    #[arg(long, default_value_t = DEFAULT_IDLE_TIMEOUT_SECS)]
    idle_timeout: u64,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

/// Rate limiter for per-IP flood protection
struct IpRateLimiter {
    /// IP -> (packet_count, window_start)
    counts: HashMap<std::net::IpAddr, (u64, std::time::Instant)>,
    max_pps: u64,
    window: Duration,
}

impl IpRateLimiter {
    fn new(max_pps: u64) -> Self {
        Self {
            counts: HashMap::new(),
            max_pps,
            window: Duration::from_secs(1),
        }
    }

    fn check(&mut self, ip: std::net::IpAddr) -> bool {
        let now = std::time::Instant::now();
        let entry = self.counts.entry(ip).or_insert((0, now));

        // Reset window if expired
        if now.duration_since(entry.1) > self.window {
            *entry = (0, now);
        }

        entry.0 += 1;
        entry.0 <= self.max_pps
    }

    fn cleanup(&mut self) {
        let now = std::time::Instant::now();
        self.counts
            .retain(|_, (_, start)| now.duration_since(*start) < self.window * 2);
    }
}

/// Relay server state
struct RelayServer {
    socket: UdpSocket,
    sessions: RwLock<SessionPool>,
    ip_limiter: RwLock<IpRateLimiter>,
    lease_duration: Duration,
}

impl RelayServer {
    async fn new(listen: SocketAddr, max_sessions: usize, idle_timeout: Duration) -> Result<Self> {
        let socket = UdpSocket::bind(listen).await?;
        info!("Relay listening on {}", listen);

        Ok(Self {
            socket,
            sessions: RwLock::new(SessionPool::new(max_sessions, idle_timeout)),
            ip_limiter: RwLock::new(IpRateLimiter::new(1000)),
            lease_duration: Duration::from_secs(DEFAULT_LEASE_DURATION_SECS),
        })
    }

    async fn run(&self) -> Result<()> {
        let mut buf = vec![0u8; RELAY_MAX_PACKET_SIZE];
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));

        loop {
            tokio::select! {
                result = self.socket.recv_from(&mut buf) => {
                    let (len, src) = result?;
                    let packet = &buf[..len];

                    if let Err(e) = self.handle_packet(packet, src).await {
                        debug!("Packet from {} dropped: {}", src, e);
                    }
                }
                _ = cleanup_interval.tick() => {
                    self.cleanup().await;
                }
            }
        }
    }

    async fn handle_packet(&self, packet: &[u8], src: SocketAddr) -> Result<(), PacketError> {
        // 1. Size check
        if packet.len() < RELAY_HEADER_SIZE || packet.len() > RELAY_MAX_PACKET_SIZE {
            return Err(PacketError::InvalidSize);
        }

        // 2. Magic and version check (fast path)
        if !RelayHeader::quick_check(packet) {
            return Err(PacketError::InvalidMagic);
        }

        // 3. Per-IP rate limit
        {
            let mut limiter = self.ip_limiter.write().await;
            if !limiter.check(src.ip()) {
                return Err(PacketError::RateLimited);
            }
        }

        // 4. Parse header
        let header = RelayHeader::decode(packet).map_err(|_| PacketError::InvalidHeader)?;
        let payload = &packet[RELAY_HEADER_SIZE..];

        // 5. Dispatch by type
        match header.packet_type {
            RelayPacketType::LeasePresent => {
                self.handle_lease_present(&header, payload, src).await
            }
            RelayPacketType::LeaseRenew => {
                self.handle_lease_renew(&header, src).await
            }
            RelayPacketType::Forward => {
                self.handle_forward(&header, payload, src).await
            }
            _ => Err(PacketError::UnexpectedType),
        }
    }

    async fn handle_lease_present(
        &self,
        header: &RelayHeader,
        payload: &[u8],
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        // Parse lease present payload
        if payload.is_empty() {
            return Err(PacketError::InvalidPayload);
        }

        let peer_role = PeerRole::try_from(payload[0]).map_err(|_| PacketError::InvalidPayload)?;

        // TODO: Parse and validate PASETO lease token from payload
        // For now, accept all leases (stub implementation)
        let wavry_id = format!("peer-{}", src);

        // Get or create session
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_or_create(header.session_id, self.lease_duration)
            .map_err(|e| match e {
                SessionError::SessionFull => PacketError::SessionFull,
                _ => PacketError::SessionError,
            })?;

        // Register peer
        if let Err(e) = session.register_peer(peer_role, wavry_id, src) {
            warn!("Failed to register peer from {}: {}", src, e);
            self.send_lease_reject(header.session_id, src, LeaseRejectReason::SessionFull).await;
            return Err(PacketError::SessionError);
        }

        // Send ACK
        self.send_lease_ack(header.session_id, src, session.lease_expires)
            .await;

        info!(
            "Peer {:?} registered for session {} from {}",
            peer_role, header.session_id, src
        );

        Ok(())
    }

    async fn handle_lease_renew(
        &self,
        header: &RelayHeader,
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&header.session_id)
            .ok_or(PacketError::SessionNotFound)?;

        // Verify source is a known peer
        if session.identify_peer(src).is_none() {
            return Err(PacketError::UnknownPeer);
        }

        session.renew_lease(self.lease_duration);

        self.send_lease_ack(header.session_id, src, session.lease_expires)
            .await;

        debug!("Lease renewed for session {} by {}", header.session_id, src);

        Ok(())
    }

    async fn handle_forward(
        &self,
        header: &RelayHeader,
        payload: &[u8],
        src: SocketAddr,
    ) -> Result<(), PacketError> {
        // Get session
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&header.session_id)
            .ok_or(PacketError::SessionNotFound)?;

        // Check session is active
        if !session.is_active() {
            return Err(PacketError::SessionNotActive);
        }

        // Identify sender and get destination
        let (sender_role, _sender, dest) = session
            .identify_peer(src)
            .ok_or(PacketError::UnknownPeer)?;

        let dest_addr = dest.socket_addr;

        // TODO: Sequence number check from payload header
        // TODO: Per-peer rate limiting

        // Update sender's last seen (for NAT rebinding)
        if let Some(sender) = session.get_peer_mut(sender_role) {
            if sender.socket_addr != src {
                debug!("NAT rebinding detected for {:?}: {} -> {}", sender_role, sender.socket_addr, src);
                sender.socket_addr = src;
            }
            sender.last_seen = std::time::Instant::now();
        }

        // Record stats
        let forward_size = RELAY_HEADER_SIZE + payload.len();
        session.record_forward(forward_size);

        // Forward the entire original packet to destination
        // We need to re-encode header + payload
        let mut forward_buf = vec![0u8; RELAY_HEADER_SIZE + payload.len()];
        header.encode(&mut forward_buf).map_err(|_| PacketError::InvalidHeader)?;
        forward_buf[RELAY_HEADER_SIZE..].copy_from_slice(payload);

        drop(sessions); // Release lock before sending

        self.socket.send_to(&forward_buf, dest_addr).await?;

        Ok(())
    }

    async fn send_lease_ack(
        &self,
        session_id: uuid::Uuid,
        dest: SocketAddr,
        expires: std::time::Instant,
    ) {
        let header = RelayHeader::new(RelayPacketType::LeaseAck, session_id);

        // Calculate expiry as duration from now
        let expires_ms = expires
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let unix_expires = chrono::Utc::now().timestamp_millis() as u64 + expires_ms;

        let payload = LeaseAckPayload {
            expires_ms: unix_expires,
            soft_limit_kbps: 50_000, // 50 Mbps
            hard_limit_kbps: 100_000, // 100 Mbps
        };

        let mut packet = vec![0u8; RELAY_HEADER_SIZE + LeaseAckPayload::SIZE];
        if header.encode(&mut packet).is_err() {
            warn!("Failed to encode header");
            return;
        }
        if payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_err() {
            warn!("Failed to encode payload");
            return;
        }

        if let Err(e) = self.socket.send_to(&packet, dest).await {
            warn!("Failed to send LEASE_ACK to {}: {}", dest, e);
        }
    }

    async fn send_lease_reject(&self, session_id: uuid::Uuid, dest: SocketAddr, reason: LeaseRejectReason) {
        let header = RelayHeader::new(RelayPacketType::LeaseReject, session_id);

        let payload = LeaseRejectPayload { reason };

        let mut packet = vec![0u8; RELAY_HEADER_SIZE + LeaseRejectPayload::SIZE];
        if header.encode(&mut packet).is_err() {
            return;
        }
        if payload.encode(&mut packet[RELAY_HEADER_SIZE..]).is_err() {
            return;
        }

        if let Err(e) = self.socket.send_to(&packet, dest).await {
            warn!("Failed to send LEASE_REJECT to {}: {}", dest, e);
        }
    }

    async fn cleanup(&self) {
        let mut sessions = self.sessions.write().await;
        let cleaned = sessions.cleanup();
        if cleaned > 0 {
            info!("Cleaned up {} expired/idle sessions", cleaned);
        }

        let mut limiter = self.ip_limiter.write().await;
        limiter.cleanup();
    }
}

/// Packet handling errors
#[derive(Debug, thiserror::Error)]
enum PacketError {
    #[error("invalid packet size")]
    InvalidSize,
    #[error("invalid magic/version")]
    InvalidMagic,
    #[error("rate limited")]
    RateLimited,
    #[error("invalid header")]
    InvalidHeader,
    #[error("invalid payload")]
    InvalidPayload,
    #[error("unexpected packet type")]
    UnexpectedType,
    #[error("session not found")]
    SessionNotFound,
    #[error("session not active")]
    SessionNotActive,
    #[error("session full")]
    SessionFull,
    #[error("unknown peer")]
    UnknownPeer,
    #[error("session error")]
    SessionError,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = format!("{},hyper=warn,tokio=warn", args.log_level);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!("Starting wavry-relay v{}", env!("CARGO_PKG_VERSION"));

    let server = RelayServer::new(
        args.listen,
        args.max_sessions,
        Duration::from_secs(args.idle_timeout),
    )
    .await?;

    server.run().await
}
